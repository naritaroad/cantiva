//! 資料模型：引擎描述、聲音、合成請求與結果、專案與錄音片段（take）。

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

/// 一個 TTS 模型的描述，來自 `engines/<id>/engine.toml`，隨程式一起發佈。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EngineManifest {
    pub id: String,
    pub name: String,
    pub vendor: String,
    /// 給人看的一句話介紹。
    pub summary: String,
    /// 權重授權的簡述，例如「Apache-2.0，可商用」。
    pub license: String,
    pub license_url: String,
    pub homepage: String,
    /// 建議的最小顯示記憶體（GB）。
    pub min_vram_gb: f32,
    pub sample_rate: u32,
    pub languages: Vec<String>,
    pub capabilities: Capabilities,
    /// 內建聲音（沒有內建聲音的模型為空，必須用參考音檔）。
    #[serde(default)]
    pub builtin_voices: Vec<BuiltinVoice>,
    /// 單次送給模型的建議字數上限，長文會依此分段。
    #[serde(default = "default_chunk_chars")]
    pub chunk_chars: usize,
    /// 在引擎清單中的排序（小的在前）。
    #[serde(default = "default_order")]
    pub order: u32,
}

fn default_order() -> u32 {
    100
}

fn default_chunk_chars() -> usize {
    200
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Capabilities {
    /// 可以用 3–15 秒參考音檔複製聲音。
    #[serde(default)]
    pub clone: bool,
    /// 可以用自然語言指示語氣（例如「溫柔、放慢」）。
    #[serde(default)]
    pub instruct: bool,
    /// 支援情緒控制（情緒參考音檔或情緒向量）。
    #[serde(default)]
    pub emotion: bool,
    /// 可以用文字描述「設計」一個新聲音。
    #[serde(default)]
    pub design: bool,
    /// 支援用注音指定破音字讀音。
    #[serde(default)]
    pub zhuyin: bool,
    /// 支援調整語速。
    #[serde(default)]
    pub speed: bool,
    /// 可以匯入微調過的角色模型權重（GPT-SoVITS）。
    #[serde(default)]
    pub trained_models: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BuiltinVoice {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub gender: String,
}

/// 聲音庫裡的一個聲音：引擎內建的，或使用者用參考音檔建立的。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Voice {
    pub id: Uuid,
    pub name: String,
    pub engine_id: String,
    pub kind: VoiceKind,
    #[serde(default)]
    pub notes: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum VoiceKind {
    /// 引擎內建的說話人。
    Builtin { speaker: String },
    /// 參考音檔複製的聲音。檔案存在 `references/` 底下。
    Reference {
        /// 相對於 references 資料夾的檔名。
        audio_file: String,
        /// 參考音檔的逐字稿；多數模型要求和音檔內容完全一致。
        transcript: String,
        consent: Consent,
        /// 參考音檔的語言（GPT-SoVITS 需要，例如 "ja"）；舊資料沒有這個欄位。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        language: Option<String>,
        /// 微調過的模型權重（GPT-SoVITS 的角色模型），相對於 references 資料夾。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        trained: Option<TrainedWeights>,
    },
}

/// GPT-SoVITS 訓練產生的一組權重：GPT（.ckpt）與 SoVITS（.pth）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TrainedWeights {
    pub gpt_file: String,
    pub sovits_file: String,
}

/// 複製聲音前必須留下的同意紀錄。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Consent {
    pub basis: ConsentBasis,
    /// 誰同意、何時、怎麼同意的，給使用者自己留存。
    pub statement: String,
    #[serde(with = "time::serde::rfc3339")]
    pub recorded_at: OffsetDateTime,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConsentBasis {
    /// 使用者自己的聲音。
    OwnVoice,
    /// 聲音主人已同意（書面或錄音）。
    WrittenPermission,
    /// 授權允許複製的素材（例如有授權的配音庫）。
    LicensedMaterial,
}

/// 情緒控制。不同模型支援的方式不同，engine worker 會挑自己懂的欄位。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct EmotionSpec {
    /// 用一句話描述情緒，例如「難過、壓低聲音」。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// IndexTTS 的八維情緒向量：喜、怒、哀、懼、厭惡、低落、驚訝、平靜，各 0–1。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vector: Option<[f32; 8]>,
    /// 情緒強度 0–1。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strength: Option<f32>,
}

/// 送給引擎 worker 的一段合成請求（已分段後的單段文字）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SegmentRequest {
    pub text: String,
    /// 內建說話人 id；用參考音檔時為 None。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
    /// 參考音檔的絕對路徑。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_audio: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_text: Option<String>,
    /// 參考音檔的語言（GPT-SoVITS 用）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_language: Option<String>,
    /// 角色模型權重的絕對路徑（GPT-SoVITS 用）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpt_weights: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sovits_weights: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instruct: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emotion: Option<EmotionSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    /// worker 要把 WAV 寫到這個絕對路徑。
    pub output_path: String,
}

/// 使用者（GUI、MCP、API）發出的一次完整合成請求。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct SpeakRequest {
    pub text: String,
    /// 聲音庫的聲音名稱或 id；省略時用目前引擎的預設聲音。
    #[serde(default)]
    pub voice: Option<String>,
    /// 指定引擎；省略時用聲音所屬引擎或目前作用中的引擎。
    #[serde(default)]
    pub engine: Option<String>,
    #[serde(default)]
    pub instruct: Option<String>,
    #[serde(default)]
    pub emotion: Option<EmotionSpec>,
    #[serde(default)]
    pub speed: Option<f32>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub seed: Option<u64>,
    /// 輸出檔名（不含副檔名）。
    #[serde(default)]
    pub file_stem: Option<String>,
    /// 歸到哪個專案；省略時歸到「快速合成」。
    #[serde(default)]
    pub project: Option<String>,
    /// 是否套用響度正規化（預設依設定）。
    #[serde(default)]
    pub normalize: Option<bool>,
}

/// 一次合成產生的錄音片段。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Take {
    pub id: Uuid,
    pub project_id: Uuid,
    pub text: String,
    pub voice_name: String,
    pub engine_id: String,
    pub path: String,
    pub duration_sec: f64,
    pub sample_rate: u32,
    pub segments: u32,
    /// 合成耗時，含模型載入。
    pub elapsed_sec: f64,
    #[serde(default)]
    pub loudness_lufs: Option<f64>,
    #[serde(default)]
    pub starred: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    /// 腳本原文，格式見 `text::parse_script`。
    pub script: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

/// 引擎目前的狀態，給 UI 與 MCP 顯示。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EngineState {
    /// 還沒安裝虛擬環境或權重。
    NotInstalled,
    /// 已安裝，沒有載入。
    Idle,
    /// worker 正在啟動、載入權重。
    Loading,
    /// 已載入，可以合成。
    Ready,
    /// 正在合成。
    Busy,
    /// 上次啟動失敗，見 last_error。
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EngineStatus {
    pub manifest: EngineManifest,
    pub state: EngineState,
    #[serde(default)]
    pub last_error: Option<String>,
    /// 目前載入時佔用的顯示記憶體（MB），worker 回報。
    #[serde(default)]
    pub vram_mb: Option<u64>,
}

pub fn now() -> OffsetDateTime {
    OffsetDateTime::now_utc()
}
