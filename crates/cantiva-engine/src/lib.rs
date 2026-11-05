//! 引擎層：找到模型描述、管理 Python worker 程序、把長文合成成一個 take。
//!
//! 結構：
//! - [`catalog`]：讀取 `engines/<id>/engine.toml`，判斷每個引擎是否已安裝。
//! - [`worker`]：啟動與監看單一 Python worker，透過本機 HTTP 協定溝通。
//! - [`studio`]：給 GUI、控制 API、MCP 共用的服務物件。同一時間只讓一個 GPU 模型常駐。

pub mod catalog;
pub mod error;
pub mod studio;
pub mod worker;

pub use error::{EngineError, Result};
pub use studio::{Progress, Studio, StudioConfig};
