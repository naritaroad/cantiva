//! 本機控制 API：讓 MCP server（與其他本機工具）操作同一個 Studio。
//!
//! - 只綁 127.0.0.1，每個請求都要帶 `Authorization: Bearer <token>`。
//! - 權杖存在資料夾的 `api-token`，實際連接埠寫在 `api-port`。
//! - 由 GUI 或 `cantiva --headless` 啟動；兩者不會同時跑（見 [`ensure_single`]）。

pub mod client;
pub mod server;
pub mod types;

pub use client::ApiClient;
pub use server::{Player, ServeOptions, serve};

use cantiva_core::paths::AppPaths;
use rand::RngExt;

/// 讀取或建立控制 API 的權杖。
pub fn load_or_create_token(paths: &AppPaths) -> std::io::Result<String> {
    let path = paths.api_token();
    if let Ok(t) = std::fs::read_to_string(&path) {
        let t = t.trim().to_string();
        if t.len() >= 32 {
            return Ok(t);
        }
    }
    let token: String = {
        let mut rng = rand::rng();
        (0..48).map(|_| format!("{:x}", rng.random_range(0..16u8))).collect()
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, &token)?;
    Ok(token)
}

/// MCP Streamable HTTP 與對外通道用的密鑰（放在網址路徑或 Bearer 標頭）。
pub fn load_or_create_mcp_secret(paths: &AppPaths) -> std::io::Result<String> {
    let path = paths.root().join("mcp-secret");
    if let Ok(s) = std::fs::read_to_string(&path) {
        let s = s.trim().to_string();
        if s.len() >= 32 {
            return Ok(s);
        }
    }
    let secret: String = {
        let mut rng = rand::rng();
        (0..40).map(|_| format!("{:x}", rng.random_range(0..16u8))).collect()
    };
    std::fs::create_dir_all(paths.root())?;
    std::fs::write(&path, &secret)?;
    Ok(secret)
}

pub fn read_token(paths: &AppPaths) -> Option<String> {
    std::fs::read_to_string(paths.api_token()).ok().map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
}

pub fn read_port(paths: &AppPaths) -> Option<u16> {
    std::fs::read_to_string(paths.api_port_file()).ok()?.trim().parse().ok()
}

/// 檢查是否已經有另一個 Cantiva（GUI 或無視窗模式）在跑。有的話回傳它的客戶端。
pub async fn ensure_single(paths: &AppPaths) -> Option<ApiClient> {
    let client = ApiClient::from_paths(paths)?;
    client.health().await.ok().map(|_| client)
}
