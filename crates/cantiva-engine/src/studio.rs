//! `Studio`：GUI、控制 API 與 MCP 共用的服務物件。
//!
//! - 同一時間只有一個 worker（一個模型）常駐，避免 8GB 顯示記憶體爆掉。
//! - 合成工作依序執行；進度透過 broadcast 頻道推給 UI。
//! - 閒置超過設定分鐘數會自動卸載模型。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cantiva_audio as audio;
use cantiva_core::model::{
    Consent, EngineState, EngineStatus, SegmentRequest, SpeakRequest, Take, TrainedWeights, Voice, VoiceKind, now,
};
use cantiva_core::paths::{AppPaths, is_within, sanitize_file_stem, unique_path};
use cantiva_core::settings::Settings;
use cantiva_core::store::{QUICK_PROJECT, Store};
use cantiva_core::text::{parse_script, split_for_synthesis};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::catalog::{Catalog, EngineEntry};
use crate::worker::{SpawnOptions, Worker};
use crate::{EngineError, Result};

/// 請求從哪裡來。透過對外通道進來的請求權限比較小。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Gui,
    LocalAgent,
    Tunnel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Progress {
    Engine { engine: String, state: EngineState, error: Option<String> },
    Job { job: Uuid, label: String, done: u32, total: u32 },
    TakeCreated { take: Take },
    JobFailed { job: Uuid, message: String },
}

/// 給使用者挑選的聲音：聲音庫裡的，或引擎內建的說話人。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VoiceChoice {
    /// 在 SpeakRequest.voice 裡用的名稱。
    pub name: String,
    pub engine_id: String,
    pub source: VoiceSource,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VoiceSource {
    Builtin,
    Library,
}

pub struct StudioConfig {
    pub paths: AppPaths,
    pub engines_dir: PathBuf,
    /// 等模型載入的時間上限。
    pub load_timeout: Duration,
}

pub struct Studio {
    paths: AppPaths,
    catalog: Catalog,
    settings: Mutex<Settings>,
    store: Mutex<Store>,
    worker: tokio::sync::Mutex<Option<Worker>>,
    states: Mutex<HashMap<String, (EngineState, Option<String>)>>,
    last_used: Mutex<Instant>,
    /// 目前載入的引擎。和 worker 鎖分開，健康檢查才不會被長時間的合成卡住。
    loaded: Mutex<Option<String>>,
    progress: broadcast::Sender<Progress>,
    load_timeout: Duration,
}

impl Studio {
    pub fn open(config: StudioConfig) -> Result<Arc<Self>> {
        config.paths.ensure()?;
        let settings = Settings::load(&config.paths.settings())?;
        let store = Store::open(&config.paths.database())?;
        let catalog = Catalog::load(&config.engines_dir, &config.paths)?;
        let (progress, _) = broadcast::channel(256);
        Ok(Arc::new(Self {
            paths: config.paths,
            catalog,
            settings: Mutex::new(settings),
            store: Mutex::new(store),
            worker: tokio::sync::Mutex::new(None),
            states: Mutex::new(HashMap::new()),
            last_used: Mutex::new(Instant::now()),
            loaded: Mutex::new(None),
            progress,
            load_timeout: config.load_timeout,
        }))
    }

