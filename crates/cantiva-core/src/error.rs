use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("資料庫錯誤：{0}")]
    Db(#[from] rusqlite::Error),
    #[error("讀寫檔案失敗（{path}）：{source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("設定檔格式錯誤（{path}）：{message}")]
    Config { path: PathBuf, message: String },
    #[error("找不到{kind}「{id}」")]
    NotFound { kind: &'static str, id: String },
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io { path: path.into(), source }
    }

    pub fn not_found(kind: &'static str, id: impl Into<String>) -> Self {
        Self::NotFound { kind, id: id.into() }
    }
}
