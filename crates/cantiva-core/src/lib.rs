//! Cantiva 的核心：資料模型、路徑、設定、SQLite 儲存與文字分段。
//!
//! 這個 crate 不碰 UI、不碰網路，也不啟動任何程序，所有邏輯都可以單元測試。

pub mod error;
pub mod model;
pub mod paths;
pub mod settings;
pub mod store;
pub mod text;

pub use error::{Error, Result};
