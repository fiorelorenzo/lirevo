#![warn(clippy::pedantic)]
#![allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]

use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result};
use axum::{
    body::Bytes,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use tokio::net::UnixListener;
use tokio::signal::unix::{signal, SignalKind};
use tracing::{info, warn};

use crate::audio;
use crate::backend::{
    ChatMessage, ChatRequest, ChatRole, LlmBackendHandle, LlmError, ModelInfo, SttBackendHandle,
    SttError, SttOptions,
};
use crate::wire::{error_response, ErrorBody, Wire, WireResponse};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const BUILD_SHA: &str = match option_env!("BUILD_SHA") {
    Some(sha) => sha,
    None => "unknown",
};
pub const BACKEND_NAME: &str = "inference-core";

const MAX_BODY_BYTES: usize = 50 * 1024 * 1024; // 50 MiB

#[derive(Clone)]
pub struct AppState {
    pub started_at: Instant,
    pub stt: Option<SttBackendHandle>,
    pub llm: Option<LlmBackendHandle>,
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    version: &'static str,
    uptime_ms: u128,
    stt_ready: bool,
    llm_ready: bool,
}

#[derive(Serialize)]
struct VersionResponse {
    version: &'static str,
    build: &'static str,
    backend: &'static str,
}

async fn healthz(headers: HeaderMap, State(state): State<AppState>) -> WireResponse<HealthResponse> {
    WireResponse::ok(
        Wire::from_accept(&headers),
        HealthResponse {
            status: "ok",
            version: VERSION,
            uptime_ms: state.started_at.elapsed().as_millis(),
            stt_ready: state.stt.is_some(),
            llm_ready: state.llm.is_some(),
        },
    )
}

async fn version(headers: HeaderMap) -> WireResponse<VersionResponse> {
    WireResponse::ok(
        Wire::from_accept(&headers),
        VersionResponse {
            version: VERSION,
            build: BUILD_SHA,
            backend: BACKEND_NAME,
        },
    )
}

#[derive(Serialize)]
struct ModelsResponse {
    models: Vec<ModelInfo>,
}

async fn models(headers: HeaderMap, State(state): State<AppState>) -> WireResponse<ModelsResponse> {
    let list = match &state.stt {
        Some(b) => vec![b.model_info()],
        None => Vec::new(),
    };
    WireResponse::ok(Wire::from_accept(&headers), ModelsResponse { models: list })
}

#[derive(Debug, Deserialize, Default)]
pub struct SttQuery {
    pub language: Option<String>,
    #[serde(default)]
    pub translate: bool,
    #[serde(default)]
    pub segments: bool,
}

async fn stt(
    headers: HeaderMap,
    Query(q): Query<SttQuery>,
    State(state): State<AppState>,
    body: Bytes,
) -> Response {
    let wire = Wire::from_accept(&headers);

    // Content-Type check
    let ct_ok = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|s| s.to_ascii_lowercase().starts_with("audio/wav"));
    if !ct_ok {
        return error_response(
            wire,
            StatusCode::BAD_REQUEST,
            "bad_audio",
            "Content-Type must be audio/wav",
        )
        .into_response();
    }
    if body.len() > MAX_BODY_BYTES {
        return error_response(
            wire,
            StatusCode::PAYLOAD_TOO_LARGE,
            "payload_too_large",
            format!("body {} bytes exceeds {MAX_BODY_BYTES}", body.len()),
        )
        .into_response();
    }

    let Some(stt_handle) = state.stt.clone() else {
        return error_response(
            wire,
            StatusCode::SERVICE_UNAVAILABLE,
            "stt_unavailable",
            "model not loaded",
        )
        .into_response();
    };

    // Audio processing is CPU-bound: run on the blocking pool.
    let body_vec = body.to_vec();
    let samples = match tokio::task::spawn_blocking(move || audio::process_wav(&body_vec)).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => return stt_error_to_response(wire, &e).into_response(),
        Err(join_err) => {
            return error_response(
                wire,
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                format!("audio task panicked: {join_err}"),
            )
            .into_response();
        }
    };

    let opts = SttOptions {
        language: q.language,
        translate: q.translate,
        want_segments: q.segments,
    };
    match stt_handle.transcribe(samples, opts).await {
        Ok(transcript) => WireResponse::ok(wire, transcript).into_response(),
        Err(e) => stt_error_to_response(wire, &e).into_response(),
    }
}

fn stt_error_to_response(wire: Wire, err: &SttError) -> WireResponse<ErrorBody> {
    let (status, code) = match err {
        SttError::AudioDecode(_) => (StatusCode::BAD_REQUEST, "bad_audio"),
        SttError::AudioUnsupported(_) => (StatusCode::BAD_REQUEST, "unsupported_audio"),
        SttError::ModelNotLoaded => (StatusCode::SERVICE_UNAVAILABLE, "stt_unavailable"),
        SttError::Busy => (StatusCode::SERVICE_UNAVAILABLE, "busy"),
        SttError::Resample(_) | SttError::Whisper(_) | SttError::Internal(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
        ),
    };
    error_response(wire, status, code, err.to_string())
}

// ---------- /v1/chat ----------

const CHAT_BODY_LIMIT_BYTES: usize = 256 * 1024;
const DEFAULT_TEMPERATURE: f32 = 0.7;
const DEFAULT_MAX_TOKENS: u32 = 1024;

