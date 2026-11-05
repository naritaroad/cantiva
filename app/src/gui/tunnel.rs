//! 對外通道：啟動 `cantiva-mcp --tunnel`（Streamable HTTP，連接埠由系統挑選），
//! 確認它真的在監聽後，才啟動 `cloudflared tunnel --url` 指向它。
//!
//! 安全設計：
//! - 每次開啟通道都產生新的密鑰，透過環境變數交給 cantiva-mcp，不和本機 HTTP 模式共用。
//! - cantiva-mcp 沒起來就不啟動 cloudflared，避免把別的服務（或本機權限的 MCP）開到網路上。
//! - 任一子程序結束都會回報，介面不會繼續顯示失效的網址。

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::platform::hide_window;

pub enum TunnelEvent {
    Url(String),
    /// 通道停止（任一子程序結束或啟動失敗）。
    Stopped(String),
}

pub struct Tunnel {
    mcp: Arc<Mutex<Option<Child>>>,
    cloudflared: Arc<Mutex<Option<Child>>>,
}

impl Tunnel {
    /// 啟動通道，立即返回；進度透過 `on_event` 在背景執行緒回報。
    pub fn start(
        mcp_exe: &Path,
        cloudflared: &Path,
        on_event: impl Fn(TunnelEvent) + Send + Sync + 'static,
    ) -> Result<Self, String> {
        let secret = new_secret();
        let mut mcp_cmd = Command::new(mcp_exe);
        mcp_cmd
            .args(["--tunnel", "--no-autostart", "--port", "0"])
            .env("CANTIVA_TUNNEL_SECRET", &secret)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut mcp = hide_window(&mut mcp_cmd).spawn().map_err(|e| format!("無法啟動 cantiva-mcp：{e}"))?;
        let stdout = mcp.stdout.take().expect("piped stdout");
        let mcp = Arc::new(Mutex::new(Some(mcp)));
        let cf_slot: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));
        let on_event = Arc::new(on_event);

        let (port_tx, port_rx) = std::sync::mpsc::channel::<u16>();
        {
            let on_event = on_event.clone();
            std::thread::spawn(move || {
                let mut sent = false;
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    if !sent && let Some(port) = line.strip_prefix("CANTIVA_PORT ").and_then(|p| p.trim().parse().ok()) {
                        let _ = port_tx.send(port);
                        sent = true;
                    }
                }
                on_event(TunnelEvent::Stopped("cantiva-mcp 已結束".into()));
            });
        }

        let cloudflared: PathBuf = cloudflared.to_path_buf();
        {
            let mcp = mcp.clone();
            let cf_slot = cf_slot.clone();
            let on_event = on_event.clone();
            std::thread::spawn(move || {
                let Ok(port) = port_rx.recv_timeout(Duration::from_secs(15)) else {
                    kill(&mcp);
                    on_event(TunnelEvent::Stopped("cantiva-mcp 沒有成功啟動".into()));
                    return;
                };
                let mut cf_cmd = Command::new(&cloudflared);
                cf_cmd
                    .args(["tunnel", "--no-autoupdate", "--url", &format!("http://127.0.0.1:{port}")])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::piped());
                let mut child = match hide_window(&mut cf_cmd).spawn() {
                    Ok(c) => c,
                    Err(e) => {
                        kill(&mcp);
                        on_event(TunnelEvent::Stopped(format!("無法啟動 cloudflared：{e}")));
                        return;
                    }
                };
                let stderr = child.stderr.take().expect("piped stderr");
                *cf_slot.lock().expect("lock") = Some(child);
                let mut found = false;
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    if !found && let Some(host) = find_trycloudflare(&line) {
                        found = true;
                        on_event(TunnelEvent::Url(format!("{host}/mcp/{secret}")));
                    }
                }
                // cloudflared 結束：通道一定斷了，連同 MCP 一起關掉。
                kill(&mcp);
                on_event(TunnelEvent::Stopped(
                    if found { "cloudflared 已結束，通道中斷" } else { "cloudflared 結束了，沒有取得網址" }.into(),
                ));
            });
        }
        Ok(Self { mcp, cloudflared: cf_slot })
    }

    pub fn stop(self) {
        kill(&self.cloudflared);
        kill(&self.mcp);
    }
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        kill(&self.cloudflared);
        kill(&self.mcp);
    }
}

fn kill(slot: &Arc<Mutex<Option<Child>>>) {
    if let Some(mut c) = slot.lock().expect("lock").take() {
        let _ = c.kill();
        let _ = c.wait();
    }
}

fn new_secret() -> String {
    // 40 個十六進位字元，來源是作業系統的亂數（透過 uuid v4）。
    let a = uuid::Uuid::new_v4().simple().to_string();
    let b = uuid::Uuid::new_v4().simple().to_string();
    format!("{a}{}", &b[..8])
}

/// 從 cloudflared 的記錄行找出 `https://xxxx.trycloudflare.com`。
pub fn find_trycloudflare(line: &str) -> Option<String> {
    let start = line.find("https://")?;
    let rest = &line[start..];
    let end = rest.find(|c: char| c.is_whitespace() || c == '|' || c == '"').unwrap_or(rest.len());
    let url = &rest[..end];
    url.ends_with(".trycloudflare.com").then(|| url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_quick_tunnel_url() {
        let line = "2026-11-05T03:00:00Z INF |  https://shy-river-1234.trycloudflare.com                                  |";
        assert_eq!(find_trycloudflare(line).as_deref(), Some("https://shy-river-1234.trycloudflare.com"));
        assert_eq!(find_trycloudflare("INF Visit https://www.cloudflare.com/terms"), None);
        assert_eq!(find_trycloudflare("no url here"), None);
    }

    #[test]
    fn secrets_are_long_and_unique() {
        let (a, b) = (new_secret(), new_secret());
        assert_eq!(a.len(), 40);
        assert_ne!(a, b);
    }
}
