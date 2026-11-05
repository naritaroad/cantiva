//! 控制 API 的客戶端，MCP server 用它操作 Cantiva。

use std::time::Duration;

use cantiva_core::model::Take;
use cantiva_core::paths::AppPaths;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::types::*;

#[derive(Clone)]
pub struct ApiClient {
    base: String,
    token: String,
    http: reqwest::Client,
    origin: Option<&'static str>,
}

impl ApiClient {
    pub fn new(port: u16, token: String) -> Self {
        Self {
            base: format!("http://127.0.0.1:{port}"),
            token,
            http: reqwest::Client::builder().no_proxy().build().expect("reqwest client"),
            origin: None,
        }
    }

    pub fn from_paths(paths: &AppPaths) -> Option<Self> {
        Some(Self::new(crate::read_port(paths)?, crate::read_token(paths)?))
    }

    /// 對外通道模式：之後的每個請求都標記為來自通道，伺服器會套用比較嚴格的權限。
    pub fn as_tunnel(mut self) -> Self {
        self.origin = Some("tunnel");
        self
    }

    pub async fn health(&self) -> Result<Health, String> {
        self.get("/v1/health", Duration::from_secs(3)).await
    }

    pub async fn engines(&self) -> Result<EngineList, String> {
        self.get("/v1/engines", Duration::from_secs(10)).await
    }

    pub async fn load_engine(&self, engine: &str) -> Result<EngineList, String> {
        self.post("/v1/engines/load", &LoadEngine { engine: engine.into() }, Duration::from_secs(900)).await
    }

    pub async fn unload_engine(&self) -> Result<EngineList, String> {
        self.post("/v1/engines/unload", &serde_json::json!({}), Duration::from_secs(30)).await
    }

    pub async fn voices(&self) -> Result<VoiceList, String> {
        self.get("/v1/voices", Duration::from_secs(10)).await
    }

    pub async fn create_voice(&self, body: &CreateVoice) -> Result<VoiceList, String> {
        self.post("/v1/voices", body, Duration::from_secs(30)).await
    }

    pub async fn speak(&self, body: &SpeakBody) -> Result<Take, String> {
        // 首次合成可能包含載入模型，給足時間。
        self.post("/v1/speak", body, Duration::from_secs(1800)).await
    }

    pub async fn script(&self, body: &ScriptBody) -> Result<TakeList, String> {
        self.post("/v1/script", body, Duration::from_secs(7200)).await
    }

    pub async fn takes(&self, limit: usize, project: Option<&str>) -> Result<TakeList, String> {
        let mut path = format!("/v1/takes?limit={limit}");
        if let Some(p) = project {
            path.push_str("&project=");
            path.push_str(&url_encode(p));
        }
        self.get(&path, Duration::from_secs(10)).await
    }

    pub async fn play(&self, take_id: &str) -> Result<(), String> {
        let resp = self
            .request(reqwest::Method::POST, "/v1/play", Duration::from_secs(10))
            .json(&PlayBody { take_id: take_id.into() })
            .send()
            .await
            .map_err(connect_error)?;
        if resp.status().is_success() { Ok(()) } else { Err(error_text(resp).await) }
    }

    /// 請無視窗模式的 Cantiva 結束（GUI 啟動時接手用）。
    pub async fn shutdown(&self) -> Result<(), String> {
        let resp = self
            .request(reqwest::Method::POST, "/v1/shutdown", Duration::from_secs(5))
            .send()
            .await
            .map_err(connect_error)?;
        if resp.status().is_success() { Ok(()) } else { Err(error_text(resp).await) }
    }

    fn request(&self, method: reqwest::Method, path: &str, timeout: Duration) -> reqwest::RequestBuilder {
        let mut rb = self
            .http
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(&self.token)
            .timeout(timeout);
        if let Some(o) = self.origin {
            rb = rb.header("X-Cantiva-Origin", o);
        }
        rb
    }

    async fn get<T: DeserializeOwned>(&self, path: &str, timeout: Duration) -> Result<T, String> {
        let resp = self.request(reqwest::Method::GET, path, timeout).send().await.map_err(connect_error)?;
        decode(resp).await
    }

    async fn post<B: Serialize, T: DeserializeOwned>(&self, path: &str, body: &B, timeout: Duration) -> Result<T, String> {
        let resp = self.request(reqwest::Method::POST, path, timeout).json(body).send().await.map_err(connect_error)?;
        decode(resp).await
    }
}

fn connect_error(e: reqwest::Error) -> String {
    if e.is_connect() {
        "連不到 Cantiva。請打開 Cantiva，或確認它沒有被防毒軟體擋住".into()
    } else if e.is_timeout() {
        "Cantiva 回應逾時（可能正在載入大模型），請稍後再試".into()
    } else {
        e.to_string()
    }
}

async fn decode<T: DeserializeOwned>(resp: reqwest::Response) -> Result<T, String> {
    if resp.status().is_success() {
        resp.json().await.map_err(|e| format!("回應格式錯誤：{e}"))
    } else {
        Err(error_text(resp).await)
    }
}

async fn error_text(resp: reqwest::Response) -> String {
    let status = resp.status();
    match resp.json::<ApiError>().await {
        Ok(e) => e.error,
        Err(_) => format!("HTTP {status}"),
    }
}

fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}
