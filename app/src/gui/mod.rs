//! 桌面介面：把 Slint 的 AppState 接到 Studio。
//!
//! 執行緒模型：所有碰到 Slint 的程式都在 UI 執行緒（事件迴圈）。合成、匯出等耗時工作
//! 丟到 tokio，結束後用 `slint::invoke_from_event_loop` 回到 UI 執行緒，透過 `with_app` 取得控制器。

slint::include_modules!();

mod audio_out;
mod clients;
mod platform;
mod tunnel;
mod waveform;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use cantiva_audio::{self as audio, Clip};
use cantiva_core::model::{Consent, ConsentBasis, EmotionSpec, EngineState, Project, SpeakRequest, Take, now};
use cantiva_core::text::{estimate_seconds, parse_script};
use cantiva_engine::Studio;
use cantiva_engine::studio::{NewVoice, Origin, Progress, VoiceChoice, VoiceSource};
use slint::{ComponentHandle, Image, Model, ModelRc, SharedString, VecModel};

use crate::runtime::Runtime;
use audio_out::AudioOut;
use tunnel::{Tunnel, TunnelEvent};

thread_local! {
    static APP: RefCell<Option<Rc<App>>> = const { RefCell::new(None) };
}

fn with_app(f: impl FnOnce(&Rc<App>)) {
    let app = APP.with(|a| a.borrow().clone());
    if let Some(app) = app {
        f(&app);
    }
}

const LANGUAGES: [(&str, &str); 6] = [("", "自動判斷"), ("zh", "中文"), ("ja", "日文"), ("en", "英文"), ("ko", "韓文"), ("yue", "粵語")];
const EMOTIONS: [&str; 9] = ["", "開心", "生氣", "難過", "害怕", "厭惡", "低落", "驚訝", "平靜"];

#[derive(PartialEq, Eq, Clone, Copy)]
enum PlayMode {
    Idle,
    Program,
    Preview,
}

struct TimelineClip {
    take: Take,
}

struct State {
    projects: Vec<Project>,
    project: usize,
    voices: Vec<VoiceChoice>,
    timeline: Vec<TimelineClip>,
    cache: HashMap<String, (Arc<Clip>, Image)>,
    program: Option<(Arc<Clip>, Vec<f32>)>,
    mode: PlayMode,
    audio: AudioOut,
    peak: (f32, Instant),
    script_dirty: Option<Instant>,
    ref_file: Option<PathBuf>,
    gpt_file: Option<PathBuf>,
    sovits_file: Option<PathBuf>,
    exports: Vec<ExportItem>,
    tunnel: Option<Tunnel>,
    gpu: Option<(String, u64)>,
    toast_timer: slint::Timer,
    clone_engine_ids: Vec<String>,
    about: Option<AboutWindow>,
}

pub struct App {
    ui: slint::Weak<AppWindow>,
    rt: Runtime,
    state: RefCell<State>,
    /// 介面發起、還沒結束的工作數。agent 發起的工作只會短暫顯示進度，不會鎖住介面。
    gui_jobs: std::cell::Cell<u32>,
    /// 對外通道的世代編號：舊通道的事件不會影響新通道。
    tunnel_gen: std::cell::Cell<u64>,
}

/// 讓 agent 透過控制 API 請 Cantiva 播放（跨執行緒轉回 UI 執行緒）。
struct GuiPlayer;

impl cantiva_api::Player for GuiPlayer {
    fn play(&self, path: &Path) -> Result<(), String> {
        let path = path.to_path_buf();
        slint::invoke_from_event_loop(move || with_app(|a| a.play_file(&path))).map_err(|e| e.to_string())
    }
}

pub fn run(mut rt: Runtime) -> anyhow::Result<()> {
    if rt.other_instance {
        // 可能是 agent 先以無視窗模式啟動的；請它讓出，再由主視窗接手。
        let client = cantiva_api::ApiClient::from_paths(&rt.paths);
        let asked = rt.tokio.block_on(async {
            match client {
                Some(c) => c.shutdown().await.is_ok(),
                None => false,
            }
        });
        if !(asked && rt.acquire_instance_lock(Duration::from_secs(15))) {
            rfd::MessageDialog::new()
                .set_title("Cantiva")
                .set_description("Cantiva 已經開著了。請切換到已開啟的視窗。")
                .set_level(rfd::MessageLevel::Info)
                .show();
            return Ok(());
        }
    }

    let ui = AppWindow::new()?;
    let app = Rc::new(App {
        ui: ui.as_weak(),
        state: RefCell::new(State {
            projects: Vec::new(),
            project: 0,
            voices: Vec::new(),
            timeline: Vec::new(),
            cache: HashMap::new(),
            program: None,
            mode: PlayMode::Idle,
            audio: AudioOut::new(),
            peak: (0.0, Instant::now()),
            script_dirty: None,
            ref_file: None,
            gpt_file: None,
            sovits_file: None,
            exports: Vec::new(),
            tunnel: None,
            gpu: None,
            toast_timer: slint::Timer::default(),
            clone_engine_ids: Vec::new(),
            about: None,
        }),
        rt,
        gui_jobs: std::cell::Cell::new(0),
        tunnel_gen: std::cell::Cell::new(0),
    });
    APP.with(|a| *a.borrow_mut() = Some(app.clone()));

    wire_callbacks(&ui);
    let port = app.rt.serve_api(Some(Arc::new(GuiPlayer)), None)?;
    app.listen_progress();
    app.detect_gpu();

    let last = app.studio().settings().last_project;
    app.refresh_projects(last.as_deref());
    app.refresh_voices();
    app.refresh_engines();
    app.refresh_connect(port);
    app.load_project_view();
    // 開發與截圖用：CANTIVA_PAGE=0..5 指定啟動時的頁面。
    if let Some(page) = std::env::var("CANTIVA_PAGE").ok().and_then(|p| p.parse::<i32>().ok()) {
        ui.global::<AppState>().set_page(page.clamp(0, 5));
    }

    // 播放頭與音量表：約 30 fps。
    let tick = slint::Timer::default();
    tick.start(slint::TimerMode::Repeated, Duration::from_millis(33), || with_app(|a| a.tick()));
    // 腳本自動存檔與引擎狀態。
    let slow = slint::Timer::default();
    slow.start(slint::TimerMode::Repeated, Duration::from_secs(1), || with_app(|a| a.slow_tick()));

    ui.run()?;

    // 關閉：停掉通道與模型。
    let app = APP.with(|a| a.borrow_mut().take()).expect("app");
    app.save_script_now();
    if let Some(t) = app.state.borrow_mut().tunnel.take() {
        t.stop();
    }
    app.rt.tokio.block_on(app.rt.studio.unload());
    Ok(())
}

