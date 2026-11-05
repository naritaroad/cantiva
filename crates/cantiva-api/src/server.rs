use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use axum::extract::{Query, Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use cantiva_core::model::{Consent, now};
use cantiva_engine::studio::{NewVoice, Origin};
use cantiva_engine::{EngineError, Studio};
use serde::Deserialize;

use crate::types::*;

/// GUI 實作這個 trait，讓 agent 可以請 Cantiva 直接播放音檔。
pub trait Player: Send + Sync {
    fn play(&self, path: &Path) -> Result<(), String>;
}

pub struct ServeOptions {
    pub port: u16,
    pub token: String,
    pub player: Option<Arc<dyn Player>>,
    /// 無視窗模式才給：GUI 啟動時會請無視窗模式讓出控制權。
    pub shutdown: Option<Arc<tokio::sync::Notify>>,
}

#[derive(Clone)]
struct AppState {
    studio: Arc<Studio>,
    token: Arc<str>,
    player: Option<Arc<dyn Player>>,
    shutdown: Option<Arc<tokio::sync::Notify>>,
}

/// 啟動控制 API。回傳實際綁定的位址（port 0 時由系統挑選）。
pub async fn serve(studio: Arc<Studio>, opts: ServeOptions) -> std::io::Result<SocketAddr> {
    let listener = match tokio::net::TcpListener::bind(("127.0.0.1", opts.port)).await {
        Ok(l) => l,
        // 指定的連接埠被佔用時退而求其次，實際連接埠會寫進 api-port。
        // 單一實例由 instance.lock 保證，所以走到這裡代表被別的程式佔用，不是另一個 Cantiva。
        Err(_) if opts.port != 0 => tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?,
        Err(e) => return Err(e),
    };
    let addr = listener.local_addr()?;
    std::fs::write(studio.paths().api_port_file(), addr.port().to_string())?;
    let state = AppState { studio, token: opts.token.into(), player: opts.player, shutdown: opts.shutdown };
    let app = router(state);
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!("控制 API 停止：{e}");
        }
    });
    Ok(addr)
}

fn router(state: AppState) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/engines", get(engines))
        .route("/v1/engines/load", post(load_engine))
        .route("/v1/engines/unload", post(unload_engine))
        .route("/v1/voices", get(voices).post(create_voice))
        .route("/v1/speak", post(speak))
        .route("/v1/script", post(script))
        .route("/v1/takes", get(takes))
        .route("/v1/play", post(play))
        .route("/v1/shutdown", post(shutdown))
        .layer(middleware::from_fn_with_state(state.clone(), auth))
        // 合成可能很久；請求本文上限 1 MB（文字而已，不收音檔）。
        .layer(axum::extract::DefaultBodyLimit::max(1024 * 1024))
        .with_state(state)
}

async fn auth(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let ok = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .is_some_and(|t| constant_time_eq(t.as_bytes(), state.token.as_bytes()));
    if !ok {
        return (StatusCode::UNAUTHORIZED, Json(ApiError { error: "權杖錯誤".into() })).into_response();
    }
    next.run(req).await
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// 由哪裡發出的請求。MCP server 在對外通道模式下會加上這個標頭。
fn origin(headers: &HeaderMap) -> Origin {
    match headers.get("x-cantiva-origin").and_then(|v| v.to_str().ok()) {
        Some("tunnel") => Origin::Tunnel,
        _ => Origin::LocalAgent,
    }
}

struct Failure(StatusCode, String);

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        (self.0, Json(ApiError { error: self.1 })).into_response()
    }
}