    /// 在背景定期檢查閒置時間，超過設定就卸載模型。
    pub fn spawn_idle_unloader(self: &Arc<Self>) {
        let studio = Arc::downgrade(self);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(30)).await;
                let Some(s) = studio.upgrade() else { break };
                let minutes = s.settings().unload_after_minutes;
                if minutes == 0 {
                    continue;
                }
                let idle = s.last_used.lock().expect("lock").elapsed();
                if idle > Duration::from_secs(minutes as u64 * 60) {
                    // try_lock：正在合成時不會卸載。
                    if let Ok(mut guard) = s.worker.try_lock()
                        && let Some(w) = guard.take() {
                            let id = w.engine_id().to_string();
                            w.shutdown().await;
                            *s.loaded.lock().expect("lock") = None;
                            s.set_state(&id, EngineState::Idle, None);
                        }
                }
            }
        });
    }

    pub fn paths(&self) -> &AppPaths {
        &self.paths
    }

    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Progress> {
        self.progress.subscribe()
    }

    pub fn settings(&self) -> Settings {
        self.settings.lock().expect("settings lock").clone()
    }

    pub fn update_settings(&self, new: Settings) -> Result<()> {
        self.update_settings_with(|s| *s = new)
    }

    /// 在鎖內讀取、修改、存檔，避免兩處同時改設定時互相覆蓋。
    pub fn update_settings_with(&self, f: impl FnOnce(&mut Settings)) -> Result<()> {
        let mut guard = self.settings.lock().expect("settings lock");
        let mut next = guard.clone();
        f(&mut next);
        if next == *guard {
            return Ok(());
        }
        next.validate()?;
        next.save(&self.paths.settings())?;
        *guard = next;
        Ok(())
    }

    pub fn output_dir(&self) -> PathBuf {
        self.settings().output_dir.unwrap_or_else(|| self.paths.default_output_dir())
    }

    pub fn with_store<T>(&self, f: impl FnOnce(&Store) -> cantiva_core::Result<T>) -> Result<T> {
        let store = self.store.lock().expect("store lock");
        Ok(f(&store)?)
    }

    pub fn with_store_mut<T>(&self, f: impl FnOnce(&mut Store) -> cantiva_core::Result<T>) -> Result<T> {
        let mut store = self.store.lock().expect("store lock");
        Ok(f(&mut store)?)
    }

    /// 把專案時間軸上的 take 依序接成一個檔案。回傳輸出路徑與總長度（秒）。
    pub fn export_project(&self, project_name: &str, gap_sec: f64, target_lufs: Option<f64>) -> Result<(PathBuf, f64)> {
        let project = self
            .with_store(|s| s.project_by_name(project_name))?
            .ok_or_else(|| EngineError::Invalid(format!("找不到專案「{project_name}」")))?;
        let takes = self.with_store(|s| s.timeline(project.id))?;
        if takes.is_empty() {
            return Err(EngineError::Invalid("時間軸上沒有任何片段".into()));
        }
        let mut clips = Vec::with_capacity(takes.len());
        for t in &takes {
            let clip = audio::read_wav(Path::new(&t.path))
                .map_err(|e| EngineError::Invalid(format!("讀不到 {}：{e}", t.path)))?;
            clips.push(clip);
        }
        // 不同引擎的取樣率不同（22.05 或 24 kHz），統一轉成最高的那個。
        let rate = clips.iter().map(|c| c.sample_rate).max().unwrap_or(24_000);
        let clips: Vec<_> = clips.iter().map(|c| audio::resample(c, rate)).collect();
        let mut program = audio::concat(&clips, gap_sec)?;
        if let Some(lufs) = target_lufs {
            audio::normalize_loudness(&mut program, lufs)?;
        }
        let dir = self.output_dir().join(sanitize_file_stem(&project.name));
        std::fs::create_dir_all(&dir).map_err(|e| cantiva_core::Error::io(&dir, e))?;
        let path = unique_path(&dir, &format!("{}-完整", project.name), "wav");
        audio::write_wav(&path, &program)?;
        Ok((path, program.duration_sec()))
    }

    // ---- 引擎 ----

    pub fn engines(&self) -> Vec<EngineStatus> {
        let states = self.states.lock().expect("states lock");
        self.catalog
            .entries()
            .iter()
            .map(|e| {
                let (state, err) = states.get(&e.manifest.id).cloned().unwrap_or_else(|| {
                    if e.is_installed() { (EngineState::Idle, None) } else { (EngineState::NotInstalled, None) }
                });
                let state = if !e.is_installed() { EngineState::NotInstalled } else { state };
                EngineStatus { manifest: e.manifest.clone(), state, last_error: err, vram_mb: None }
            })
            .collect()
    }

    /// 目前載入的引擎 id。不會等待合成完成。
    pub async fn loaded_engine(&self) -> Option<String> {
        self.loaded.lock().expect("lock").clone()
    }

    pub async fn load(&self, engine_id: &str) -> Result<()> {
        let mut guard = self.worker.lock().await;
        self.ensure_loaded(&mut guard, engine_id).await
    }

    pub async fn unload(&self) {
        let mut guard = self.worker.lock().await;
        if let Some(w) = guard.take() {
            let id = w.engine_id().to_string();
            w.shutdown().await;
            *self.loaded.lock().expect("lock") = None;
            self.set_state(&id, EngineState::Idle, None);
        }
    }

    async fn ensure_loaded(&self, slot: &mut Option<Worker>, engine_id: &str) -> Result<()> {
        let entry = self.catalog.get(engine_id)?.clone();
        let id = entry.manifest.id.clone();
        if let Some(w) = slot.as_mut()
            && w.engine_id() == id && w.is_alive() {
                return Ok(());
            }
        if let Some(old) = slot.take() {
            let old_id = old.engine_id().to_string();
            old.shutdown().await;
            *self.loaded.lock().expect("lock") = None;
            self.set_state(&old_id, EngineState::Idle, None);
        }
        self.set_state(&id, EngineState::Loading, None);
        let opts = SpawnOptions {
            pythonpath: self.catalog.worker_pythonpath(),
            log_file: self.paths.logs().join(format!("{id}.log")),
            load_timeout: self.load_timeout,
        };
        match Worker::spawn(&entry, opts).await {
            Ok(w) => {
                *slot = Some(w);
                *self.loaded.lock().expect("lock") = Some(id.clone());
                // 剛載入就算使用過，閒置卸載才不會馬上把它卸掉。
                *self.last_used.lock().expect("lock") = Instant::now();
                self.set_state(&id, EngineState::Ready, None);
                let _ = self.update_settings_with(|s| s.active_engine = Some(id));
                Ok(())
            }
            Err(e) => {
                let state = if matches!(e, EngineError::NotInstalled(_)) { EngineState::NotInstalled } else { EngineState::Failed };
                self.set_state(&id, state, Some(e.to_string()));
                Err(e)
            }
        }
    }

    fn set_state(&self, engine: &str, state: EngineState, error: Option<String>) {
        self.states.lock().expect("states lock").insert(engine.to_string(), (state, error.clone()));
        let _ = self.progress.send(Progress::Engine { engine: engine.to_string(), state, error });
    }

    // ---- 聲音 ----

    pub fn voice_choices(&self) -> Result<Vec<VoiceChoice>> {
        let mut out = Vec::new();
        for v in self.with_store(|s| s.voices())? {
            let language = match &v.kind {
                VoiceKind::Reference { language: Some(l), .. } => l.clone(),
                _ => String::new(),
            };
            let description = match &v.kind {
                VoiceKind::Reference { transcript, trained: Some(_), .. } => {
                    format!("角色模型＋參考音檔；逐字稿：{}", truncate(transcript, 30))
                }
                VoiceKind::Reference { transcript, .. } => format!("參考音檔複製；逐字稿：{}", truncate(transcript, 30)),
                VoiceKind::Builtin { speaker } => format!("內建說話人 {speaker}"),
            };
            out.push(VoiceChoice { name: v.name, engine_id: v.engine_id, source: VoiceSource::Library, language, description });
        }
        for e in self.catalog.entries() {
            for b in &e.manifest.builtin_voices {
                out.push(VoiceChoice {
                    name: format!("{}/{}", e.manifest.id, b.id),
                    engine_id: e.manifest.id.clone(),
                    source: VoiceSource::Builtin,
                    language: b.language.clone(),
                    description: format!("{}（{}）", b.label, b.gender),
                });
            }
        }
        Ok(out)
    }

    /// 匯入參考音檔，建立可複製的聲音。音檔會複製到 references 資料夾。
    pub fn create_reference_voice(
        &self,
        name: &str,
        engine_id: &str,
        source_audio: &Path,
        transcript: &str,
        consent: Consent,
    ) -> Result<Voice> {
        self.create_voice(NewVoice {
            name: name.into(),
            engine_id: engine_id.into(),
            audio: source_audio.to_path_buf(),
            transcript: transcript.into(),
            language: None,
            gpt_weights: None,
            sovits_weights: None,
            consent,
        })
    }

    /// 建立聲音：參考音檔，以及（GPT-SoVITS）選填的角色模型權重。所有檔案都複製一份到 references。
    /// 由 agent（控制 API）建立聲音時使用：檢查設定是否允許。
    pub fn create_voice_from(&self, input: NewVoice, origin: Origin) -> Result<Voice> {
        match origin {
            Origin::Gui => {}
            Origin::Tunnel => return Err(EngineError::Invalid("透過對外通道不能建立複製聲音".into())),
            Origin::LocalAgent if !self.settings().mcp.allow_clone_tools => {
                return Err(EngineError::Invalid("設定裡關閉了 agent 使用複製聲音".into()));
            }
            Origin::LocalAgent => {}
        }
        self.create_voice(input)
    }

    pub fn create_voice(&self, input: NewVoice) -> Result<Voice> {
        let entry = self.catalog.get(&input.engine_id)?;
        let caps = &entry.manifest.capabilities;
        if !caps.clone {
            return Err(EngineError::Invalid(format!("{} 不支援用參考音檔複製聲音", entry.manifest.name)));
        }
        if input.transcript.trim().is_empty() {
            return Err(EngineError::Invalid("請填寫參考音檔的逐字稿，要和音檔內容完全一致".into()));
        }
        if input.consent.statement.trim().is_empty() {
            return Err(EngineError::Invalid("請寫下同意說明（例如「我本人的聲音」）".into()));
        }
        let has_weights = input.gpt_weights.is_some() || input.sovits_weights.is_some();
        if has_weights && !caps.trained_models {
            return Err(EngineError::Invalid(format!("{} 不能匯入角色模型權重", entry.manifest.name)));
        }
        if has_weights && (input.gpt_weights.is_none() || input.sovits_weights.is_none()) {
            return Err(EngineError::Invalid("角色模型需要兩個檔案：GPT 權重（.ckpt）與 SoVITS 權重（.pth）".into()));
        }
        let clip = audio::read_wav(&input.audio)
            .map_err(|e| EngineError::Invalid(format!("參考音檔讀不出來（目前只接受 WAV）：{e}")))?;
        let secs = clip.duration_sec();
        if !(2.0..=30.0).contains(&secs) {
            return Err(EngineError::Invalid(format!("參考音檔長度 {secs:.1} 秒；請剪成 3–15 秒的乾淨人聲")));
        }
        let refs = self.paths.references();
        let stem = sanitize_file_stem(&input.name);
        let copy_into = |src: &Path, suffix: &str, ext: &str| -> Result<String> {
            let dest = unique_path(&refs, &format!("{stem}{suffix}"), ext);
            std::fs::copy(src, &dest).map_err(|e| cantiva_core::Error::io(&dest, e))?;
            Ok(dest.file_name().and_then(|f| f.to_str()).unwrap_or_default().to_string())
        };
        let check_ext = |p: &Path, want: &str| -> Result<()> {
            let ok = p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case(want));
            if ok { Ok(()) } else { Err(EngineError::Invalid(format!("{} 應該是 .{want} 檔", p.display()))) }
        };
        let trained = match (&input.gpt_weights, &input.sovits_weights) {
            (Some(gpt), Some(sovits)) => {
                check_ext(gpt, "ckpt")?;
                check_ext(sovits, "pth")?;
                Some(TrainedWeights { gpt_file: copy_into(gpt, "-gpt", "ckpt")?, sovits_file: copy_into(sovits, "-sovits", "pth")? })
            }
            _ => None,
        };
        let audio_file = copy_into(&input.audio, "", "wav")?;
        let kind = VoiceKind::Reference {
            audio_file,
            transcript: input.transcript.trim().to_string(),
            consent: input.consent,
            language: input.language.filter(|l| !l.trim().is_empty()),
            trained,
        };
        let id = entry.manifest.id.clone();
        self.with_store(|s| s.add_voice(&input.name, &id, kind, ""))
    }

    /// 把使用者給的聲音名稱解析成 (引擎, 說話人或參考音檔)。
    fn resolve_voice(&self, key: Option<&str>, engine_hint: Option<&str>) -> Result<ResolvedVoice> {
        let default_engine = engine_hint
            .map(str::to_string)
            .or_else(|| self.settings().active_engine)
            .unwrap_or_else(|| "qwen3-tts".into());
        let Some(key) = key.map(str::trim).filter(|k| !k.is_empty()) else {
            let entry = self.catalog.get(&default_engine)?;
            let speaker = entry.manifest.builtin_voices.first().map(|b| b.id.clone());
            if speaker.is_none() {
                return Err(EngineError::Invalid(format!(
                    "{} 沒有內建聲音，請指定聲音庫裡的參考聲音",
                    entry.manifest.name
                )));
            }
            let display = format!("{}/{}", entry.manifest.id, speaker.as_deref().unwrap_or_default());
            return Ok(ResolvedVoice { engine_id: entry.manifest.id.clone(), display, speaker, reference: None });
        };
        if let Some(v) = self.with_store(|s| s.find_voice(key))? {
            return Ok(match v.kind {
                VoiceKind::Builtin { speaker } => {
                    ResolvedVoice { engine_id: v.engine_id, display: v.name, speaker: Some(speaker), reference: None }
                }
                VoiceKind::Reference { audio_file, transcript, language, trained, .. } => {
                    let refs = self.paths.references();
                    let path = refs.join(&audio_file);
                    if !is_within(&refs, &path) {
                        return Err(EngineError::Invalid(format!("聲音「{}」的參考音檔不見了", v.name)));
                    }
                    let weights = match trained {
                        Some(t) => {
                            let (g, s) = (refs.join(&t.gpt_file), refs.join(&t.sovits_file));
                            if !is_within(&refs, &g) || !is_within(&refs, &s) {
                                return Err(EngineError::Invalid(format!("聲音「{}」的模型權重檔不見了", v.name)));
                            }
                            Some((g, s))
                        }
                        None => None,
                    };
                    ResolvedVoice {
                        engine_id: v.engine_id,
                        display: v.name,
                        speaker: None,
                        reference: Some(RefVoice { path, text: transcript, language, weights }),
                    }
                }
            });
        }
        // 內建說話人：可寫「引擎/說話人」或只寫說話人。
        let (engine_part, speaker_part) = match key.split_once('/') {
            Some((e, s)) => (e.to_string(), s),
            None => (default_engine, key),
        };
        let entry = self.catalog.get(&engine_part)?;
        let found = entry
            .manifest
            .builtin_voices
            .iter()
            .find(|b| b.id.eq_ignore_ascii_case(speaker_part) || b.label == speaker_part)
            .ok_or_else(|| EngineError::UnknownVoice(key.to_string()))?;
        Ok(ResolvedVoice {
            engine_id: entry.manifest.id.clone(),
            display: format!("{}/{}", entry.manifest.id, found.id),
            speaker: Some(found.id.clone()),
            reference: None,
        })
    }

    // ---- 合成 ----

    pub async fn speak(&self, req: SpeakRequest, origin: Origin) -> Result<Take> {
        let job = Uuid::new_v4();
        match self.speak_inner(job, req, origin).await {
            Ok(t) => Ok(t),
            Err(e) => {
                let _ = self.progress.send(Progress::JobFailed { job, message: e.to_string() });
                Err(e)
            }
        }
    }

    async fn speak_inner(&self, job: Uuid, req: SpeakRequest, origin: Origin) -> Result<Take> {
        let settings = self.settings();
        let text = req.text.trim();
        if text.is_empty() {
            return Err(EngineError::Invalid("文字是空的".into()));
        }
        let limit = settings.mcp.max_chars_per_request;
        if origin != Origin::Gui && text.chars().count() > limit {
            return Err(EngineError::Invalid(format!(
                "一次最多 {limit} 字（這次 {} 字）。請分段呼叫，或在設定裡調高上限",
                text.chars().count()
            )));
        }
        let voice = self.resolve_voice(req.voice.as_deref(), req.engine.as_deref())?;
        if voice.reference.is_some() {
            if origin == Origin::Tunnel && !(settings.tunnel.allow_clone_over_tunnel && settings.mcp.allow_clone_tools) {
                return Err(EngineError::Invalid("透過對外通道不允許使用複製的聲音（可在設定頁開啟）".into()));
            }
            if origin == Origin::LocalAgent && !settings.mcp.allow_clone_tools {
                return Err(EngineError::Invalid("設定裡關閉了 agent 使用複製聲音".into()));
            }
        }
        let entry: EngineEntry = self.catalog.get(&voice.engine_id)?.clone();
        let caps = &entry.manifest.capabilities;
        let segments = split_for_synthesis(text, entry.manifest.chunk_chars);
        let total = segments.len() as u32;

        let project_name = req.project.clone().unwrap_or_else(|| QUICK_PROJECT.to_string());
        let project = self.with_store(|s| s.ensure_project(&project_name))?;
        let tmp_dir = self.paths.root().join("tmp").join(job.to_string());
        std::fs::create_dir_all(&tmp_dir).map_err(|e| cantiva_core::Error::io(&tmp_dir, e))?;

        let started = Instant::now();
        let mut guard = self.worker.lock().await;
        let _ = self.progress.send(Progress::Job { job, label: format!("載入 {}", entry.manifest.name), done: 0, total });
        if let Err(e) = self.ensure_loaded(&mut guard, &entry.manifest.id).await {
            let _ = std::fs::remove_dir_all(&tmp_dir);
            return Err(e);
        }
        let worker = guard.as_ref().expect("worker loaded");
        self.set_state(&entry.manifest.id, EngineState::Busy, None);

        let mut clips = Vec::with_capacity(segments.len());
        let result: Result<()> = async {
            for (i, seg) in segments.iter().enumerate() {
                let _ = self.progress.send(Progress::Job {
                    job,
                    label: format!("合成第 {}／{} 段", i + 1, total),
                    done: i as u32,
                    total,
                });
                let out = tmp_dir.join(format!("seg-{i:04}.wav"));
                let seg_req = SegmentRequest {
                    text: seg.clone(),
                    speaker: voice.speaker.clone(),
                    reference_audio: voice.reference.as_ref().map(|r| r.path.display().to_string()),
                    reference_text: voice.reference.as_ref().map(|r| r.text.clone()),
                    reference_language: voice.reference.as_ref().and_then(|r| r.language.clone()),
                    gpt_weights: voice.reference.as_ref().and_then(|r| r.weights.as_ref().map(|w| w.0.display().to_string())),
                    sovits_weights: voice.reference.as_ref().and_then(|r| r.weights.as_ref().map(|w| w.1.display().to_string())),
                    // IndexTTS 會把語氣指示裡的情緒字樣（難過、開心…）對應成情緒向量。
                    instruct: if caps.instruct || caps.emotion { req.instruct.clone() } else { None },
                    emotion: if caps.emotion { req.emotion.clone() } else { None },
                    speed: if caps.speed { req.speed } else { None },
                    language: req.language.clone(),
                    seed: req.seed,
                    output_path: out.display().to_string(),
                };
                let res = worker.synthesize(&seg_req).await?;
                let clip = audio::read_wav(Path::new(&res.path))?;
                clips.push(audio::trim_silence(&clip, 0.003, 0.06));
            }
            Ok(())
        }
        .await;
        *self.last_used.lock().expect("lock") = Instant::now();
        if let Err(e) = result {
            let _ = std::fs::remove_dir_all(&tmp_dir);
            let alive = guard.as_mut().is_some_and(|w| w.is_alive());
            if alive {
                self.set_state(&entry.manifest.id, EngineState::Ready, None);
            } else {
                guard.take();
                *self.loaded.lock().expect("lock") = None;
                self.set_state(&entry.manifest.id, EngineState::Failed, Some(format!("worker 意外結束：{e}")));
            }
            return Err(e);
        }
        self.set_state(&entry.manifest.id, EngineState::Ready, None);

        let mut joined = audio::concat(&clips, 0.22)?;
        let normalize = req.normalize.unwrap_or(settings.normalize_loudness);
        let loudness = if normalize {
            audio::normalize_loudness(&mut joined, settings.target_lufs)?
        } else {
            audio::integrated_loudness(&joined)?
        };
        let out_dir = self.output_dir().join(sanitize_file_stem(&project.name));
        std::fs::create_dir_all(&out_dir).map_err(|e| cantiva_core::Error::io(&out_dir, e))?;
        let stem = req.file_stem.clone().unwrap_or_else(|| default_stem(text));
        let path = unique_path(&out_dir, &stem, "wav");
        audio::write_wav(&path, &joined)?;
        drop(guard);
        let _ = std::fs::remove_dir_all(&tmp_dir);

        let take = Take {
            id: Uuid::new_v4(),
            project_id: project.id,
            text: text.to_string(),
            voice_name: voice.display,
            engine_id: entry.manifest.id.clone(),
            path: path.display().to_string(),
            duration_sec: joined.duration_sec(),
            sample_rate: joined.sample_rate,
            segments: total,
            elapsed_sec: started.elapsed().as_secs_f64(),
            loudness_lufs: loudness,
            starred: false,
            created_at: now(),
        };
        self.with_store(|s| s.add_take(&take))?;
        let _ = self.progress.send(Progress::Job { job, label: "完成".into(), done: total, total });
        let _ = self.progress.send(Progress::TakeCreated { take: take.clone() });
        Ok(take)
    }

    /// 依腳本逐行合成，每行一個 take。`default_voice` 用在沒寫 `@聲音：` 的行。
    pub async fn render_script(
        &self,
        project_name: &str,
        script: &str,
        default_voice: Option<&str>,
        origin: Origin,
    ) -> Result<Vec<Take>> {
        let lines = parse_script(script);
        if lines.is_empty() {
            return Err(EngineError::Invalid("腳本裡沒有可以唸的內容".into()));
        }
        if origin != Origin::Gui {
            let limit = self.settings().mcp.max_chars_per_request;
            let total: usize = lines.iter().map(|l| l.text.chars().count()).sum();
            if total > limit || lines.len() > 200 {
                return Err(EngineError::Invalid(format!(
                    "腳本一次最多 {limit} 字、200 行（這次 {total} 字、{} 行）。請分成幾段呼叫",
                    lines.len()
                )));
            }
        }
        let project = self.with_store(|s| s.ensure_project(project_name))?;
        self.with_store(|s| s.save_script(project.id, script))?;
        let mut takes = Vec::with_capacity(lines.len());
        for line in lines {
            let req = SpeakRequest {
                text: line.text.clone(),
                voice: line.voice.clone().or_else(|| default_voice.map(str::to_string)),
                instruct: line.instruct.clone(),
                project: Some(project.name.clone()),
                file_stem: Some(format!("{:03}-{}", line.index, default_stem(&line.text))),
                ..Default::default()
            };
            takes.push(self.speak(req, origin).await?);
        }
        Ok(takes)
    }
}