fn wire_callbacks(ui: &AppWindow) {
    let s = ui.global::<AppState>();
    s.on_dismiss_toast(|| with_app(|a| a.toast("", false)));
    s.on_show_about(|| with_app(|a| a.show_about()));
    s.on_select_project(|i| with_app(|a| a.select_project(i as usize)));
    s.on_create_project(|name| with_app(|a| a.create_project(&name)));
    s.on_script_edited(|text| with_app(|a| a.script_edited(&text)));
    s.on_render_script(|| with_app(|a| a.render_script()));
    s.on_render_line(|i| with_app(|a| a.render_line(i as usize)));
    s.on_quick_voice_changed(|i| with_app(|a| a.update_quick_hint(i)));
    s.on_quick_speak(|| with_app(|a| a.quick_speak()));
    s.on_select_clip(|id| with_app(|a| a.ui().global::<AppState>().set_selected_clip(id)));
    s.on_play_pause(|| with_app(|a| a.play_pause()));
    s.on_stop(|| with_app(|a| a.stop_playback(true)));
    s.on_seek(|sec| with_app(|a| a.seek(sec as f64)));
    s.on_to_start(|| with_app(|a| a.seek(0.0)));
    s.on_to_end(|| with_app(|a| a.seek(a.ui().global::<AppState>().get_total_seconds() as f64)));
    s.on_move_clip(|d| with_app(|a| a.move_clip(d)));
    s.on_remove_clip(|| with_app(|a| a.remove_clip()));
    s.on_regenerate_clip(|| with_app(|a| a.regenerate_clip()));
    s.on_star_clip(|on| with_app(|a| a.star_clip(on)));
    s.on_open_clip_folder(|| with_app(|a| a.open_clip_folder()));
    s.on_voice_engine_changed(|i| with_app(|a| a.voice_engine_changed(i)));
    s.on_pick_reference(|| with_app(|a| a.pick_reference()));
    s.on_pick_gpt_weights(|| with_app(|a| a.pick_weights(true)));
    s.on_pick_sovits_weights(|| with_app(|a| a.pick_weights(false)));
    s.on_clear_trained_weights(|| with_app(|a| a.clear_weights()));
    s.on_create_voice(|| with_app(|a| a.create_voice()));
    s.on_delete_voice(|name| with_app(|a| a.delete_voice(&name)));
    s.on_preview_voice(|name| with_app(|a| a.preview_voice(&name)));
    s.on_export_project(|| with_app(|a| a.export_project()));
    s.on_open_output_dir(|| with_app(|a| a.open_output_dir()));
    s.on_reveal(|p| with_app(|a| a.report(platform::reveal(Path::new(p.as_str())))));
    s.on_install_engine(|id| with_app(|a| a.install_engine(&id)));
    s.on_load_engine(|id| with_app(|a| a.load_engine(&id)));
    s.on_unload_engine(|| with_app(|a| a.unload_engine()));
    s.on_open_engine_log(|id| with_app(|a| a.open_engine_log(&id)));
    s.on_refresh_engines(|| with_app(|a| a.refresh_engines()));
    s.on_copy_text(|t| with_app(|a| a.copy_text(&t)));
    s.on_toggle_tunnel(|on| with_app(|a| a.toggle_tunnel(on)));
    s.on_tunnel_clone_changed(|on| with_app(|a| a.tunnel_clone_changed(on)));
    s.on_install_skill(|| with_app(|a| a.install_skill()));
}

fn model<T: Clone + 'static>(items: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(items))
}

fn strings(items: impl IntoIterator<Item = String>) -> ModelRc<SharedString> {
    model(items.into_iter().map(SharedString::from).collect())
}

fn timecode(sec: f64) -> String {
    let sec = sec.max(0.0);
    let cs = (sec * 100.0).round() as u64;
    format!("{:02}:{:02}:{:02}.{:02}", cs / 360_000, (cs / 6000) % 60, (cs / 100) % 60, cs % 100)
}

fn state_label(state: EngineState) -> (&'static str, &'static str) {
    match state {
        EngineState::NotInstalled => ("not_installed", "未安裝"),
        EngineState::Idle => ("idle", "已安裝"),
        EngineState::Loading => ("loading", "載入中"),
        EngineState::Ready => ("ready", "就緒"),
        EngineState::Busy => ("busy", "合成中"),
        EngineState::Failed => ("failed", "啟動失敗"),
    }
}

impl App {
    fn ui(&self) -> AppWindow {
        self.ui.upgrade().expect("視窗已關閉")
    }

    fn show_about(&self) {
        match AboutWindow::new() {
            Ok(w) => {
                w.set_data_dir(self.rt.paths.root().display().to_string().into());
                let _ = w.show();
                // 視窗關閉前保留參照，否則會立刻被釋放。
                self.state.borrow_mut().about = Some(w);
            }
            Err(e) => self.toast(&e.to_string(), true),
        }
    }

    fn studio(&self) -> &Arc<Studio> {
        &self.rt.studio
    }

    fn toast(&self, text: &str, error: bool) {
        let ui = self.ui();
        let g = ui.global::<AppState>();
        g.set_toast(text.into());
        g.set_toast_error(error);
        if !text.is_empty() {
            let timer = &self.state.borrow().toast_timer;
            timer.start(slint::TimerMode::SingleShot, Duration::from_secs(if error { 12 } else { 6 }), || {
                with_app(|a| a.toast("", false))
            });
        }
    }

    fn report(&self, r: Result<(), String>) {
        if let Err(e) = r {
            self.toast(&e, true);
        }
    }

    fn set_busy(&self, busy: bool, label: &str) {
        let ui = self.ui();
        let g = ui.global::<AppState>();
        g.set_busy(busy);
        g.set_job_label(label.into());
        if busy {
            g.set_job_progress(0.0);
        }
    }

    /// 在 tokio 上跑耗時工作，結束後回到 UI 執行緒處理結果。
    fn spawn<T, F>(&self, label: &str, fut: F, done: impl FnOnce(&Rc<App>, T) + Send + 'static)
    where
        T: Send + 'static,
        F: std::future::Future<Output = T> + Send + 'static,
    {
        self.gui_jobs.set(self.gui_jobs.get() + 1);
        self.set_busy(true, label);
        self.rt.tokio.spawn(async move {
            let result = fut.await;
            let _ = slint::invoke_from_event_loop(move || {
                with_app(|a| {
                    a.gui_jobs.set(a.gui_jobs.get().saturating_sub(1));
                    if a.gui_jobs.get() == 0 {
                        a.set_busy(false, "");
                    }
                    done(a, result);
                })
            });
        });
    }

    fn listen_progress(&self) {
        let mut rx = self.studio().subscribe();
        self.rt.tokio.spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(ev) => {
                        let _ = slint::invoke_from_event_loop(move || with_app(|a| a.on_progress(ev)));
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
        });
    }