#[derive(Debug, Deserialize)]
struct ChatRequestBody {
    #[serde(default)]
    system: Option<String>,
    user: String,
    #[serde(default)]
    history: Vec<ChatMessage>,
    #[serde(default)]
    temperature: Option<f32>,
    #[serde(default)]
    max_tokens: Option<u32>,
    #[serde(default)]
    stop: Vec<String>,
}

async fn chat(
    headers: HeaderMap,
    State(state): State<AppState>,
    body: Bytes,
) -> Response {
    let wire = Wire::from_accept(&headers);

    if body.len() > CHAT_BODY_LIMIT_BYTES {
        return error_response(
            wire,
            StatusCode::PAYLOAD_TOO_LARGE,
            "payload_too_large",
            format!("body {} bytes exceeds {CHAT_BODY_LIMIT_BYTES}", body.len()),
        )
        .into_response();
    }

    let ct_msgpack = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|s| s.to_ascii_lowercase().starts_with("application/msgpack"));

    let parsed: Result<ChatRequestBody, String> = if ct_msgpack {
        rmp_serde::from_slice(&body).map_err(|e| format!("msgpack decode: {e}"))
    } else {
        serde_json::from_slice(&body).map_err(|e| format!("json decode: {e}"))
    };
    let req_body = match parsed {
        Ok(b) => b,
        Err(e) => {
            return error_response(wire, StatusCode::BAD_REQUEST, "bad_request", e)
                .into_response()
        }
    };

    let req = match build_chat_request(req_body) {
        Ok(r) => r,
        Err(e) => {
            return error_response(wire, StatusCode::BAD_REQUEST, "bad_request", e)
                .into_response()
        }
    };

    let Some(llm_handle) = state.llm.clone() else {
        return error_response(
            wire,
            StatusCode::SERVICE_UNAVAILABLE,
            "llm_unavailable",
            "model not loaded",
        )
        .into_response();
    };

    match llm_handle.chat(req).await {
        Ok(resp) => WireResponse::ok(wire, resp).into_response(),
        Err(e) => llm_error_to_response(wire, &e).into_response(),
    }
}

fn build_chat_request(b: ChatRequestBody) -> Result<ChatRequest, String> {
    if b.user.trim().is_empty() {
        return Err("`user` field must not be empty".to_string());
    }
    for (i, m) in b.history.iter().enumerate() {
        if matches!(m.role, ChatRole::System) {
            return Err(format!(
                "history[{i}].role = system not allowed; pass system prompt via top-level `system` field"
            ));
        }
    }
    let temperature = b.temperature.unwrap_or(DEFAULT_TEMPERATURE);
    if !(0.0..=2.0).contains(&temperature) {
        return Err(format!("temperature {temperature} not in [0.0, 2.0]"));
    }
    let max_tokens = b.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS);
    if max_tokens == 0 {
        return Err("max_tokens must be >= 1".to_string());
    }
    Ok(ChatRequest {
        system: b.system,
        user: b.user,
        history: b.history,
        temperature,
        max_tokens,
        stop: b.stop,
    })
}

fn llm_error_to_response(wire: Wire, err: &LlmError) -> WireResponse<ErrorBody> {
    let (status, code) = match err {
        LlmError::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request"),
        LlmError::ContextOverflow(_) => (StatusCode::PAYLOAD_TOO_LARGE, "context_overflow"),
        LlmError::ModelNotLoaded => (StatusCode::SERVICE_UNAVAILABLE, "llm_unavailable"),
        LlmError::Busy => (StatusCode::SERVICE_UNAVAILABLE, "busy"),
        LlmError::Llama(_) | LlmError::Internal(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
        ),
    };
    error_response(wire, status, code, err.to_string())
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/version", get(version))
        .route("/v1/models", get(models))
        .route("/v1/stt", axum::routing::post(stt))
        .route("/v1/chat", axum::routing::post(chat))
        .with_state(state)
        .layer(axum::extract::DefaultBodyLimit::max(MAX_BODY_BYTES))
}

pub async fn shutdown_signal(socket_path: PathBuf) {
    let mut sigterm = signal(SignalKind::terminate()).expect("install SIGTERM handler");
    let mut sigint = signal(SignalKind::interrupt()).expect("install SIGINT handler");
    tokio::select! {
        _ = sigterm.recv() => info!("received SIGTERM"),
        _ = sigint.recv()  => info!("received SIGINT"),
    }
    if socket_path.exists() {
        if let Err(e) = std::fs::remove_file(&socket_path) {
            warn!(?socket_path, ?e, "failed to remove socket file during shutdown");
        } else {
            info!(?socket_path, "removed socket file");
        }
    }
}

pub async fn run(
    socket_path: PathBuf,
    stt: Option<SttBackendHandle>,
    llm: Option<LlmBackendHandle>,
) -> Result<()> {
    if socket_path.exists() {
        warn!(?socket_path, "removing stale socket file");
        std::fs::remove_file(&socket_path).context("remove stale socket")?;
    }

    let listener = UnixListener::bind(&socket_path).context("bind unix listener")?;
    info!(?socket_path, "listening on unix socket");

    let state = AppState { started_at: Instant::now(), stt, llm };
    let app = build_router(state);

    let shutdown = shutdown_signal(socket_path.clone());
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await
        .context("axum serve")?;
    Ok(())
}
