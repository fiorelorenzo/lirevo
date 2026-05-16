//! Prototype dictation binary: push-to-talk → record → STT → cleanup → inject.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{anyhow, Context, Result};
use clap::Parser;
use http_body_util::{BodyExt, Empty};
use hyper::body::Bytes;
use hyper::Request;
use hyper_util::client::legacy::Client as HClient;
use hyper_util::rt::TokioExecutor;
use hyperlocal::{UnixConnector, Uri};
use os_integration::{check_accessibility, prompt_accessibility, PermissionStatus};
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

    eprintln!("lda-prototype preflight ok (socket: {})", socket.display());
    eprintln!("hotkey + record loop land in T18-T19.");
    ExitCode::from(0)
}