    fn on_progress(&self, ev: Progress) {
        let ui = self.ui();
        let g = ui.global::<AppState>();
        match ev {
            Progress::Job { label, done, total, .. } => {
                // 介面自己的工作由 spawn 控制忙碌狀態；agent 的工作完成時要記得解除。
                g.set_busy(done < total || self.gui_jobs.get() > 0);
                g.set_job_label(label.into());
                g.set_job_progress(if total == 0 { 0.0 } else { done as f32 / total as f32 });
            }
            Progress::Engine { .. } => self.refresh_engines(),
            Progress::TakeCreated { take } => {
                // agent 透過 MCP 合成的結果也會即時出現在時間軸上。
                let current = self.current_project().map(|p| p.id);
                if current == Some(take.project_id) {
                    self.load_timeline();
                } else {
                    self.refresh_projects(None);
                }
            }
            Progress::JobFailed { message, .. } => {
                g.set_busy(self.gui_jobs.get() > 0);
                self.toast(&message, true);
            }
        }
    }

    fn detect_gpu(&self) {
        self.rt.tokio.spawn(async {
            let gpu = tokio::task::spawn_blocking(platform::detect_gpu).await.ok().flatten();
            let _ = slint::invoke_from_event_loop(move || {
                with_app(|a| {
                    a.state.borrow_mut().gpu = gpu;
                    a.refresh_engines();
                })
            });
        });
    }

    // ---- 專案與腳本 ----

    fn current_project(&self) -> Option<Project> {
        let st = self.state.borrow();
        st.projects.get(st.project).cloned()
    }

    fn refresh_projects(&self, select_name: Option<&str>) {
        let projects = match self.studio().with_store(|s| s.projects()) {
            Ok(p) => p,
            Err(e) => return self.toast(&e.to_string(), true),
        };
        let current = select_name.map(str::to_string).or_else(|| self.current_project().map(|p| p.name));
        let index = current.and_then(|n| projects.iter().position(|p| p.name == n)).unwrap_or(0);
        let names: Vec<String> = projects.iter().map(|p| p.name.clone()).collect();
        {
            let mut st = self.state.borrow_mut();
            st.projects = projects;
            st.project = index;
        }
        let ui = self.ui();
        let g = ui.global::<AppState>();
        g.set_projects(strings(names.clone()));
        g.set_project_index(index as i32);
        g.set_project_name(names.get(index).cloned().unwrap_or_default().into());
    }

    fn select_project(&self, index: usize) {
        self.save_script_now();
        self.stop_playback(true);
        self.state.borrow_mut().project = index;
        let name = self.current_project().map(|p| p.name).unwrap_or_default();
        let ui = self.ui();
        let g = ui.global::<AppState>();
        g.set_project_index(index as i32);
        g.set_project_name(name.into());
        g.set_selected_clip("".into());
        self.load_project_view();
    }

    fn create_project(&self, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        match self.studio().with_store(|s| s.ensure_project(name)) {
            Ok(_) => {
                self.save_script_now();
                self.refresh_projects(Some(name));
                self.load_project_view();
            }
            Err(e) => self.toast(&e.to_string(), true),
        }
    }

    fn load_project_view(&self) {
        let project = self.current_project();
        if let Some(p) = &project {
            let _ = self.studio().update_settings_with(|s| s.last_project = Some(p.name.clone()));
        }
        let script = project.map(|p| p.script).unwrap_or_default();
        self.ui().global::<AppState>().set_script_text(script.clone().into());
        self.update_script_lines(&script);
        self.load_timeline();
        self.refresh_output_dir();
    }

    fn script_edited(&self, text: &str) {
        self.update_script_lines(text);
        self.state.borrow_mut().script_dirty = Some(Instant::now());
    }

    fn update_script_lines(&self, text: &str) {
        let lines = parse_script(text);
        let total: f64 = lines.iter().map(|l| estimate_seconds(&l.text)).sum();
        let items: Vec<ScriptLineItem> = lines
            .iter()
            .map(|l| ScriptLineItem {
                index: l.index as i32,
                voice: l.voice.clone().unwrap_or_default().into(),
                instruct: l.instruct.clone().unwrap_or_default().into(),
                text: l.text.clone().into(),
                seconds: format!("約 {:.0} 秒", estimate_seconds(&l.text).max(1.0)).into(),
            })
            .collect();
        let ui = self.ui();
        let g = ui.global::<AppState>();
        g.set_script_summary(
            if lines.is_empty() {
                "還沒有台詞".to_string()
            } else {
                format!("{} 行，預估 {} 分 {:02} 秒", lines.len(), total as u64 / 60, total as u64 % 60)
            }
            .into(),
        );
        g.set_script_lines(model(items));
    }

    fn save_script_now(&self) {
        let dirty = self.state.borrow_mut().script_dirty.take();
        if dirty.is_none() {
            return;
        }
        let Some(project) = self.current_project() else { return };
        let text = self.ui().global::<AppState>().get_script_text().to_string();
        if let Err(e) = self.studio().with_store(|s| s.save_script(project.id, &text)) {
            self.toast(&format!("腳本存檔失敗：{e}"), true);
        } else if let Some(p) = self.state.borrow_mut().projects.iter_mut().find(|p| p.id == project.id) {
            p.script = text;
        }
    }

    fn slow_tick(&self) {
        let due = self.state.borrow().script_dirty.is_some_and(|t| t.elapsed() > Duration::from_millis(800));
        if due {
            self.save_script_now();
        }
        if self.ui().global::<AppState>().get_page() == 4 {
            self.refresh_engines();
        }
    }

    fn voice_at(&self, index: i32) -> Option<VoiceChoice> {
        self.state.borrow().voices.get(usize::try_from(index).ok()?).cloned()
    }

    fn render_script(&self) {
        self.save_script_now();
        let Some(project) = self.current_project() else { return };
        let ui = self.ui();
        let g = ui.global::<AppState>();
        let script = g.get_script_text().to_string();
        let default_voice = self.voice_at(g.get_default_voice_index()).map(|v| v.name);
        let studio = self.studio().clone();
        let name = project.name.clone();
        self.spawn(
            "合成腳本",
            async move { studio.render_script(&name, &script, default_voice.as_deref(), Origin::Gui).await },
            |a, result| match result {
                Ok(takes) => {
                    let total: f64 = takes.iter().map(|t| t.duration_sec).sum();
                    a.toast(&format!("完成 {} 行，共 {:.1} 秒。已排到時間軸。", takes.len(), total), false);
                    a.load_timeline();
                    a.ui().global::<AppState>().set_page(2);
                }
                Err(e) => a.toast(&e.to_string(), true),
            },
        );
    }

