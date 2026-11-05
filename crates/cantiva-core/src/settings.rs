//! 使用者設定，存成 `settings.toml`，人可以直接編輯。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// 上次使用的引擎，啟動時預先選好（不會自動載入）。
    pub active_engine: Option<String>,
    /// 音檔輸出資料夾；None 表示用預設位置。
    pub output_dir: Option<PathBuf>,
    /// 合成後是否把響度正規化到 `target_lufs`。
    pub normalize_loudness: bool,
    /// 目標整合響度。Podcast 與 YouTube 常用 -16 到 -14 LUFS。
    pub target_lufs: f64,
    /// 閒置多少分鐘後卸載模型，釋放顯示記憶體；0 表示不自動卸載。
    pub unload_after_minutes: u32,
    pub mcp: McpSettings,
    pub tunnel: TunnelSettings,
    /// 介面語言：zh-TW 或 en。
    pub locale: String,
    /// 上次開著的專案，下次啟動時打開它。
    pub last_project: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            active_engine: Some("qwen3-tts".into()),
            output_dir: None,
            normalize_loudness: true,
            target_lufs: -16.0,
            unload_after_minutes: 15,
            mcp: McpSettings::default(),
            tunnel: TunnelSettings::default(),
            locale: "zh-TW".into(),
            last_project: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct McpSettings {
    /// 本機控制 API 的連接埠（只綁定 127.0.0.1）；0 表示自動挑選。
    pub api_port: u16,
    /// MCP Streamable HTTP 的連接埠（給通道或其他 HTTP client 用）。
    pub http_port: u16,
    /// 允許 agent 使用複製聲音的工具。
    pub allow_clone_tools: bool,
    /// 單次合成的字數上限，避免 agent 一次丟進整本書。
    pub max_chars_per_request: usize,
}

impl Default for McpSettings {
    fn default() -> Self {
        Self { api_port: 47820, http_port: 47821, allow_clone_tools: true, max_chars_per_request: 6000 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
#[derive(Default)]
pub struct TunnelSettings {
    /// 對外通道預設關閉，只有在設定頁手動開啟才會啟動。
    pub enabled: bool,
    /// cloudflared 執行檔路徑；None 表示從 PATH 找。
    pub cloudflared_path: Option<PathBuf>,
    /// 透過通道進來的請求是否允許複製聲音。預設不允許。
    pub allow_clone_over_tunnel: bool,
}


impl Settings {
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text)
                .map_err(|e| Error::Config { path: path.to_path_buf(), message: e.to_string() }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(Error::io(path, e)),
        }
    }

    /// 先寫到暫存檔再改名，避免寫到一半當機留下壞檔。
    pub fn save(&self, path: &Path) -> Result<()> {
        let text = toml::to_string_pretty(self)
            .map_err(|e| Error::Config { path: path.to_path_buf(), message: e.to_string() })?;
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text).map_err(|e| Error::io(&tmp, e))?;
        std::fs::rename(&tmp, path).map_err(|e| Error::io(path, e))
    }

    pub fn validate(&self) -> Result<()> {
        if !(-40.0..=-5.0).contains(&self.target_lufs) {
            return Err(Error::Invalid("目標響度要在 -40 到 -5 LUFS 之間".into()));
        }
        if self.mcp.max_chars_per_request == 0 {
            return Err(Error::Invalid("單次字數上限不能是 0".into()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_partial_files() {
        let dir = std::env::temp_dir().join(format!("cantiva-settings-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.toml");
        assert_eq!(Settings::load(&path).unwrap(), Settings::default());

        let mut s = Settings { target_lufs: -14.0, ..Default::default() };
        s.tunnel.enabled = true;
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), s);

        std::fs::write(&path, "target_lufs = -18.0\n[tunnel]\nenabled = true\n").unwrap();
        let partial = Settings::load(&path).unwrap();
        assert_eq!(partial.target_lufs, -18.0);
        assert!(partial.tunnel.enabled);
        assert!(!partial.tunnel.allow_clone_over_tunnel);
        assert_eq!(partial.mcp.api_port, 47820);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn tunnel_is_off_by_default() {
        let s = Settings::default();
        assert!(!s.tunnel.enabled);
        assert!(!s.tunnel.allow_clone_over_tunnel);
        assert!(s.validate().is_ok());
    }
}
