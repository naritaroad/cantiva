//! 各 AI agent 的 MCP 設定範例（2026-11-05 依各家官方文件查證）。

use std::path::Path;

pub struct ClientSnippet {
    pub client: &'static str,
    pub location: &'static str,
    pub snippet: String,
    pub note: &'static str,
}

pub fn snippets(mcp_exe: &Path) -> Vec<ClientSnippet> {
    let exe = mcp_exe.display().to_string();
    let json_exe = exe.replace('\\', "\\\\");
    // TOML 基本字串（雙引號）：和 JSON 一樣跳脫反斜線與雙引號。
    let toml_exe = json_exe.replace('"', "\\\"");
    vec![
        ClientSnippet {
            client: "Claude Code",
            location: "在終端機執行（--scope user 讓所有專案都能用）",
            snippet: format!("claude mcp add --scope user cantiva -- \"{exe}\""),
            note: "",
        },
        ClientSnippet {
            client: "Claude Desktop",
            location: "%APPDATA%\\Claude\\claude_desktop_config.json 的 mcpServers 裡加入，存檔後重新開啟 Claude Desktop",
            snippet: format!("\"cantiva\": {{\n  \"command\": \"{json_exe}\",\n  \"args\": []\n}}"),
            note: "",
        },
        ClientSnippet {
            client: "Codex（CLI、IDE 擴充、Codex 桌面版共用）",
            location: "%USERPROFILE%\\.codex\\config.toml",
            snippet: format!("[mcp_servers.cantiva]\ncommand = \"{toml_exe}\"\nstartup_timeout_sec = 30\ntool_timeout_sec = 1800"),
            note: "Codex 預設工具逾時 60 秒，第一次載入模型會超過，所以調長。",
        },
        ClientSnippet {
            client: "Cursor",
            location: "%USERPROFILE%\\.cursor\\mcp.json（全域）或專案的 .cursor\\mcp.json 的 mcpServers 裡加入",
            snippet: format!("\"cantiva\": {{\n  \"type\": \"stdio\",\n  \"command\": \"{json_exe}\",\n  \"args\": []\n}}"),
            note: "",
        },
        ClientSnippet {
            client: "Grok Build CLI",
            location: "%USERPROFILE%\\.grok\\config.toml，或執行 grok mcp add cantiva -- <路徑>",
            snippet: format!("[mcp_servers.cantiva]\ncommand = \"{toml_exe}\"\nargs = []"),
            note: "Grok Bot 跑在雲端，連不到你的電腦，請改用右邊的對外通道。",
        },
    ]
}