    fn render_line(&self, index: usize) {
        let ui = self.ui();
        let g = ui.global::<AppState>();
        let lines = parse_script(&g.get_script_text());
        let Some(line) = lines.into_iter().find(|l| l.index == index) else { return };
        let default_voice = self.voice_at(g.get_default_voice_index()).map(|v| v.name);
        let req = SpeakRequest {
            text: line.text.clone(),
            voice: line.voice.clone().or(default_voice),
            instruct: line.instruct.clone(),
            project: self.current_project().map(|p| p.name),
            file_stem: Some(format!("{:03}", line.index)),
            ..Default::default()
        };
        self.speak(req, format!("合成第 {index} 行"));
    }

    fn quick_speak(&self) {
        let ui = self.ui();
        let g = ui.global::<AppState>();
        let Some(voice) = self.voice_at(g.get_quick_voice_index()) else {
            return self.toast("請先選一個聲音", true);
        };
        let emotion_index = g.get_quick_emotion_index().max(0) as usize;
        let emotion = (emotion_index > 0).then(|| EmotionSpec {
            text: Some(EMOTIONS[emotion_index.min(8)].to_string()),
            vector: None,
            strength: Some(g.get_quick_emotion_strength()),
        });
        let speed = g.get_quick_speed();
        let instruct = g.get_quick_instruct().trim().to_string();
        let req = SpeakRequest {
            text: g.get_quick_text().to_string(),
            voice: Some(voice.name),
            instruct: (!instruct.is_empty()).then_some(instruct),
            emotion,
            speed: ((speed - 1.0).abs() > 0.01).then_some(speed),
            seed: g.get_quick_seed().trim().parse().ok(),
            project: self.current_project().map(|p| p.name),
            ..Default::default()
        };
        self.speak(req, "合成".into());
    }

    fn speak(&self, req: SpeakRequest, label: String) {
        let studio = self.studio().clone();
        self.spawn(&label, async move { studio.speak(req, Origin::Gui).await }, |a, result| match result {
            Ok(take) => {
                a.toast(&format!("完成：{:.1} 秒，{}", take.duration_sec, take.path), false);
                a.load_timeline();
                a.play_file(Path::new(&take.path));
            }
            Err(e) => a.toast(&e.to_string(), true),
        });
    }

    fn update_quick_hint(&self, index: i32) {
        let hint = self
            .voice_at(index)
            .and_then(|v| self.studio().catalog().get(&v.engine_id).ok().map(|e| e.manifest.clone()))
            .map(|m| {
                let c = &m.capabilities;
                let mut yes = Vec::new();
                let mut no = Vec::new();
                for (ok, name) in [(c.instruct, "語氣指示"), (c.emotion, "情緒"), (c.speed, "語速"), (c.zhuyin, "注音標記")] {
                    if ok { yes.push(name) } else { no.push(name) }
                }
                let mut s = format!("{}：", m.name);
                if !yes.is_empty() {
                    s.push_str(&format!("支援{}", yes.join("、")));
                }
                if !no.is_empty() {
                    s.push_str(&format!("{}不支援{}", if yes.is_empty() { "" } else { "；" }, no.join("、")));
                }
                s
            })
            .unwrap_or_default();
        self.ui().global::<AppState>().set_quick_hint(hint.into());
    }

    // ---- 聲音庫 ----

    fn refresh_voices(&self) {
        let voices = match self.studio().voice_choices() {
            Ok(v) => v,
            Err(e) => return self.toast(&e.to_string(), true),
        };
        // 清單重新排序後，用名稱把原本選的聲音找回來。
        let (quick_prev, default_prev) = {
            let ui = self.ui();
            let g = ui.global::<AppState>();
            (self.voice_at(g.get_quick_voice_index()).map(|v| v.name), self.voice_at(g.get_default_voice_index()).map(|v| v.name))
        };
        let index_of = |name: &Option<String>| -> i32 {
            name.as_ref().and_then(|n| voices.iter().position(|v| &v.name == n)).map_or(0, |i| i as i32)
        };
        let (quick_idx, default_idx) = (index_of(&quick_prev), index_of(&default_prev));
        let items: Vec<VoiceItem> = voices
            .iter()
            .map(|v| VoiceItem {
                name: v.name.clone().into(),
                engine: v.engine_id.clone().into(),
                source: if v.source == VoiceSource::Library { "library" } else { "builtin" }.into(),
                language: v.language.clone().into(),
                description: v.description.clone().into(),
            })
            .collect();
        let names: Vec<String> = voices
            .iter()
            .map(|v| match v.source {
                VoiceSource::Library => format!("{}（{}）", v.name, v.engine_id),
                VoiceSource::Builtin => format!("{}　{}", v.name, v.description),
            })
            .collect();
        let clone_engines: Vec<_> = self
            .studio()
            .catalog()
            .entries()
            .iter()
            .filter(|e| e.manifest.capabilities.clone)
            .map(|e| (e.manifest.id.clone(), e.manifest.name.clone()))
            .collect();
        let ui = self.ui();
        let g = ui.global::<AppState>();
        g.set_voices(model(items));
        g.set_voice_names(strings(names));
        g.set_quick_voice_index(quick_idx);
        g.set_default_voice_index(default_idx);
        g.set_clone_engines(strings(clone_engines.iter().map(|(_, n)| n.clone())));
        g.set_clone_engine_ids(strings(clone_engines.iter().map(|(i, _)| i.clone())));
        {
            let mut st = self.state.borrow_mut();
            st.voices = voices;
            st.clone_engine_ids = clone_engines.iter().map(|(i, _)| i.clone()).collect();
        }
        self.update_quick_hint(g.get_quick_voice_index());
        self.voice_engine_changed(g.get_new_voice_engine_index());
    }

    fn voice_engine_changed(&self, index: i32) {
        let id = self.state.borrow().clone_engine_ids.get(index.max(0) as usize).cloned();
        let supported = id
            .and_then(|id| self.studio().catalog().get(&id).ok().map(|e| e.manifest.capabilities.trained_models))
            .unwrap_or(false);
        self.ui().global::<AppState>().set_new_voice_trained_supported(supported);
        if !supported {
            self.clear_weights();
        }
    }

