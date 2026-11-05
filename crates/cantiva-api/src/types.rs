//! 控制 API 的請求與回應型別，server 與 client 共用。

use cantiva_core::model::{ConsentBasis, EngineStatus, SpeakRequest, Take};
use cantiva_engine::studio::VoiceChoice;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Health {
    pub app: String,
    pub version: String,
    pub loaded_engine: Option<String>,
    /// GUI 有開時才能在本機播放。
    pub can_play: bool,
    pub output_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineList {
    pub engines: Vec<EngineStatus>,
    pub loaded: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadEngine {
    pub engine: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceList {
    pub voices: Vec<VoiceChoice>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateVoice {
    pub name: String,
    pub engine: String,
    /// 本機 WAV 檔的絕對路徑；會複製到 Cantiva 的 references 資料夾。
    pub audio_path: String,
    pub transcript: String,
    pub consent_basis: ConsentBasis,
    pub consent_statement: String,
    /// 參考音檔的語言（GPT-SoVITS 用），例如 "ja"。
    #[serde(default)]
    pub reference_language: Option<String>,
    /// GPT-SoVITS 角色模型：GPT 權重（.ckpt）與 SoVITS 權重（.pth）的絕對路徑。
    #[serde(default)]
    pub gpt_weights_path: Option<String>,
    #[serde(default)]
    pub sovits_weights_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeakBody {
    #[serde(flatten)]
    pub request: SpeakRequest,
    /// 合成後是否在 Cantiva 播放（需要 GUI 開著）。
    #[serde(default)]
    pub play: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptBody {
    pub project: String,
    pub script: String,
    #[serde(default)]
    pub default_voice: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TakeList {
    pub takes: Vec<Take>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayBody {
    pub take_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiError {
    pub error: String,
}

