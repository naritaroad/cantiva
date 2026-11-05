//! 所有檔案位置集中在這裡。
//!
//! Windows 預設放在 `%USERPROFILE%\Cantiva`，其他系統用作業系統的應用程式資料夾；
//! 設定環境變數 `CANTIVA_HOME` 可以整個搬走，測試也靠它把資料寫到暫存資料夾。
//!
//! Windows 不放 `%APPDATA%`：從 MSIX 套件程式（例如 Claude 桌面版）啟動的程序寫入 AppData 時，
//! 會被轉到該套件的私有資料夾，其他程式看不到。這樣 agent 合成的音檔、裝好的引擎
//! 會和使用者自己開的 Cantiva 分成兩份。使用者資料夾不會被轉址。

use std::path::{Path, PathBuf};

use crate::{Error, Result};

/// 沒有設定 `CANTIVA_HOME` 時的根目錄。engines/_common.ps1 的 Get-CantivaHome 要保持一致。
#[cfg(windows)]
fn default_root() -> Result<PathBuf> {
    let dirs = directories::BaseDirs::new().ok_or_else(|| Error::Invalid("找不到使用者資料夾".into()))?;
    Ok(dirs.home_dir().join("Cantiva"))
}

#[cfg(not(windows))]
fn default_root() -> Result<PathBuf> {
    let dirs = directories::ProjectDirs::from("tw", "Naritaroad", "Cantiva")
        .ok_or_else(|| Error::Invalid("找不到使用者資料夾".into()))?;
    Ok(dirs.data_dir().to_path_buf())
}

#[derive(Debug, Clone)]
pub struct AppPaths {
    root: PathBuf,
}

impl AppPaths {
    /// 依 `CANTIVA_HOME` 或作業系統的使用者資料夾決定根目錄。
    pub fn discover() -> Result<Self> {
        if let Some(home) = std::env::var_os("CANTIVA_HOME") {
            return Ok(Self::at(PathBuf::from(home)));
        }
        Ok(Self::at(default_root()?))
    }

    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn database(&self) -> PathBuf {
        self.root.join("cantiva.db")
    }

    pub fn settings(&self) -> PathBuf {
        self.root.join("settings.toml")
    }

    /// 本機控制 API 的存取權杖。只有同一位使用者讀得到。
    pub fn api_token(&self) -> PathBuf {
        self.root.join("api-token")
    }

    /// 控制 API 啟動後寫下實際的連接埠，MCP server 讀它來連線。
    pub fn api_port_file(&self) -> PathBuf {
        self.root.join("api-port")
    }

    /// 預設的輸出資料夾（使用者可在設定裡改）。
    pub fn default_output_dir(&self) -> PathBuf {
        self.root.join("takes")
    }

    /// 複製聲音用的參考音檔，App 匯入時會複製一份到這裡。
    pub fn references(&self) -> PathBuf {
        self.root.join("references")
    }

    /// 各模型的虛擬環境與權重放在 `engines/<id>`。
    pub fn engine_home(&self, engine_id: &str) -> PathBuf {
        self.root.join("engines").join(engine_id)
    }

    pub fn logs(&self) -> PathBuf {
        self.root.join("logs")
    }

    pub fn ensure(&self) -> Result<()> {
        for dir in [self.root.clone(), self.references(), self.logs(), self.default_output_dir()] {
            std::fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
        }
        Ok(())
    }
}

/// 把使用者或 agent 給的檔名清成安全的檔名（不含路徑、不含 Windows 保留字元與保留名稱）。
pub fn sanitize_file_stem(input: &str) -> String {
    let cleaned: String = input
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .take(80)
        .collect();
    // 先截斷再修剪，結果才不會以句點或空白結尾（Windows 不允許）。
    let stem = cleaned.trim().trim_matches('.').trim().to_string();
    let base = stem.split('.').next().unwrap_or("").trim().to_ascii_uppercase();
    let reserved = matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$")
        || ((base.starts_with("COM") || base.starts_with("LPT"))
            && base.len() == 4
            && base.as_bytes()[3].is_ascii_digit());
    if stem.is_empty() || reserved { "take".into() } else { stem }
}

/// 在 `dir` 底下找一個不會覆蓋舊檔的路徑。
pub fn unique_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let stem = sanitize_file_stem(stem);
    let mut candidate = dir.join(format!("{stem}.{ext}"));
    let mut n = 2;
    while candidate.exists() {
        candidate = dir.join(format!("{stem}-{n}.{ext}"));
        n += 1;
    }
    candidate
}

/// `path` 是否確實位在 `base` 裡（擋掉 `..` 與符號連結跳出）。
pub fn is_within(base: &Path, path: &Path) -> bool {
    match (base.canonicalize(), path.canonicalize()) {
        (Ok(base), Ok(path)) => path.starts_with(base),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_paths_and_reserved_names() {
        assert_eq!(sanitize_file_stem("../../etc/passwd"), "_.._etc_passwd");
        assert_eq!(sanitize_file_stem("旁白 第1段"), "旁白 第1段");
        assert_eq!(sanitize_file_stem("con"), "take");
        assert_eq!(sanitize_file_stem("   "), "take");
        assert_eq!(sanitize_file_stem("a:b*c?"), "a_b_c_");
        assert_eq!(sanitize_file_stem("nul.x"), "take");
        assert_eq!(sanitize_file_stem("COM7"), "take");
        assert_eq!(sanitize_file_stem("CONOUT$"), "take");
        assert_eq!(sanitize_file_stem("COMPANY"), "COMPANY");
        let long = format!("{}.", "字".repeat(79));
        assert!(!sanitize_file_stem(&long).ends_with('.'));
    }

    #[test]
    fn unique_path_does_not_overwrite() {
        let dir = std::env::temp_dir().join(format!("cantiva-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let first = unique_path(&dir, "x", "wav");
        std::fs::write(&first, b"1").unwrap();
        let second = unique_path(&dir, "x", "wav");
        assert_ne!(first, second);
        assert!(second.ends_with("x-2.wav"));
        assert!(is_within(&dir, &first));
        assert!(!is_within(&dir, Path::new("/")));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