    fn pick_reference(&self) {
        if let Some(path) = rfd::FileDialog::new().set_title("選擇參考音檔").add_filter("WAV 音檔", &["wav"]).pick_file() {
            self.ui().global::<AppState>().set_new_voice_file(path.display().to_string().into());
            self.state.borrow_mut().ref_file = Some(path);
        }
    }

    fn pick_weights(&self, gpt: bool) {
        let (title, label, ext) = if gpt { ("選擇 GPT 權重", "GPT 權重", "ckpt") } else { ("選擇 SoVITS 權重", "SoVITS 權重", "pth") };
        let Some(path) = rfd::FileDialog::new().set_title(title).add_filter(label, &[ext]).pick_file() else { return };
        let ui = self.ui();
        let g = ui.global::<AppState>();
        let shown: SharedString = path.display().to_string().into();
        let mut st = self.state.borrow_mut();
        if gpt {
            g.set_new_voice_gpt_file(shown);
            st.gpt_file = Some(path);
        } else {
            g.set_new_voice_sovits_file(shown);
            st.sovits_file = Some(path);
        }
    }

    fn clear_weights(&self) {
        let ui = self.ui();
        let g = ui.global::<AppState>();
        g.set_new_voice_gpt_file("".into());
        g.set_new_voice_sovits_file("".into());
        let mut st = self.state.borrow_mut();
        st.gpt_file = None;
        st.sovits_file = None;
    }

    fn create_voice(&self) {
        let ui = self.ui();
        let g = ui.global::<AppState>();
        let (audio_file, gpt, sovits, engine_id) = {
            let st = self.state.borrow();
            let engine = st.clone_engine_ids.get(g.get_new_voice_engine_index().max(0) as usize).cloned();
            (st.ref_file.clone(), st.gpt_file.clone(), st.sovits_file.clone(), engine)
        };
        let (Some(audio_file), Some(engine_id)) = (audio_file, engine_id) else {
            return self.toast("請選擇引擎與參考音檔", true);
        };
        let basis = match g.get_new_voice_consent() {
            0 => ConsentBasis::OwnVoice,
            1 => ConsentBasis::WrittenPermission,
            2 => ConsentBasis::LicensedMaterial,
            _ => return self.toast("請選擇同意依據", true),
        };
        let language = LANGUAGES
            .get(g.get_new_voice_language_index().max(0) as usize)
            .map(|(code, _)| code.to_string())
            .filter(|c| !c.is_empty());
        let name = g.get_new_voice_name().trim().to_string();
        let input = NewVoice {
            name: name.clone(),
            engine_id,
            audio: audio_file,
            transcript: g.get_new_voice_transcript().to_string(),
            language,
            gpt_weights: gpt,
            sovits_weights: sovits,
            consent: Consent { basis, statement: g.get_new_voice_statement().to_string(), recorded_at: now() },
        };
        match self.studio().create_voice(input) {
            Ok(_) => {
                g.set_new_voice_name("".into());
                g.set_new_voice_file("".into());
                g.set_new_voice_transcript("".into());
                g.set_new_voice_statement("".into());
                g.set_new_voice_consent(-1);
                self.state.borrow_mut().ref_file = None;
                self.clear_weights();
                self.refresh_voices();
                g.set_selected_voice(name.clone().into());
                self.toast(&format!("已建立聲音「{name}」"), false);
            }
            Err(e) => self.toast(&e.to_string(), true),
        }
    }

    fn delete_voice(&self, name: &str) {
        let confirmed = rfd::MessageDialog::new()
            .set_title("刪除聲音")
            .set_description(format!("刪除聲音「{name}」？用它合成過的音檔不受影響，參考音檔仍保留在 references 資料夾。"))
            .set_buttons(rfd::MessageButtons::OkCancelCustom("刪除".into(), "取消".into()))
            .set_level(rfd::MessageLevel::Warning)
            .show();
        let ok = match &confirmed {
            rfd::MessageDialogResult::Ok => true,
            rfd::MessageDialogResult::Custom(label) => label == "刪除",
            _ => false,
        };
        if !ok {
            return;
        }
        let result = self.studio().with_store(|s| match s.find_voice(name)? {
            Some(v) => s.delete_voice(v.id),
            None => Ok(()),
        });
        match result {
            Ok(()) => {
                self.ui().global::<AppState>().set_selected_voice("".into());
                self.refresh_voices();
            }
            Err(e) => self.toast(&e.to_string(), true),
        }
    }

    fn preview_voice(&self, name: &str) {
        let voice = self.state.borrow().voices.iter().find(|v| v.name == name).cloned();
        let text = match voice.as_ref().map(|v| v.language.as_str()) {
            Some("ja") => "こんにちは。これは音声のテストです。",
            Some("en") => "Hello, this is a voice preview.",
            Some("ko") => "안녕하세요. 음성 미리 듣기입니다.",
            _ => "你好，這是聲音試聽。今天天氣很好。",
        };
        let req = SpeakRequest {
            text: text.into(),
            voice: Some(name.to_string()),
            project: Some("試聽".into()),
            file_stem: Some(format!("試聽-{name}")),
            ..Default::default()
        };
        self.speak(req, format!("試聽 {name}"));
    }

    // ---- 時間軸與播放 ----

    fn load_clip(&self, path: &str) -> Option<(Arc<Clip>, Image)> {
        if let Some(hit) = self.state.borrow().cache.get(path) {
            return Some(hit.clone());
        }
        let clip = Arc::new(audio::read_wav(Path::new(path)).ok()?);
        let img = waveform::render(&clip);
        self.state.borrow_mut().cache.insert(path.to_string(), (clip.clone(), img.clone()));
        Some((clip, img))
    }

