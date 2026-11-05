//! 無視窗模式：只跑 Studio 與控制 API，給 cantiva-mcp 在主程式沒開時使用。
//! 閒置 30 分鐘沒有任何合成就自動結束，釋放記憶體。

use std::time::{Duration, Instant};

use crate::runtime::Runtime;

pub fn run(rt: Runtime) -> anyhow::Result<()> {
    if rt.other_instance {
        tracing::info!("已經有 Cantiva 在執行，無視窗模式不再重複啟動");
        return Ok(());
    }
    let stop = std::sync::Arc::new(tokio::sync::Notify::new());
    rt.serve_api(None, Some(stop.clone()))?;
    let mut events = rt.studio.subscribe();
    rt.tokio.block_on(async move {
        let mut last = Instant::now();
        let idle_limit = Duration::from_secs(30 * 60);
        loop {
            tokio::select! {
                ev = events.recv() => {
                    if ev.is_ok() { last = Instant::now(); }
                }
                _ = tokio::time::sleep(Duration::from_secs(30)) => {
                    if last.elapsed() > idle_limit {
                        tracing::info!("閒置超過 30 分鐘，無視窗模式結束");
                        break;
                    }
                }
                _ = stop.notified() => {
                    tracing::info!("收到結束要求（通常是主視窗要接手），無視窗模式結束");
                    break;
                }
                _ = tokio::signal::ctrl_c() => break,
            }
        }
    });
    rt.tokio.block_on(rt.studio.unload());
    Ok(())
}
