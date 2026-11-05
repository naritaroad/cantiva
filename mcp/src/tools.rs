//! MCP 工具定義。每個工具都轉呼叫 Cantiva 的本機控制 API。

use std::path::PathBuf;
use std::time::Duration;

use cantiva_api::ApiClient;
use cantiva_api::types::{CreateVoice, ScriptBody, SpeakBody};
use cantiva_core::model::{ConsentBasis, EmotionSpec, SpeakRequest, Take};
use serde::Serialize;
use cantiva_core::paths::AppPaths;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{ServerHandler, schemars, tool, tool_handler, tool_router};
use serde::Deserialize;
use serde_json::json;

const INSTRUCTIONS: &str = "Cantiva 是這台電腦上的本機語音工作室（文字轉語音）。\
建議流程：先 list_voices 看可用聲音 → speak 合成一段話，或 render_script 依腳本逐行合成。\
音檔存在使用者電腦上，工具回傳檔案路徑。第一次合成會載入模型，可能要一到數分鐘。\
同一時間只會載入一個模型；換引擎會自動卸載前一個。\
複製聲音只能用在使用者本人或已取得同意的聲音。";

#[derive(Clone)]
pub struct Cantiva {
    paths: AppPaths,
    tunnel: bool,
    autostart: bool,
    tool_router: ToolRouter<Self>,
}

impl Cantiva {
    pub fn new(paths: AppPaths, tunnel: bool, autostart: bool) -> Self {
        let mut tool_router = Self::tool_router();
        if tunnel {
            // 通道對外，這些只能在本機做的事情直接不提供。
            tool_router.remove_route("create_voice");
            tool_router.remove_route("play_take");
            tool_router.remove_route("load_engine");
            tool_router.remove_route("unload_engine");
        }
        Self { paths, tunnel, autostart, tool_router }
    }

    /// 取得連到 Cantiva 的客戶端；沒在跑而且允許時，自動以無視窗模式啟動。
    async fn client(&self) -> Result<ApiClient, String> {
        if let Some(c) = cantiva_api::ensure_single(&self.paths).await {
            return Ok(if self.tunnel { c.as_tunnel() } else { c });
        }
        if !self.autostart {
            return Err("Cantiva 沒有在執行。請先打開 Cantiva".into());
        }
        let exe = sibling_exe("cantiva").ok_or("找不到 cantiva 執行檔（應該和 cantiva-mcp 放在同一個資料夾）")?;
        std::process::Command::new(&exe)
            .arg("--headless")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("無法啟動 {}：{e}", exe.display()))?;
        for _ in 0..60 {
            tokio::time::sleep(Duration::from_millis(500)).await;
            if let Some(c) = cantiva_api::ensure_single(&self.paths).await {
                return Ok(if self.tunnel { c.as_tunnel() } else { c });
            }
        }
        Err("Cantiva 啟動逾時，請手動打開 Cantiva 再試一次".into())
    }
}

fn sibling_exe(name: &str) -> Option<PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let file = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
    let p = dir.join(file);
    p.is_file().then_some(p)
}

fn pretty(v: serde_json::Value) -> String {
    serde_json::to_string_pretty(&v).unwrap_or_default()
}

/// 通道模式只回傳檔名，不透露本機路徑（裡面有 Windows 使用者名稱）。
fn shown_path(path: &str, tunnel: bool) -> String {
    if tunnel {
        std::path::Path::new(path).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default()
    } else {
        path.to_string()
    }
}

fn take_json(t: &Take, tunnel: bool) -> serde_json::Value {
    json!({
        "take_id": t.id,
        "path": shown_path(&t.path, tunnel),
        "duration_sec": (t.duration_sec * 100.0).round() / 100.0,
        "voice": t.voice_name,
        "engine": t.engine_id,
        "segments": t.segments,
        "loudness_lufs": t.loudness_lufs.map(|l| (l * 10.0).round() / 10.0),
        "elapsed_sec": (t.elapsed_sec * 10.0).round() / 10.0,
    })
}