impl From<EngineError> for Failure {
    fn from(e: EngineError) -> Self {
        let code = match &e {
            EngineError::UnknownEngine(_) | EngineError::UnknownVoice(_) => StatusCode::NOT_FOUND,
            EngineError::Invalid(_) | EngineError::NotInstalled(_) => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        Failure(code, e.to_string())
    }
}

type ApiResult<T> = Result<Json<T>, Failure>;

async fn health(State(s): State<AppState>) -> Json<Health> {
    Json(Health {
        app: "Cantiva".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        loaded_engine: s.studio.loaded_engine().await,
        can_play: s.player.is_some(),
        output_dir: s.studio.output_dir().display().to_string(),
    })
}

async fn engines(State(s): State<AppState>) -> Json<EngineList> {
    Json(EngineList { engines: s.studio.engines(), loaded: s.studio.loaded_engine().await })
}

async fn load_engine(State(s): State<AppState>, headers: HeaderMap, Json(body): Json<LoadEngine>) -> ApiResult<EngineList> {
    deny_tunnel(&headers, "透過對外通道不能載入或卸載模型")?;
    s.studio.load(&body.engine).await?;
    Ok(engines(State(s)).await)
}

async fn unload_engine(State(s): State<AppState>, headers: HeaderMap) -> ApiResult<EngineList> {
    deny_tunnel(&headers, "透過對外通道不能載入或卸載模型")?;
    s.studio.unload().await;
    Ok(engines(State(s)).await)
}

fn deny_tunnel(headers: &HeaderMap, message: &str) -> Result<(), Failure> {
    if origin(headers) == Origin::Tunnel { Err(Failure(StatusCode::FORBIDDEN, message.into())) } else { Ok(()) }
}

async fn voices(State(s): State<AppState>) -> ApiResult<VoiceList> {
    Ok(Json(VoiceList { voices: s.studio.voice_choices()? }))
}

async fn create_voice(State(s): State<AppState>, headers: HeaderMap, Json(body): Json<CreateVoice>) -> ApiResult<VoiceList> {
    if origin(&headers) == Origin::Tunnel {
        return Err(Failure(StatusCode::FORBIDDEN, "透過對外通道不能建立複製聲音".into()));
    }
    let consent = Consent { basis: body.consent_basis, statement: body.consent_statement, recorded_at: now() };
    s.studio.create_voice_from(NewVoice {
        name: body.name,
        engine_id: body.engine,
        audio: body.audio_path.into(),
        transcript: body.transcript,
        language: body.reference_language,
        gpt_weights: body.gpt_weights_path.map(Into::into),
        sovits_weights: body.sovits_weights_path.map(Into::into),
        consent,
    }, origin(&headers))?;
    voices(State(s)).await
}

async fn speak(State(s): State<AppState>, headers: HeaderMap, Json(body): Json<SpeakBody>) -> ApiResult<cantiva_core::model::Take> {
    let take = s.studio.speak(body.request, origin(&headers)).await?;
    if body.play && origin(&headers) != Origin::Tunnel
        && let Some(p) = &s.player {
            let _ = p.play(Path::new(&take.path));
        }
    Ok(Json(take))
}

async fn script(State(s): State<AppState>, headers: HeaderMap, Json(body): Json<ScriptBody>) -> ApiResult<TakeList> {
    let takes = s
        .studio
        .render_script(&body.project, &body.script, body.default_voice.as_deref(), origin(&headers))
        .await?;
    Ok(Json(TakeList { takes }))
}

#[derive(Deserialize)]
struct TakesQuery {
    #[serde(default = "default_limit")]
    limit: usize,
    project: Option<String>,
}

fn default_limit() -> usize {
    20
}

async fn takes(State(s): State<AppState>, Query(q): Query<TakesQuery>) -> ApiResult<TakeList> {
    let limit = q.limit.clamp(1, 200);
    let takes = match q.project {
        Some(name) => match s.studio.with_store(|st| st.project_by_name(&name))? {
            Some(p) => s.studio.with_store(|st| st.takes(p.id, limit))?,
            None => return Err(Failure(StatusCode::NOT_FOUND, format!("找不到專案「{name}」"))),
        },
        None => s.studio.with_store(|st| st.recent_takes(limit))?,
    };
    Ok(Json(TakeList { takes }))
}

async fn play(State(s): State<AppState>, headers: HeaderMap, Json(body): Json<PlayBody>) -> Result<StatusCode, Failure> {
    if origin(&headers) == Origin::Tunnel {
        return Err(Failure(StatusCode::FORBIDDEN, "透過對外通道不能在這台電腦播放聲音".into()));
    }
    let Some(player) = &s.player else {
        return Err(Failure(StatusCode::CONFLICT, "Cantiva 目前以無視窗模式執行，不能播放。請打開 Cantiva 主程式".into()));
    };
    let take = s
        .studio
        .with_store(|st| st.recent_takes(500))?
        .into_iter()
        .find(|t| t.id.to_string() == body.take_id)
        .ok_or_else(|| Failure(StatusCode::NOT_FOUND, format!("找不到 take {}", body.take_id)))?;
    player.play(Path::new(&take.path)).map_err(|e| Failure(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn shutdown(State(s): State<AppState>) -> Result<StatusCode, Failure> {
    match &s.shutdown {
        Some(n) => {
            n.notify_one();
            Ok(StatusCode::ACCEPTED)
        }
        None => Err(Failure(StatusCode::CONFLICT, "Cantiva 主視窗開著，不能從外部關閉".into())),
    }
}
