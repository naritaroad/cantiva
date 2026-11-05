#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error(transparent)]
    Core(#[from] cantiva_core::Error),
    #[error(transparent)]
    Audio(#[from] cantiva_audio::AudioError),
    #[error("找不到引擎「{0}」")]
    UnknownEngine(String),
    #[error("引擎「{0}」還沒安裝。請在「引擎」頁按「安裝」，或執行 scripts\\install-engine.ps1 {0}")]
    NotInstalled(String),
    #[error("引擎「{engine}」啟動失敗：{message}")]
    StartFailed { engine: String, message: String },
    #[error("引擎「{engine}」合成失敗：{message}")]
    Synthesis { engine: String, message: String },
    #[error("找不到聲音「{0}」。可用 list_voices 查看聲音庫")]
    UnknownVoice(String),
    #[error("{0}")]
    Invalid(String),
    #[error("工作已取消")]
    Cancelled,
}

pub type Result<T> = std::result::Result<T, EngineError>;