// ---- 參數 ----

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SpeakParams {
    /// 要唸的文字。長文會自動依標點分段再合併成一個檔案。
    pub text: String,
    /// 聲音名稱，用 list_voices 查。內建聲音寫成「引擎/說話人」，例如 "qwen3-tts/Vivian"；
    /// 聲音庫的聲音直接寫名稱。省略時用目前引擎的預設聲音。
    #[serde(default)]
    pub voice: Option<String>,
    /// 語氣指示，例如「用溫柔、放慢的語氣」。只有 Qwen3-TTS（內建聲音）與 CosyVoice3 支援。
    #[serde(default)]
    pub instruct: Option<String>,
    /// 情緒描述，例如「難過」「開心」「生氣」。只有 IndexTTS 支援。
    #[serde(default)]
    pub emotion: Option<String>,
    /// 精確情緒向量，8 個 0–1 的數字：喜、怒、哀、懼、厭惡、低落、驚訝、平靜。只有 IndexTTS 支援。
    #[serde(default)]
    pub emotion_vector: Option<[f32; 8]>,
    /// 情緒強度 0–1，預設 0.6。
    #[serde(default)]
    pub emotion_strength: Option<f32>,
    /// 語速倍率，0.5–2.0，1.0 為正常。CosyVoice3 與 IndexTTS 支援。
    #[serde(default)]
    pub speed: Option<f32>,
    /// 要唸的文字的語言：zh、en、ja、ko、yue。多數情況可省略，模型會自己判斷（GPT-SoVITS 看到假名會當日文）。
    #[serde(default)]
    pub language: Option<String>,
    /// 固定隨機種子，讓同樣的輸入產生同樣的結果。
    #[serde(default)]
    pub seed: Option<u64>,
    /// 輸出檔名（不含副檔名）。省略時用文字開頭幾個字。
    #[serde(default)]
    pub file_name: Option<String>,
    /// 歸到哪個專案（資料夾）。省略時放在「快速合成」。
    #[serde(default)]
    pub project: Option<String>,
    /// 合成完成後在 Cantiva 直接播放（需要 Cantiva 主視窗開著）。
    #[serde(default)]
    pub play: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ScriptParams {
    /// 專案名稱；音檔會放在同名資料夾，檔名依行號排序。
    pub project: String,
    /// 腳本。每個段落（空行分隔）是一行台詞；開頭寫「@聲音名：」指定聲音，
    /// 接著可寫「{語氣指示}」；「#」開頭是註解。例如：
    /// "@旁白：{低沉}夜深了。\n\n@qwen3-tts/Serena：你還在嗎？"
    pub script: String,
    /// 沒有寫「@聲音名：」的行用這個聲音。
    #[serde(default)]
    pub default_voice: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct VoicesParams {
    /// 只列出這個引擎的聲音，例如 "qwen3-tts"、"indextts2"。
    #[serde(default)]
    pub engine: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct EngineParams {
    /// 引擎 id：qwen3-tts、cosyvoice3、indextts2、breezyvoice、gpt-sovits。
    pub engine: String,
}

/// 複製聲音的同意依據（對應 cantiva_core::model::ConsentBasis）。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Consent {
    /// 使用者本人的聲音
    OwnVoice,
    /// 聲音主人已書面或錄音同意
    WrittenPermission,
    /// 有授權可複製的素材
    LicensedMaterial,
}

impl From<Consent> for ConsentBasis {
    fn from(c: Consent) -> Self {
        match c {
            Consent::OwnVoice => ConsentBasis::OwnVoice,
            Consent::WrittenPermission => ConsentBasis::WrittenPermission,
            Consent::LicensedMaterial => ConsentBasis::LicensedMaterial,
        }
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreateVoiceParams {
    /// 新聲音的名稱，之後在 speak 的 voice 參數使用。
    pub name: String,
    /// 用哪個引擎複製：qwen3-tts、cosyvoice3、indextts2、breezyvoice、gpt-sovits。
    pub engine: String,
    /// 參考音檔的絕對路徑（WAV，3–15 秒的乾淨人聲；GPT-SoVITS 限 3–10 秒）。
    pub audio_path: String,
    /// 參考音檔裡實際說的內容，必須逐字一致。
    pub transcript: String,
    /// 同意依據：own_voice（使用者本人）、written_permission（聲音主人書面同意）、licensed_material（授權素材）。
    pub consent_basis: Consent,
    /// 同意說明，例如「這是我本人的聲音」。必須先向使用者確認，不能自行假設。
    pub consent_statement: String,
    /// 參考音檔的語言：zh、ja、en、ko、yue。GPT-SoVITS 建議填寫。
    #[serde(default)]
    pub reference_language: Option<String>,
    /// GPT-SoVITS 角色模型的 GPT 權重（.ckpt）絕對路徑；要和 sovits_weights_path 成對提供。
    #[serde(default)]
    pub gpt_weights_path: Option<String>,
    /// GPT-SoVITS 角色模型的 SoVITS 權重（.pth）絕對路徑。
    #[serde(default)]
    pub sovits_weights_path: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TakesParams {
    /// 最多列幾筆，預設 10。
    #[serde(default)]
    pub limit: Option<usize>,
    /// 只列出某個專案的錄音。
    #[serde(default)]
    pub project: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PlayParams {
    /// 要播放的 take_id（speak 或 list_takes 回傳的）。
    pub take_id: String,
}

#[tool_router]
impl Cantiva {
    #[tool(
        description = "查看 Cantiva 狀態：版本、目前載入的模型、輸出資料夾。",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn status(&self) -> Result<String, String> {
        let c = self.client().await?;
        let h = c.health().await?;
        Ok(pretty(json!({
            "version": h.version,
            "loaded_engine": h.loaded_engine,
            "output_dir": if self.tunnel { "（通道模式不顯示）".to_string() } else { h.output_dir },
            "can_play_on_this_computer": h.can_play && !self.tunnel,
        })))
    }

    #[tool(
        description = "列出 TTS 引擎：是否已安裝、目前狀態、支援的功能（複製聲音、語氣指示、情緒、語速、注音）與授權。",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn list_engines(&self) -> Result<String, String> {
        let list = self.client().await?.engines().await?;
        let engines: Vec<_> = list
            .engines
            .iter()
            .map(|e| {
                let m = &e.manifest;
                json!({
                    "id": m.id, "name": m.name, "state": e.state, "summary": m.summary,
                    "languages": m.languages, "capabilities": m.capabilities, "license": m.license,
                    "builtin_voice_count": m.builtin_voices.len(), "last_error": e.last_error,
                })
            })
            .collect();
        Ok(pretty(json!({ "loaded": list.loaded, "engines": engines })))
    }

    #[tool(
        description = "列出可用的聲音：聲音庫（使用者建立的複製聲音）與各引擎的內建說話人。回傳的 name 直接填到 speak 的 voice。",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn list_voices(&self, Parameters(p): Parameters<VoicesParams>) -> Result<String, String> {
        let list = self.client().await?.voices().await?;
        let tunnel = self.tunnel;
        let voices: Vec<_> = list
            .voices
            .into_iter()
            .filter(|v| p.engine.as_deref().is_none_or(|e| v.engine_id.eq_ignore_ascii_case(e)))
            .map(|mut v| {
                // 通道模式不透露參考音檔的逐字稿。
                if tunnel && v.description.contains("逐字稿") {
                    v.description = "聲音庫的聲音".into();
                }
                v
            })
            .collect();
        Ok(pretty(json!({ "count": voices.len(), "voices": voices })))
    }

    #[tool(
        description = "文字轉語音：合成一段話並存成 WAV，回傳檔案路徑與長度。長文會自動分段、合併並做響度正規化。",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = false, open_world_hint = false)
    )]
    async fn speak(&self, Parameters(p): Parameters<SpeakParams>) -> Result<String, String> {
        let emotion = if p.emotion.is_some() || p.emotion_vector.is_some() {
            Some(EmotionSpec { text: p.emotion, vector: p.emotion_vector, strength: p.emotion_strength })
        } else {
            None
        };
        let body = SpeakBody {
            request: SpeakRequest {
                text: p.text,
                voice: p.voice,
                instruct: p.instruct,
                emotion,
                speed: p.speed,
                language: p.language,
                seed: p.seed,
                file_stem: p.file_name,
                project: p.project,
                ..Default::default()
            },
            play: p.play && !self.tunnel,
        };
        let take = self.client().await?.speak(&body).await?;
        Ok(pretty(take_json(&take, self.tunnel)))
    }

    #[tool(
        description = "依腳本逐行合成，每行產生一個音檔，適合廣播劇、Podcast、有聲書章節或多角色對話。",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = false, open_world_hint = false)
    )]
    async fn render_script(&self, Parameters(p): Parameters<ScriptParams>) -> Result<String, String> {
        let body = ScriptBody { project: p.project, script: p.script, default_voice: p.default_voice };
        let list = self.client().await?.script(&body).await?;
        let total: f64 = list.takes.iter().map(|t| t.duration_sec).sum();
        let takes: Vec<_> = list.takes.iter().map(|t| take_json(t, self.tunnel)).collect();
        Ok(pretty(json!({ "lines": takes.len(), "total_duration_sec": (total * 10.0).round() / 10.0, "takes": takes })))
    }

    #[tool(
        description = "列出最近的錄音（take），含檔案路徑、聲音與長度。",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn list_takes(&self, Parameters(p): Parameters<TakesParams>) -> Result<String, String> {
        let list = self.client().await?.takes(p.limit.unwrap_or(10), p.project.as_deref()).await?;
        let takes: Vec<_> = list
            .takes
            .iter()
            .map(|t| {
                let mut v = take_json(t, self.tunnel);
                let text: String = t.text.chars().take(60).collect();
                v["text"] = json!(text);
                v
            })
            .collect();
        Ok(pretty(json!({ "takes": takes })))
    }

    #[tool(
        description = "用參考音檔建立可複製的聲音並存進聲音庫；GPT-SoVITS 還可以一起匯入訓練好的角色模型權重。只能用在使用者本人或已取得同意的聲音；呼叫前必須先向使用者確認同意依據。",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = false, open_world_hint = false)
    )]
    async fn create_voice(&self, Parameters(p): Parameters<CreateVoiceParams>) -> Result<String, String> {
        let body = CreateVoice {
            name: p.name.clone(),
            engine: p.engine,
            audio_path: p.audio_path,
            transcript: p.transcript,
            consent_basis: p.consent_basis.into(),
            consent_statement: p.consent_statement,
            reference_language: p.reference_language,
            gpt_weights_path: p.gpt_weights_path,
            sovits_weights_path: p.sovits_weights_path,
        };
        self.client().await?.create_voice(&body).await?;
        Ok(format!("已建立聲音「{}」。之後在 speak 的 voice 填「{}」即可使用。", p.name, p.name))
    }

    #[tool(
        description = "預先載入某個引擎（模型），讓之後的合成不用等載入。會先卸載目前的模型。",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true, open_world_hint = false)
    )]
    async fn load_engine(&self, Parameters(p): Parameters<EngineParams>) -> Result<String, String> {
        let list = self.client().await?.load_engine(&p.engine).await?;
        Ok(format!("已載入 {}。", list.loaded.unwrap_or(p.engine)))
    }

    #[tool(
        description = "卸載目前的模型，釋放顯示記憶體（例如使用者要玩遊戲或跑其他 AI）。",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true, open_world_hint = false)
    )]
    async fn unload_engine(&self) -> Result<String, String> {
        self.client().await?.unload_engine().await?;
        Ok("已卸載模型，顯示記憶體已釋放。".into())
    }

    #[tool(
        description = "在 Cantiva 主視窗播放某個錄音，讓使用者直接聽。",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn play_take(&self, Parameters(p): Parameters<PlayParams>) -> Result<String, String> {
        self.client().await?.play(&p.take_id).await?;
        Ok("開始播放。".into())
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for Cantiva {
    fn get_info(&self) -> rmcp::model::ServerConfig {
        rmcp::model::ServerConfig::new(rmcp::model::ServerCapabilities::builder().enable_tools().build())
            .with_server_info(rmcp::model::Implementation::new("cantiva", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS.to_string())
    }
}
