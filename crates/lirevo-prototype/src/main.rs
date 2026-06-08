//! Prototype dictation binary: push-to-talk → record → STT → cleanup → inject.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{anyhow, Context, Result};
use audio_capture::{AudioError, Recorder, RecorderConfig};
use clap::Parser;
use http_body_util::{BodyExt, Empty, Full};
use hyper::body::Bytes;
use hyper::Request;
use hyper_util::client::legacy::Client as HClient;
use hyper_util::rt::TokioExecutor;
use hyperlocal::{UnixConnector, Uri};
use os_integration::{
    check_accessibility, prompt_accessibility, Hotkey, HotkeyEvent, HotkeyListener,
    InjectionMethod, Injector, PermissionStatus,
};
use serde::Deserialize;
use serde_json::json;

#[derive(Parser, Debug)]
#[command(name = "lirevo-prototype", version, about = "push-to-talk dictation prototype for macOS")]
struct Cli {
    #[arg(long)]
    hotkey: Option<String>,
    #[arg(long)]
    socket: Option<PathBuf>,
    #[arg(long, default_value = "auto")]
    language: String,
    #[arg(long)]
    paste_delay_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct HealthBody {
    status: String,
    version: String,
    stt_ready: bool,
    llm_ready: bool,
}

#[derive(Debug, Deserialize, Default)]
struct SttBody {
    text: String,
    // present in sidecar response; captured for future use
    #[serde(default)]
    #[allow(dead_code)]
    language: String,
    #[serde(default)]
    #[allow(dead_code)]
    duration_ms: u32,
}

#[derive(Debug, Deserialize, Default)]
struct ChatBody {
    text: String,
    // present in sidecar response; captured for future use
    #[serde(default)]
    #[allow(dead_code)]
    model: String,
}

fn resolve_socket(arg: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(p) = arg {
        return Ok(p);
    }
    if let Ok(s) = std::env::var("SIDECAR_SOCKET_PATH") {
        return Ok(PathBuf::from(s));
    }
    // Per-user app data dir: macOS ~/Library/Application Support,
    // Linux $XDG_DATA_HOME or ~/.local/share, Windows %APPDATA%.
    let base = dirs::data_dir().ok_or_else(|| anyhow!("could not resolve user data dir"))?;
    Ok(base.join("app").join("sidecar.sock"))
}

async fn check_sidecar_health(socket: &Path) -> Result<HealthBody> {
    let connector = UnixConnector;
    let client: HClient<UnixConnector, Empty<Bytes>> =
        HClient::builder(TokioExecutor::new()).build(connector);
    let uri: hyper::Uri = Uri::new(socket, "/healthz").into();
    let req = Request::builder()
        .uri(uri)
        .header("accept", "application/json")
        .body(Empty::<Bytes>::new())?;
    let resp = client.request(req).await.context("connect to sidecar")?;
    let (parts, body) = resp.into_parts();
    if !parts.status.is_success() {
        return Err(anyhow!("sidecar /healthz returned {}", parts.status));
    }
    let bytes = body.collect().await?.to_bytes();
    let h: HealthBody = serde_json::from_slice(&bytes).context("parse healthz body")?;
    Ok(h)
}

fn run_preflight(cli: &Cli) -> Result<PathBuf, ExitCode> {
    match check_accessibility() {
        PermissionStatus::Granted => {
            tracing::info!("accessibility: granted");
        }
        PermissionStatus::Denied | PermissionStatus::NotDetermined => {
            tracing::warn!("accessibility not granted; showing prompt");
            let _ = prompt_accessibility();
            eprintln!();
            eprintln!("==============================================================");
            eprintln!(" lirevo-prototype needs Accessibility permission to:");
            eprintln!("   - listen for the push-to-talk hotkey (CGEventTap)");
            eprintln!("   - inject text into the focused app (AXUIElement)");
            eprintln!();
            eprintln!(" Grant access in:");
            eprintln!("   System Settings → Privacy & Security → Accessibility");
            eprintln!();
            eprintln!(" Then re-run `lirevo-prototype`.");
            eprintln!("==============================================================");
            return Err(ExitCode::from(2));
        }
    }

    let socket = resolve_socket(cli.socket.clone()).map_err(|e| {
        eprintln!("socket resolve error: {e}");
        ExitCode::from(3)
    })?;
    if !socket.exists() {
        eprintln!(
            "sidecar socket not found: {}\nStart the sidecar (`just dev` or `cargo run -p inference-core`).",
            socket.display()
        );
        return Err(ExitCode::from(3));
    }

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio rt for preflight");
    let health = match rt.block_on(check_sidecar_health(&socket)) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("sidecar /healthz failed: {e:#}");
            return Err(ExitCode::from(3));
        }
    };
    if !health.stt_ready {
        eprintln!("sidecar reachable but stt_ready=false; set SIDECAR_STT_MODEL_NAME (or SIDECAR_STT_BACKEND=stub) and restart it");
        return Err(ExitCode::from(4));
    }
    if !health.llm_ready {
        eprintln!("sidecar reachable but llm_ready=false; set SIDECAR_LLM_MODEL_PATH and restart it");
        return Err(ExitCode::from(4));
    }
    tracing::info!(version = %health.version, status = %health.status, "sidecar healthy");

    Ok(socket)
}

fn parse_hotkey_arg(s: &str) -> Option<Hotkey> {
    match s {
        "right-option" | "RightOption" => Some(Hotkey::RightOption),
        "left-option" | "LeftOption" => Some(Hotkey::LeftOption),
        "right-command" | "RightCommand" => Some(Hotkey::RightCommand),
        "fn" | "Fn" => Some(Hotkey::Fn),
        "f5" | "F5" => Some(Hotkey::F5),
        _ => None,
    }
}