    fn load_timeline(&self) {
        let Some(project) = self.current_project() else { return };
        let takes = match self.studio().with_store(|s| s.timeline(project.id)) {
            Ok(t) => t,
            Err(e) => return self.toast(&e.to_string(), true),
        };
        let ui = self.ui();
        let g = ui.global::<AppState>();
        let gap = g.get_export_gap() as f64;
        let palette: Vec<slint::Color> = Theme::get(&ui).get_clip_colors().iter().collect();
        let mut track_names: Vec<String> = Vec::new();
        let mut clips = Vec::new();
        let mut timeline = Vec::new();
        let mut cursor = 0.0;
        for take in takes {
            let track = match track_names.iter().position(|n| *n == take.voice_name) {
                Some(i) => i,
                None => {
                    track_names.push(take.voice_name.clone());
                    track_names.len() - 1
                }
            };
            let wave = self.load_clip(&take.path).map(|(_, img)| img).unwrap_or_default();
            let color = palette.get(track % palette.len().max(1)).copied().unwrap_or_default();
            clips.push(ClipItem {
                id: take.id.to_string().into(),
                track: track as i32,
                start: cursor as f32,
                duration: take.duration_sec as f32,
                text: take.text.clone().into(),
                voice: take.voice_name.clone().into(),
                engine: take.engine_id.clone().into(),
                lufs: take.loudness_lufs.map(|l| format!("{l:.1} LUFS")).unwrap_or_else(|| "—".into()).into(),
                path: take.path.clone().into(),
                starred: take.starred,
                wave,
                color,
            });
            timeline.push(TimelineClip { take: take.clone() });
            cursor += take.duration_sec + gap;
        }
        let total = (cursor - gap).max(0.0);
        let tracks: Vec<TrackItem> = track_names
            .iter()
            .enumerate()
            .map(|(i, n)| TrackItem { name: n.clone().into(), color: palette.get(i % palette.len().max(1)).copied().unwrap_or_default() })
            .collect();
        g.set_tracks(model(tracks));
        g.set_clips(model(clips));
        g.set_total_seconds(total as f32);
        g.set_duration_code(timecode(total).into());
        if g.get_playhead() as f64 > total {
            g.set_playhead(0.0);
            g.set_timecode(timecode(0.0).into());
        }
        let mut st = self.state.borrow_mut();
        st.timeline = timeline;
        st.program = None;
    }

    /// 把時間軸接成一段音訊（播放用），間隔與匯出相同。
    fn program(&self) -> Option<(Arc<Clip>, Vec<f32>)> {
        if let Some(p) = self.state.borrow().program.clone() {
            return Some(p);
        }
        let paths: Vec<String> = self.state.borrow().timeline.iter().map(|t| t.take.path.clone()).collect();
        if paths.is_empty() {
            return None;
        }
        let clips: Vec<Arc<Clip>> = paths.iter().filter_map(|p| self.load_clip(p).map(|(c, _)| c)).collect();
        let rate = clips.iter().map(|c| c.sample_rate).max()?;
        let clips: Vec<Clip> = clips.iter().map(|c| audio::resample(c, rate)).collect();
        let gap = self.ui().global::<AppState>().get_export_gap() as f64;
        let joined = Arc::new(audio::concat(&clips, gap).ok()?);
        let env = waveform::envelope(&joined);
        self.state.borrow_mut().program = Some((joined.clone(), env.clone()));
        Some((joined, env))
    }

    fn play_pause(&self) {
        let ui = self.ui();
        let g = ui.global::<AppState>();
        let mode = self.state.borrow().mode;
        {
            let mut st = self.state.borrow_mut();
            if mode == PlayMode::Program && st.audio.is_playing() {
                st.audio.pause();
                g.set_playing(false);
                return;
            }
            if mode == PlayMode::Program && st.audio.is_paused() {
                st.audio.resume();
                g.set_playing(true);
                return;
            }
        }
        let Some((program, _)) = self.program() else { return };
        let mut from = g.get_playhead() as f64;
        if from >= program.duration_sec() - 0.05 {
            from = 0.0;
        }
        let result = self.state.borrow_mut().audio.play(program, from);
        match result {
            Ok(()) => {
                self.state.borrow_mut().mode = PlayMode::Program;
                g.set_playing(true);
            }
            Err(e) => self.toast(&e, true),
        }
    }

    fn stop_playback(&self, rewind: bool) {
        let mut st = self.state.borrow_mut();
        st.audio.stop();
        st.mode = PlayMode::Idle;
        drop(st);
        let ui = self.ui();
        let g = ui.global::<AppState>();
        g.set_playing(false);
        g.set_meter_level(0.0);
        if rewind {
            g.set_playhead(0.0);
            g.set_timecode(timecode(0.0).into());
        }
    }

    fn seek(&self, sec: f64) {
        let ui = self.ui();
        let g = ui.global::<AppState>();
        let total = g.get_total_seconds() as f64;
        let sec = sec.clamp(0.0, total);
        g.set_playhead(sec as f32);
        g.set_timecode(timecode(sec).into());
        let playing = self.state.borrow().mode == PlayMode::Program && self.state.borrow().audio.is_playing();
        if playing
            && let Some((program, _)) = self.program() {
                let r = self.state.borrow_mut().audio.play(program, sec);
                self.report(r);
            }
    }

    /// 播放單一檔案（試聽、agent 要求播放）。不影響時間軸的播放頭。
    fn play_file(&self, path: &Path) {
        let Some((clip, _)) = self.load_clip(&path.display().to_string()) else {
            return self.toast(&format!("讀不到 {}", path.display()), true);
        };
        let result = self.state.borrow_mut().audio.play(clip, 0.0);
        match result {
            Ok(()) => {
                self.state.borrow_mut().mode = PlayMode::Preview;
                self.ui().global::<AppState>().set_playing(false);
            }
            Err(e) => self.toast(&e, true),
        }
    }

    fn tick(&self) {
        let mode = self.state.borrow().mode;
        if mode == PlayMode::Idle {
            return;
        }
        let ui = self.ui();
        let g = ui.global::<AppState>();
        let (pos, finished, level) = {
            let st = self.state.borrow();
            let pos = st.audio.position();
            let env = st.program.as_ref().map(|(_, e)| e);
            let level = match (mode, env) {
                (PlayMode::Program, Some(e)) if st.audio.is_playing() => e.get((pos * 50.0) as usize).copied().unwrap_or(0.0),
                _ => 0.0,
            };
            (pos, st.audio.finished(), level)
        };
        if mode == PlayMode::Program {
            g.set_playhead(pos as f32);
            g.set_timecode(timecode(pos).into());
            g.set_meter_level(level);
            let mut st = self.state.borrow_mut();
            if level >= st.peak.0 || st.peak.1.elapsed() > Duration::from_millis(1200) {
                st.peak = (level, Instant::now());
            }
            g.set_meter_peak(st.peak.0);
        }
        if finished {
            let mut st = self.state.borrow_mut();
            st.audio.stop();
            st.mode = PlayMode::Idle;
            drop(st);
            g.set_playing(false);
            g.set_meter_level(0.0);
            g.set_meter_peak(0.0);
        }
    }

    fn selected_take(&self) -> Option<Take> {
        let id = self.ui().global::<AppState>().get_selected_clip().to_string();
        self.state.borrow().timeline.iter().find(|t| t.take.id.to_string() == id).map(|t| t.take.clone())
    }

    fn move_clip(&self, delta: i32) {
        let Some(take) = self.selected_take() else { return };
        match self.studio().with_store_mut(|s| s.move_take(take.id, delta)) {
            Ok(()) => {
                self.stop_playback(false);
                self.load_timeline();
            }
            Err(e) => self.toast(&e.to_string(), true),
        }
    }