struct ResolvedVoice {
    engine_id: String,
    display: String,
    speaker: Option<String>,
    reference: Option<RefVoice>,
}

struct RefVoice {
    path: PathBuf,
    text: String,
    language: Option<String>,
    /// (GPT 權重, SoVITS 權重)
    weights: Option<(PathBuf, PathBuf)>,
}

/// 建立聲音的輸入。
#[derive(Debug, Clone)]
pub struct NewVoice {
    pub name: String,
    pub engine_id: String,
    pub audio: PathBuf,
    pub transcript: String,
    pub language: Option<String>,
    pub gpt_weights: Option<PathBuf>,
    pub sovits_weights: Option<PathBuf>,
    pub consent: Consent,
}

fn truncate(s: &str, n: usize) -> String {
    let mut out: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        out.push('…');
    }
    out
}

/// 用文字開頭幾個字當檔名，方便在檔案總管裡辨認。
fn default_stem(text: &str) -> String {
    let head: String = text.chars().filter(|c| c.is_alphanumeric() || *c == ' ').take(16).collect();
    let head = head.trim();
    if head.is_empty() { "take".into() } else { head.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_stem_uses_text_head() {
        assert_eq!(default_stem("今天天氣很好，我們出門吧！"), "今天天氣很好我們出門吧");
        assert_eq!(default_stem("。。。"), "take");
    }
}
