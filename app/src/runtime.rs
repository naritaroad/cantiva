//! 啟動共用的執行環境：記錄檔、tokio、Studio 與本機控制 API。

use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use cantiva_api::{Player, ServeOptions};
use cantiva_core::paths::AppPaths;
use cantiva_engine::catalog::Catalog;
use cantiva_engine::{Studio, StudioConfig};

pub struct Runtime {
    pub tokio: tokio::runtime::Runtime,
    pub studio: Arc<Studio>,
    pub paths: AppPaths,
    /// 已經有另一個 Cantiva 在跑（例如先前以無視窗模式啟動）。
    pub other_instance: bool,
    /// 單一實例鎖：持有期間其他 Cantiva 不會啟動控制 API 或載入模型。
    instance_lock: Option<std::fs::File>,
}

impl Runtime {
    pub fn start(headless: bool) -> anyhow::Result<Self> {
        let paths = AppPaths::discover().context("找不到使用者資料夾")?;
        paths.ensure()?;
        init_logging(&paths, headless);
        let tokio = tokio::runtime::Builder::new_multi_thread().enable_all().worker_threads(4).build()?;
        let engines_dir = Catalog::locate_engines_dir()
            .context("找不到 engines 資料夾。請把它放在 cantiva.exe 旁邊，或設定環境變數 CANTIVA_ENGINES_DIR")?;
        let instance_lock = try_instance_lock(&paths);
        let other_instance = instance_lock.is_none();
        let studio = {
            let _guard = tokio.enter();
            let s = Studio::open(StudioConfig {
                paths: paths.clone(),
                engines_dir,
                // 首次載入 1.7B 模型（含編譯 CUDA kernel）在慢的硬碟上可能要好幾分鐘。
                load_timeout: Duration::from_secs(15 * 60),
            })?;
            s.spawn_idle_unloader();
            s
        };
        tracing::info!("Cantiva {} 啟動，資料夾 {}", env!("CARGO_PKG_VERSION"), paths.root().display());
        Ok(Self { tokio, studio, paths, other_instance, instance_lock })
    }

    /// 在 `wait` 時間內反覆嘗試取得單一實例鎖（例如等無視窗模式結束）。
    pub fn acquire_instance_lock(&mut self, wait: Duration) -> bool {
        let deadline = std::time::Instant::now() + wait;
        loop {
            if let Some(f) = try_instance_lock(&self.paths) {
                self.instance_lock = Some(f);
                self.other_instance = false;
                return true;
            }
            if std::time::Instant::now() > deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// 啟動控制 API。player 為 None 表示無視窗模式（不能播放）。
    pub fn serve_api(
        &self,
        player: Option<Arc<dyn Player>>,
        shutdown: Option<Arc<tokio::sync::Notify>>,
    ) -> anyhow::Result<u16> {
        let token = cantiva_api::load_or_create_token(&self.paths)?;
        let port = self.studio.settings().mcp.api_port;
        let addr = self
            .tokio
            .block_on(cantiva_api::serve(self.studio.clone(), ServeOptions { port, token, player, shutdown }))?;
        tracing::info!("控制 API：http://{addr}");
        Ok(addr.port())
    }
}

/// 作業系統層級的檔案鎖：程式當掉時會自動釋放，不會留下過期的鎖。
fn try_instance_lock(paths: &AppPaths) -> Option<std::fs::File> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(paths.root().join("instance.lock"))
        .ok()?;
    file.try_lock().ok()?;
    Some(file)
}

fn init_logging(paths: &AppPaths, headless: bool) {
    let filter = tracing_subscriber::EnvFilter::try_from_env("CANTIVA_LOG").unwrap_or_else(|_| "info".into());
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(paths.logs().join(if headless { "cantiva-headless.log" } else { "cantiva.log" }));
    let builder = tracing_subscriber::fmt().with_env_filter(filter).with_ansi(false);
    match file {
        Ok(f) => builder.with_writer(std::sync::Mutex::new(f)).init(),
        Err(_) => builder.with_writer(std::io::stderr).init(),
    }
}