    fn remove_clip(&self) {
        let Some(take) = self.selected_take() else { return };
        match self.studio().with_store(|s| s.forget_take(take.id)) {
            Ok(()) => {
                self.stop_playback(false);
                self.ui().global::<AppState>().set_selected_clip("".into());
                self.load_timeline();
                self.toast("已從時間軸移除，音檔仍保留在資料夾裡。", false);
            }
            Err(e) => self.toast(&e.to_string(), true),
        }
    }

    fn star_clip(&self, on: bool) {
        let Some(take) = self.selected_take() else { return };
        if let Err(e) = self.studio().with_store(|s| s.set_starred(take.id, on)) {
            return self.toast(&e.to_string(), true);
        }
        self.load_timeline();
    }

    fn regenerate_clip(&self) {
        let Some(old) = self.selected_take() else { return };
        let project = self.current_project().map(|p| p.name);
        let req = SpeakRequest { text: old.text.clone(), voice: Some(old.voice_name.clone()), project, ..Default::default() };
        let studio = self.studio().clone();
        self.spawn("重新合成", async move { studio.speak(req, Origin::Gui).await }, move |a, result| match result {
            Ok(new) => {
                if let Err(e) = a.studio().with_store_mut(|s| s.replace_in_timeline(old.id, new.id)) {
                    a.toast(&e.to_string(), true);
                }
                a.stop_playback(false);
                a.load_timeline();
                a.ui().global::<AppState>().set_selected_clip(new.id.to_string().into());
                a.play_file(Path::new(&new.path));
            }
            Err(e) => a.toast(&e.to_string(), true),
        });
    }

    fn open_clip_folder(&self) {
        if let Some(t) = self.selected_take() {
            self.report(platform::reveal(Path::new(&t.path)));
        }
    }

    // ---- 輸出 ----

    fn project_output_dir(&self) -> PathBuf {
        let base = self.studio().output_dir();
        match self.current_project() {
            Some(p) => base.join(cantiva_core::paths::sanitize_file_stem(&p.name)),
            None => base,
        }
    }

    fn refresh_output_dir(&self) {
        self.ui().global::<AppState>().set_output_dir(self.project_output_dir().display().to_string().into());
    }

    fn open_output_dir(&self) {
        let dir = self.project_output_dir();
        let _ = std::fs::create_dir_all(&dir);
        self.report(platform::open_path(&dir));
    }

    fn export_project(&self) {
        let Some(project) = self.current_project() else { return };
        let ui = self.ui();
        let g = ui.global::<AppState>();
        let gap = g.get_export_gap() as f64;
        let lufs = g.get_export_normalize().then(|| g.get_export_lufs() as f64);
        let studio = self.studio().clone();
        let name = project.name.clone();
        self.spawn(
            "匯出中",
            async move { tokio::task::spawn_blocking(move || studio.export_project(&name, gap, lufs)).await },
            |a, result| match result {
                Ok(Ok((path, secs))) => {
                    let file = path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
                    let item = ExportItem {
                        name: file.into(),
                        path: path.display().to_string().into(),
                        detail: format!(
                            "{} 分 {:02} 秒，{}",
                            secs as u64 / 60,
                            secs as u64 % 60,
                            time::OffsetDateTime::now_local().unwrap_or(time::OffsetDateTime::now_utc()).format(
                                time::macros::format_description!("[hour]:[minute]:[second] 匯出")
                            ).unwrap_or_default()
                        )
                        .into(),
                    };
                    let exports = {
                        let mut st = a.state.borrow_mut();
                        st.exports.insert(0, item);
                        st.exports.clone()
                    };
                    a.ui().global::<AppState>().set_exports(model(exports));
                    a.toast(&format!("已匯出 {}", path.display()), false);
                }
                Ok(Err(e)) => a.toast(&e.to_string(), true),
                Err(e) => a.toast(&e.to_string(), true),
            },
        );
    }

    // ---- 引擎 ----

    fn refresh_engines(&self) {
        let gpu = self.state.borrow().gpu.clone();
        let statuses = self.studio().engines();
        let items: Vec<EngineItem> = statuses
            .iter()
            .map(|e| {
                let m = &e.manifest;
                let c = &m.capabilities;
                let mut features = Vec::new();
                if !m.builtin_voices.is_empty() {
                    features.push(format!("{} 個內建聲音", m.builtin_voices.len()));
                }
                for (ok, name) in [
                    (c.clone, "複製聲音"),
                    (c.trained_models, "匯入角色模型"),
                    (c.instruct, "語氣指示"),
                    (c.emotion, "情緒控制"),
                    (c.speed, "語速"),
                    (c.zhuyin, "注音標記"),
                    (c.design, "設計聲音"),
                ] {
                    if ok {
                        features.push(name.to_string());
                    }
                }
                let mut vram = format!("建議 {:.0} GB 以上", m.min_vram_gb);
                if let Some((_, mib)) = &gpu
                    && (*mib as f32) < m.min_vram_gb * 1024.0 {
                        vram.push_str(&format!("（你的顯示卡 {:.0} GB，可能不夠）", *mib as f32 / 1024.0));
                    }
                let (state, label) = state_label(e.state);
                EngineItem {
                    id: m.id.clone().into(),
                    name: m.name.clone().into(),
                    vendor: m.vendor.clone().into(),
                    summary: m.summary.clone().into(),
                    license: m.license.clone().into(),
                    vram: vram.into(),
                    state: state.into(),
                    state_label: label.into(),
                    error: e.last_error.clone().unwrap_or_default().into(),
                    features: features.join("、").into(),
                    languages: m.languages.join("、").into(),
                    voice_count: m.builtin_voices.len() as i32,
                }
            })
            .collect();
        let active = statuses.iter().find(|e| matches!(e.state, EngineState::Ready | EngineState::Busy | EngineState::Loading));
        let ui = self.ui();
        let g = ui.global::<AppState>();
        match active {
            Some(e) => {
                let (state, label) = state_label(e.state);
                g.set_engine_state(state.into());
                g.set_engine_summary(format!("{} {}", e.manifest.name, label).into());
            }
            None => {
                g.set_engine_state("idle".into());
                g.set_engine_summary("沒有載入模型".into());
            }
        }
        g.set_engines(model(items));
        g.set_gpu_note(
            match &gpu {
                Some((name, mib)) => format!("偵測到 {name}，顯示記憶體 {:.1} GB。同一時間只會載入一個模型，換引擎時會先卸載前一個。", *mib as f32 / 1024.0),
                None => "沒有偵測到 NVIDIA 顯示卡（找不到 nvidia-smi）。模型會改用 CPU，速度會慢很多。".into(),
            }
            .into(),
        );
    }

