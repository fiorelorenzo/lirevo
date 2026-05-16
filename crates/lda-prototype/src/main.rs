//! Prototype dictation binary: push-to-talk → record → STT → cleanup → inject.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{anyhow, Context, Result};
use audio_capture::{AudioError, Recorder, RecorderConfig};
use clap::Parser;
use http_body_util::{BodyExt, Empty};
use hyper::body::Bytes;
use hyper::Request;
use hyper_util::client::legacy::Client as HClient;
use hyper_util::rt::TokioExecutor;
use hyperlocal::{UnixConnector, Uri};
use os_integration::{check_accessibility, prompt_accessibility, Hotkey, HotkeyEvent, HotkeyListener, PermissionStatus};
use serde::Deserialize;

#[derive(Parser, Debug)]
#[command(name = "lda-prototype", version, about = "push-to-talk dictation prototype for macOS")]
struct Cli {
    #[arg(long)]
    hotkey: Option<String>,
    #[arg(long)]
    socket: Option<PathBuf>,
    #[arg(long, default_value = "auto")]
    language: String,
    #[arg(long)]
    force_pasteboard: bool,
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

fn resolve_socket(arg: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(p) = arg {
        return Ok(p);
    }
    if let Ok(s) = std::env::var("SIDECAR_SOCKET_PATH") {
        return Ok(PathBuf::from(s));
    }
    let home = std::env::var("HOME").map_err(|_| anyhow!("$HOME not set"))?;
    Ok(PathBuf::from(format!(
        "{home}/Library/Application Support/app/sidecar.sock"
    )))
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
            eprintln!(" lda-prototype needs Accessibility permission to:");
            eprintln!("   - listen for the push-to-talk hotkey (CGEventTap)");
            eprintln!("   - inject text into the focused app (AXUIElement)");
            eprintln!();
            eprintln!(" Grant access in:");
            eprintln!("   System Settings → Privacy & Security → Accessibility");
            eprintln!();
            eprintln!(" Then re-run `lda-prototype`.");
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
        eprintln!("sidecar reachable but stt_ready=false; set SIDECAR_WHISPER_MODEL_PATH and restart it");
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

async fn run_hotkey_loop(cli: Cli, _socket: PathBuf) -> ExitCode {
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

    eprintln!("lda-prototype ready. Hold {hotkey:?} to dictate. Ctrl+C to quit.");

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
                                tracing::info!(
                                    duration_ms = rec.duration_ms,
                                    device = %rec.device_label,
                                    "REC stop (pipeline in T19)"
                                );
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
            std::env::var("LDA_PROTOTYPE_LOG_LEVEL").unwrap_or_else(|_| "info".to_string()),
        )
        .with_writer(std::io::stderr)
        .init();

    let socket = match run_preflight(&cli) {
        Ok(s) => s,
        Err(code) => return code,
    };

    run_hotkey_loop(cli, socket).await
}