async fn http_post_json<T: for<'de> Deserialize<'de>>(
    socket: &Path,
    path: &str,
    content_type: &str,
    body_bytes: Vec<u8>,
) -> Result<T> {
    let connector = UnixConnector;
    let client: HClient<UnixConnector, Full<Bytes>> =
        HClient::builder(TokioExecutor::new()).build(connector);
    let uri: hyper::Uri = Uri::new(socket, path).into();
    let req = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", content_type)
        .header("accept", "application/json")
        .body(Full::new(Bytes::from(body_bytes)))?;
    let resp = client.request(req).await.context("request failed")?;
    let (parts, body) = resp.into_parts();
    let bytes = body.collect().await?.to_bytes();
    if !parts.status.is_success() {
        return Err(anyhow!(
            "HTTP {} body={}",
            parts.status,
            String::from_utf8_lossy(&bytes)
        ));
    }
    let parsed: T = serde_json::from_slice(&bytes).context("deserialize response")?;
    Ok(parsed)
}

/// Run the full dictation pipeline for one recording.
///
/// Failure modes & degradation:
/// - STT fails → return error, nothing typed, user re-dictates.
/// - LLM cleanup fails → inject raw STT (better than nothing).
/// - Inject fails → log error, user sees nothing typed.
///
/// All HTTP errors are logged with full context. Timing for each stage
/// (stt, clean, inject) is logged at info level for performance review.
async fn post_and_inject(
    rec: audio_capture::Recording,
    language: String,
    injector: Arc<Injector>,
    socket: PathBuf,
) -> Result<()> {
    let t0 = Instant::now();
    let wav = audio_capture::samples_to_wav(&rec.samples);

    let stt: SttBody = http_post_json(&socket, "/v1/stt", "audio/wav", wav).await?;
    let t_stt = t0.elapsed();

    let cleaned_req = json!({
        "system": lirevo_prompts::build_clean_system_prompt(&language),
        "user": stt.text,
        "temperature": 0.2,
        "max_tokens": 2048,
    });
    let chat: ChatBody = match http_post_json(
        &socket,
        "/v1/chat",
        "application/json",
        serde_json::to_vec(&cleaned_req)?,
    )
    .await
    {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error = %e, "LLM cleanup failed; injecting raw STT");
            ChatBody { text: stt.text.clone(), model: "fallback-raw".into() }
        }
    };
    let t_clean = t0.elapsed() - t_stt;

    let injector_for_blocking = injector.clone();
    let text_for_inject = chat.text.clone();
    let method: InjectionMethod = tokio::task::spawn_blocking(move || {
        injector_for_blocking.inject(&text_for_inject)
    })
    .await
    .context("inject task join")?
    .context("inject failed")?;
    let t_inject = t0.elapsed() - t_stt - t_clean;

    tracing::info!(
        duration_ms = rec.duration_ms,
        text_len = chat.text.len(),
        method = ?method,
        t_stt_ms = t_stt.as_millis() as u64,
        t_clean_ms = t_clean.as_millis() as u64,
        t_inject_ms = t_inject.as_millis() as u64,
        "dictation complete"
    );

    Ok(())
}

async fn run_hotkey_loop(cli: Cli, socket: PathBuf) -> ExitCode {
    let hotkey = Hotkey::from_env();
    let hotkey = match cli.hotkey.as_deref() {
        Some(s) => parse_hotkey_arg(s).unwrap_or(hotkey),
        None => hotkey,
    };

    let (listener, mut rx) = match HotkeyListener::install(hotkey) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("failed to install hotkey: {e}");
            return ExitCode::from(6);
        }
    };

    let mut recorder = match Recorder::new(RecorderConfig::default()) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("failed to build recorder: {e}");
            listener.shutdown();
            return ExitCode::from(5);
        }
    };

    let injector = Arc::new(Injector::new());

    eprintln!("lirevo-prototype ready. Hold {hotkey:?} to dictate. Ctrl+C to quit.");

    let mut shutdown = std::pin::pin!(tokio::signal::ctrl_c());

    loop {
        tokio::select! {
            _ = shutdown.as_mut() => {
                tracing::info!("received Ctrl+C, shutting down");
                listener.shutdown();
                return ExitCode::from(0);
            }
            maybe_event = rx.recv() => {
                match maybe_event {
                    Some(HotkeyEvent::Down) => {
                        match recorder.start() {
                            Ok(()) => tracing::info!("REC start"),
                            Err(e) => tracing::warn!(error = %e, "recorder start failed"),
                        }
                    }
                    Some(HotkeyEvent::Up) => {
                        match recorder.stop() {
                            Ok(rec) => {
                                let lang = cli.language.clone();
                                let inj = injector.clone();
                                let sock = socket.clone();
                                tokio::spawn(async move {
                                    if let Err(e) = post_and_inject(rec, lang, inj, sock).await {
                                        tracing::error!(error = %e, "dictation pipeline failed");
                                    }
                                });
                            }
                            Err(AudioError::NotRecording) => {}
                            Err(e) => tracing::warn!(error = %e, "recorder stop failed"),
                        }
                    }
                    None => {
                        tracing::error!("hotkey channel closed; exiting");
                        return ExitCode::from(1);
                    }
                }
            }
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("LIREVO_PROTOTYPE_LOG_LEVEL").unwrap_or_else(|_| "info".to_string()),
        )
        .with_writer(std::io::stderr)
        .init();

    if let Some(delay) = cli.paste_delay_ms {
        std::env::set_var("SIDECAR_INJECT_PASTE_DELAY_MS", delay.to_string());
    }

    let socket = match run_preflight(&cli) {
        Ok(s) => s,
        Err(code) => return code,
    };

    run_hotkey_loop(cli, socket).await
}
