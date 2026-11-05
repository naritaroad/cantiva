//! 作業系統整合：開資料夾、在檔案總管中顯示、開新視窗執行安裝腳本、偵測顯示卡、找 cloudflared。

use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn open_path(path: &Path) -> Result<(), String> {
    let result = if cfg!(windows) {
        Command::new("explorer").arg(path).spawn()
    } else if cfg!(target_os = "macos") {
        Command::new("open").arg(path).spawn()
    } else {
        Command::new("xdg-open").arg(path).spawn()
    };
    result.map(|_| ()).map_err(|e| format!("無法開啟 {}：{e}", path.display()))
}

/// 在檔案總管裡選取這個檔案。
pub fn reveal(path: &Path) -> Result<(), String> {
    if cfg!(windows) {
        // explorer 的 /select, 參數要和路徑寫成同一個引數。
        Command::new("explorer")
            .arg(format!("/select,{}", path.display()))
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("無法開啟檔案總管：{e}"))
    } else {
        open_path(path.parent().unwrap_or(path))
    }
}

/// 在新的 PowerShell 視窗執行引擎的安裝腳本，讓使用者看得到下載進度。
pub fn run_install_script(script: &Path, engine_name: &str) -> Result<(), String> {
    if !cfg!(windows) {
        return Err(format!("請在 Windows 上執行 {}", script.display()));
    }
    let title = format!("Cantiva 安裝 {engine_name}");
    Command::new("cmd")
        .args(["/c", "start", &title, "powershell", "-NoExit", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(script)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("無法開啟 PowerShell：{e}"))
}

/// 用 nvidia-smi 查顯示卡名稱與記憶體（MiB）。沒有 NVIDIA 顯示卡時回傳 None。
pub fn detect_gpu() -> Option<(String, u64)> {
    let mut cmd = Command::new("nvidia-smi");
    cmd.args(["--query-gpu=name,memory.total", "--format=csv,noheader,nounits"]);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().next()?;
    let (name, mem) = line.rsplit_once(',')?;
    Some((name.trim().to_string(), mem.trim().parse().ok()?))
}

/// 找 cloudflared：設定裡的路徑 → PATH → winget 預設安裝位置。
pub fn find_cloudflared(configured: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = configured.filter(|p| p.is_file()) {
        return Some(p.to_path_buf());
    }
    let exe = if cfg!(windows) { "cloudflared.exe" } else { "cloudflared" };
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join(exe);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    if cfg!(windows) {
        for var in ["ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"] {
            if let Some(base) = std::env::var_os(var) {
                let base = PathBuf::from(base);
                for sub in ["cloudflared\\cloudflared.exe", "Microsoft\\WinGet\\Links\\cloudflared.exe"] {
                    let p = base.join(sub);
                    if p.is_file() {
                        return Some(p);
                    }
                }
            }
        }
    }
    None
}

/// 和 cantiva 放在同一個資料夾的其他執行檔（cantiva-mcp）。
pub fn sibling_exe(name: &str) -> Option<PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let file = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
    let p = dir.join(file);
    p.is_file().then_some(p)
}

/// 背景程序不要跳出主控台視窗。
pub fn hide_window(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}