    fn install_engine(&self, id: &str) {
        let entry = match self.studio().catalog().get(id) {
            Ok(e) => e.clone(),
            Err(e) => return self.toast(&e.to_string(), true),
        };
        match platform::run_install_script(&entry.install_script(), &entry.manifest.name) {
            Ok(()) => self.toast(
                &format!("已開啟 {} 的安裝視窗。下載完成並顯示「完成」後，回到這裡按「重新整理」。", entry.manifest.name),
                false,
            ),
            Err(e) => self.toast(&e, true),
        }
    }

    fn load_engine(&self, id: &str) {
        let studio = self.studio().clone();
        let id = id.to_string();
        self.spawn(&format!("載入 {id}"), async move { studio.load(&id).await }, |a, r| {
            a.refresh_engines();
            if let Err(e) = r {
                a.toast(&e.to_string(), true);
            }
        });
    }

    fn unload_engine(&self) {
        let studio = self.studio().clone();
        self.spawn("卸載模型", async move { studio.unload().await }, |a, ()| {
            a.refresh_engines();
            a.toast("已卸載模型，顯示記憶體已釋放。", false);
        });
    }

    fn open_engine_log(&self, id: &str) {
        let log = self.rt.paths.logs().join(format!("{id}.log"));
        let target = if log.is_file() { log } else { self.rt.paths.logs() };
        self.report(platform::open_path(&target));
    }

    // ---- 連線 ----

    fn refresh_connect(&self, port: u16) {
        let mcp = platform::sibling_exe("cantiva-mcp");
        let ui = self.ui();
        let g = ui.global::<AppState>();
        g.set_api_status(format!("控制 API 127.0.0.1:{port}").into());
        match &mcp {
            Some(path) => {
                g.set_mcp_path(path.display().to_string().into());
                let configs: Vec<ClientConfig> = clients::snippets(path)
                    .into_iter()
                    .map(|c| ClientConfig {
                        client: c.client.into(),
                        location: c.location.into(),
                        snippet: c.snippet.into(),
                        note: c.note.into(),
                    })
                    .collect();
                g.set_client_configs(model(configs));
            }
            None => g.set_mcp_path("找不到 cantiva-mcp（應該和 cantiva 放在同一個資料夾）".into()),
        }
        let settings = self.studio().settings();
        g.set_tunnel_available(
            mcp.is_some() && platform::find_cloudflared(settings.tunnel.cloudflared_path.as_deref()).is_some(),
        );
        g.set_tunnel_allow_clone(settings.tunnel.allow_clone_over_tunnel);
    }

    fn toggle_tunnel(&self, on: bool) {
        let ui = self.ui();
        let g = ui.global::<AppState>();
        // 每次切換都換新的世代編號，舊通道遲到的事件會被忽略。
        let generation = self.tunnel_gen.get() + 1;
        self.tunnel_gen.set(generation);
        if let Some(t) = self.state.borrow_mut().tunnel.take() {
            t.stop();
        }
        g.set_tunnel_url("".into());
        if !on {
            g.set_tunnel_running(false);
            g.set_tunnel_status("已關閉".into());
            return;
        }
        let settings = self.studio().settings();
        let (Some(mcp), Some(cf)) = (
            platform::sibling_exe("cantiva-mcp"),
            platform::find_cloudflared(settings.tunnel.cloudflared_path.as_deref()),
        ) else {
            g.set_tunnel_running(false);
            g.set_tunnel_status("已關閉".into());
            return self.toast("找不到 cantiva-mcp 或 cloudflared", true);
        };
        let result = Tunnel::start(&mcp, &cf, move |ev| {
            let _ = slint::invoke_from_event_loop(move || {
                with_app(|a| {
                    if a.tunnel_gen.get() != generation {
                        return;
                    }
                    let ui = a.ui();
                    let g = ui.global::<AppState>();
                    match ev {
                        TunnelEvent::Url(url) => {
                            g.set_tunnel_url(url.into());
                            g.set_tunnel_status("已開啟，雲端 agent 可以連線".into());
                        }
                        TunnelEvent::Stopped(msg) => {
                            a.tunnel_gen.set(generation + 1);
                            if let Some(t) = a.state.borrow_mut().tunnel.take() {
                                t.stop();
                            }
                            g.set_tunnel_running(false);
                            g.set_tunnel_url("".into());
                            g.set_tunnel_status("已關閉".into());
                            a.toast(&format!("通道已關閉：{msg}"), true);
                        }
                    }
                })
            });
        });
        match result {
            Ok(t) => {
                self.state.borrow_mut().tunnel = Some(t);
                g.set_tunnel_running(true);
                g.set_tunnel_status("連線中…".into());
            }
            Err(e) => {
                g.set_tunnel_running(false);
                g.set_tunnel_status("已關閉".into());
                self.toast(&e, true);
            }
        }
    }

    fn tunnel_clone_changed(&self, on: bool) {
        let r = self.studio().update_settings_with(|s| s.tunnel.allow_clone_over_tunnel = on);
        self.report(r.map_err(|e| e.to_string()));
    }

    fn copy_text(&self, text: &str) {
        match arboard::Clipboard::new().and_then(|mut c| c.set_text(text.to_string())) {
            Ok(()) => self.toast("已複製", false),
            Err(e) => self.toast(&format!("複製失敗：{e}"), true),
        }
    }

    /// 把配套的 agent skill 複製到 Claude Code 與通用的 ~/.agents/skills 資料夾。
    fn install_skill(&self) {
        let Some(src) = self.studio().catalog().engines_dir().parent().map(|p| p.join("skill").join("cantiva-voice")) else {
            return;
        };
        if !src.join("SKILL.md").is_file() {
            return self.toast(&format!("找不到 {}", src.display()), true);
        }
        let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from) else {
            return self.toast("找不到使用者資料夾", true);
        };
        let targets = [home.join(".claude").join("skills").join("cantiva-voice"), home.join(".agents").join("skills").join("cantiva-voice")];
        let mut done = Vec::new();
        for t in &targets {
            match copy_dir(&src, t) {
                Ok(()) => done.push(t.display().to_string()),
                Err(e) => return self.toast(&format!("複製到 {} 失敗：{e}", t.display()), true),
            }
        }
        self.toast(&format!("已安裝 skill 到：{}", done.join("、")), false);
    }
}

fn copy_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &to)?;
        } else {
            std::fs::copy(entry.path(), to)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_timecode() {
        assert_eq!(timecode(0.0), "00:00:00.00");
        assert_eq!(timecode(83.4), "00:01:23.40");
        assert_eq!(timecode(3723.456), "01:02:03.46");
    }
}
