//! 引擎目錄：程式附帶的 `engines/` 資料夾裡，每個子資料夾是一個引擎。
//!
//! ```text
//! engines/
//!   _worker/cantiva_worker/   共用的 worker 程式（純標準函式庫）
//!   qwen3-tts/engine.toml      模型描述
//!   qwen3-tts/adapter.py       把模型的 Python API 接到共用協定
//!   qwen3-tts/install.ps1      建立虛擬環境、下載權重
//! ```
//!
//! 安裝結果放在使用者資料夾 `engines/<id>/`，裝好後寫一個 `installed.json`。

use std::path::{Path, PathBuf};

use cantiva_core::model::EngineManifest;
use cantiva_core::paths::AppPaths;

use crate::{EngineError, Result};

#[derive(Debug, Clone)]
pub struct EngineEntry {
    pub manifest: EngineManifest,
    /// 程式附帶的引擎資料夾（含 adapter.py）。
    pub source_dir: PathBuf,
    /// 使用者資料夾裡的安裝位置（含 .venv 與權重）。
    pub home: PathBuf,
}

impl EngineEntry {
    pub fn adapter(&self) -> PathBuf {
        self.source_dir.join("adapter.py")
    }

    pub fn python(&self) -> PathBuf {
        if cfg!(windows) {
            self.home.join(".venv").join("Scripts").join("python.exe")
        } else {
            self.home.join(".venv").join("bin").join("python")
        }
    }

    pub fn installed_marker(&self) -> PathBuf {
        self.home.join("installed.json")
    }

    pub fn is_installed(&self) -> bool {
        self.installed_marker().is_file() && self.python().is_file()
    }

    pub fn install_script(&self) -> PathBuf {
        self.source_dir.join("install.ps1")
    }
}

#[derive(Debug, Clone)]
pub struct Catalog {
    engines_dir: PathBuf,
    entries: Vec<EngineEntry>,
}

impl Catalog {
    /// 找附帶的 `engines/` 資料夾：`CANTIVA_ENGINES_DIR` → 執行檔旁 → 目前資料夾。
    pub fn locate_engines_dir() -> Option<PathBuf> {
        if let Some(dir) = std::env::var_os("CANTIVA_ENGINES_DIR") {
            return Some(PathBuf::from(dir));
        }
        let mut candidates = Vec::new();
        if let Ok(exe) = std::env::current_exe()
            && let Some(dir) = exe.parent() {
                candidates.push(dir.join("engines"));
                // 開發時執行檔在 target/debug，往上找到專案根目錄。
                for up in dir.ancestors().skip(1).take(3) {
                    candidates.push(up.join("engines"));
                }
            }
        candidates.push(PathBuf::from("engines"));
        candidates.into_iter().find(|d| d.join("_worker").is_dir())
    }

    pub fn load(engines_dir: &Path, paths: &AppPaths) -> Result<Self> {
        let mut entries = Vec::new();
        let read = std::fs::read_dir(engines_dir)
            .map_err(|e| cantiva_core::Error::io(engines_dir, e))?;
        for item in read.flatten() {
            let dir = item.path();
            let manifest_path = dir.join("engine.toml");
            if !manifest_path.is_file() {
                continue;
            }
            let text = std::fs::read_to_string(&manifest_path)
                .map_err(|e| cantiva_core::Error::io(&manifest_path, e))?;
            let manifest: EngineManifest = toml::from_str(&text).map_err(|e| cantiva_core::Error::Config {
                path: manifest_path.clone(),
                message: e.to_string(),
            })?;
            // 測試用的假引擎只在開發模式出現。
            if manifest.id.starts_with("dev-") && std::env::var_os("CANTIVA_DEV").is_none() {
                continue;
            }
            let home = paths.engine_home(&manifest.id);
            entries.push(EngineEntry { manifest, source_dir: dir, home });
        }
        entries.sort_by(|a, b| a.manifest.order.cmp(&b.manifest.order).then_with(|| a.manifest.name.cmp(&b.manifest.name)));
        Ok(Self { engines_dir: engines_dir.to_path_buf(), entries })
    }

    pub fn engines_dir(&self) -> &Path {
        &self.engines_dir
    }

    pub fn worker_pythonpath(&self) -> PathBuf {
        self.engines_dir.join("_worker")
    }

    pub fn entries(&self) -> &[EngineEntry] {
        &self.entries
    }

    pub fn get(&self, id: &str) -> Result<&EngineEntry> {
        self.entries
            .iter()
            .find(|e| e.manifest.id.eq_ignore_ascii_case(id))
            .ok_or_else(|| EngineError::UnknownEngine(id.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_repo_manifests() {
        let repo_engines = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../engines");
        let tmp = std::env::temp_dir().join(format!("cantiva-cat-{}", std::process::id()));
        let paths = AppPaths::at(&tmp);
        let cat = Catalog::load(&repo_engines, &paths).unwrap();
        let ids: Vec<_> = cat.entries().iter().map(|e| e.manifest.id.as_str()).collect();
        for id in ["qwen3-tts", "cosyvoice3", "indextts2", "breezyvoice", "gpt-sovits"] {
            assert!(ids.contains(&id), "missing {id} in {ids:?}");
        }
        let q = cat.get("QWEN3-TTS").unwrap();
        assert!(q.adapter().is_file());
        assert!(!q.is_installed());
        assert!(cat.get("nope").is_err());
    }
}
