//! 單一 Python worker 程序。
//!
//! 協定（只綁 127.0.0.1，每個請求都要帶 `X-Cantiva-Token`）：
//! - `GET  /health`      → `{"state": "loading"|"ready"|"error", "error"?, "vram_mb"?}`
//! - `GET  /info`        → `{"speakers": [...], "sample_rate": 24000}`
//! - `POST /synthesize`  ← [`SegmentRequest`]，→ `{"path", "sample_rate", "duration_sec"}`
//! - `POST /shutdown`
//!
//! 權杖透過環境變數傳給 worker，不放在命令列（命令列別的程式看得到）。

use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};

use cantiva_core::model::SegmentRequest;
use rand::RngExt;
use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};

use crate::catalog::EngineEntry;
use crate::{EngineError, Result};

#[derive(Debug, Clone, Deserialize)]
pub struct Health {
    pub state: String,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub vram_mb: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkerInfo {
    #[serde(default)]
    pub speakers: Vec<String>,
    pub sample_rate: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SegmentResult {
    pub path: String,
    pub sample_rate: u32,
    pub duration_sec: f64,
}

#[derive(Debug, Deserialize)]
struct ErrorBody {
    error: String,
}

pub struct Worker {
    engine_id: String,
    base: String,
    token: String,
    http: reqwest::Client,
    child: Child,
}

pub struct SpawnOptions {
    pub pythonpath: PathBuf,
    pub log_file: PathBuf,
    /// 等 worker 回報 ready 的時間上限（首次載入大模型可能要好幾分鐘）。
    pub load_timeout: Duration,
}

impl Worker {
    pub async fn spawn(entry: &EngineEntry, opts: SpawnOptions) -> Result<Self> {
        let engine_id = entry.manifest.id.clone();
        if !entry.is_installed() {
            return Err(EngineError::NotInstalled(engine_id));
        }
        let port = free_port().map_err(|e| start_failed(&engine_id, format!("找不到可用的連接埠：{e}")))?;
        let token: String = {
            let mut rng = rand::rng();
            (0..32).map(|_| format!("{:x}", rng.random_range(0..16u8))).collect()
        };

        let mut cmd = Command::new(entry.python());
        cmd.arg("-u")
            .arg("-m")
            .arg("cantiva_worker")
            .arg("--adapter")
            .arg(entry.adapter())
            .arg("--engine-home")
            .arg(&entry.home)
            .arg("--port")
            .arg(port.to_string())
            .env("PYTHONPATH", &opts.pythonpath)
            .env("PYTHONIOENCODING", "utf-8")
            .env("PYTHONUTF8", "1")
            .env("CANTIVA_WORKER_TOKEN", &token)
            // worker 會監看這個 PID：Cantiva 當掉或被強制結束時自己退出，釋放顯示記憶體。
            .env("CANTIVA_PARENT_PID", std::process::id().to_string())
            // 模型權重都放在引擎自己的資料夾，不污染使用者的全域快取。
            .env("HF_HOME", entry.home.join("hf"))
            .env("MODELSCOPE_CACHE", entry.home.join("modelscope"))
            .current_dir(&entry.home)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        {
            // CREATE_NO_WINDOW：不要跳出黑色主控台視窗。
            cmd.creation_flags(0x0800_0000);
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| start_failed(&engine_id, format!("無法執行 {}：{e}", entry.python().display())))?;
        pipe_logs(&mut child, opts.log_file).await;

        let worker = Self {
            engine_id,
            base: format!("http://127.0.0.1:{port}"),
            token,
            http: reqwest::Client::builder().no_proxy().build().expect("reqwest client"),
            child,
        };
        worker.wait_ready(opts.load_timeout).await
    }

    async fn wait_ready(mut self, timeout: Duration) -> Result<Self> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Ok(Some(status)) = self.child.try_wait() {
                return Err(start_failed(
                    &self.engine_id,
                    format!("worker 提早結束（{status}）。詳細原因請看 logs 資料夾裡的 {}.log", self.engine_id),
                ));
            }
            match self.health().await {
                Ok(h) if h.state == "ready" => return Ok(self),
                Ok(h) if h.state == "error" => {
                    let message = h.error.unwrap_or_else(|| "未知錯誤".into());
                    let _ = self.child.kill().await;
                    return Err(start_failed(&self.engine_id, message));
                }
                _ => {}
            }
            if Instant::now() > deadline {
                let _ = self.child.kill().await;
                return Err(start_failed(&self.engine_id, format!("超過 {} 秒還沒載入完成", timeout.as_secs())));
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    pub fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    pub async fn health(&self) -> Result<Health> {
        self.get("/health").await
    }

    pub async fn info(&self) -> Result<WorkerInfo> {
        self.get("/info").await
    }

    pub async fn synthesize(&self, req: &SegmentRequest) -> Result<SegmentResult> {
        let resp = self
            .http
            .post(format!("{}/synthesize", self.base))
            .header("X-Cantiva-Token", &self.token)
            .json(req)
            .timeout(Duration::from_secs(600))
            .send()
            .await
            .map_err(|e| self.synth_err(e.to_string()))?;
        let status = resp.status();
        if status.is_success() {
            resp.json().await.map_err(|e| self.synth_err(e.to_string()))
        } else {
            let msg = resp
                .json::<ErrorBody>()
                .await
                .map(|b| b.error)
                .unwrap_or_else(|_| format!("HTTP {status}"));
            Err(self.synth_err(msg))
        }
    }

    pub async fn shutdown(mut self) {
        let _ = self
            .http
            .post(format!("{}/shutdown", self.base))
            .header("X-Cantiva-Token", &self.token)
            .timeout(Duration::from_secs(3))
            .send()
            .await;
        // 給 worker 幾秒釋放顯示記憶體，還不結束就強制終止。
        if tokio::time::timeout(Duration::from_secs(8), self.child.wait()).await.is_err() {
            let _ = self.child.kill().await;
        }
    }

    async fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        let resp = self
            .http
            .get(format!("{}{path}", self.base))
            .header("X-Cantiva-Token", &self.token)
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .map_err(|e| self.synth_err(e.to_string()))?;
        resp.json().await.map_err(|e| self.synth_err(e.to_string()))
    }

    fn synth_err(&self, message: String) -> EngineError {
        EngineError::Synthesis { engine: self.engine_id.clone(), message }
    }
}

fn start_failed(engine: &str, message: String) -> EngineError {
    EngineError::StartFailed { engine: engine.to_string(), message }
}

fn free_port() -> std::io::Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}

/// 把 worker 的 stdout／stderr 附加到引擎的記錄檔。
async fn pipe_logs(child: &mut Child, log_file: PathBuf) {
    if let Some(dir) = log_file.parent() {
        let _ = tokio::fs::create_dir_all(dir).await;
    }
    let file = tokio::fs::OpenOptions::new().create(true).append(true).open(&log_file).await;
    let Ok(file) = file else { return };
    let file = std::sync::Arc::new(tokio::sync::Mutex::new(file));
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    for stream in [stdout.map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
                   stderr.map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>)]
        .into_iter()
        .flatten()
    {
        let file = file.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stream).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let mut f = file.lock().await;
                let _ = f.write_all(line.as_bytes()).await;
                let _ = f.write_all(b"\n").await;
            }
        });
    }
}
