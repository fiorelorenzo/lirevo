use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use hyper::body::Bytes;
use hyper::Request;
use http_body_util::{BodyExt, Empty, Full};
use hyperlocal::{UnixConnector, Uri};
use serde::{Deserialize, Serialize};

#[derive(Parser, Debug)]
#[command(name = "lda-cli", version, about = "client for the Lirevo inference sidecar")]
struct Cli {
    #[arg(long, global = true)]
    socket: Option<PathBuf>,

    #[arg(long, global = true)]
    msgpack: bool,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    Health,
    Version,
    Models,
    Stt {
        file: PathBuf,
        #[arg(long)]
        language: Option<String>,
        #[arg(long)]
        translate: bool,
        #[arg(long)]
        segments: bool,
        #[arg(long)]
        json: bool,
    },
    Chat {
        #[arg(long)]
        user: String,
        #[arg(long)]
        system: Option<String>,
        #[arg(long, default_value_t = 0.7_f32)]
        temperature: f32,
        #[arg(long, default_value_t = 1024_u32)]
        max_tokens: u32,
        #[arg(long)]
        stop: Vec<String>,
        #[arg(long)]
        json: bool,
    },
    Clean {
        /// Raw text to clean. Use `-` or pipe via stdin to read from stdin.
        text: Option<String>,
        #[arg(long, default_value = "auto")]
        language: String,
        #[arg(long, default_value_t = 0.2_f32)]
        temperature: f32,
        #[arg(long, default_value_t = 2048_u32)]
        max_tokens: u32,
    },
}

#[derive(Debug, Deserialize)]
struct HealthBody {
    status: String,
    version: String,
    uptime_ms: u128,
    stt_ready: bool,
}

#[derive(Debug, Deserialize)]
struct VersionBody {
    version: String,
    build: String,
    backend: String,
}

#[derive(Debug, Deserialize)]
struct ModelEntry {
    id: String,
    kind: String,
    backend: String,
    path: String,
    coreml: bool,
    loaded: bool,
}

#[derive(Debug, Deserialize)]
struct ModelsBody {
    models: Vec<ModelEntry>,
}

#[derive(Debug, Deserialize, Serialize)]
struct SttSegment {
    start_ms: u32,
    end_ms: u32,
    text: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct TokenUsageBody {
    prompt: u32,
    completion: u32,
}

#[derive(Debug, Deserialize, Serialize)]
struct ChatBodyResponse {
    text: String,
    model: String,
    stopped_by: String,
    tokens: TokenUsageBody,
}

#[derive(Debug, Deserialize)]
struct SttBody {
    text: String,
    language: String,
    duration_ms: u32,
    processing_ms: u32,
    model: String,
    backend: String,
    #[serde(default)]
    segments: Option<Vec<SttSegment>>,
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

fn accept_header(msgpack: bool) -> &'static str {
    if msgpack { "application/msgpack" } else { "application/json" }
}

async fn unix_get_bytes(socket: &Path, path: &str, accept: &str) -> Result<(hyper::StatusCode, Vec<u8>)> {
    let connector = UnixConnector;
    let client: hyper_util::client::legacy::Client<UnixConnector, Empty<Bytes>> =
        hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
            .build(connector);
    let uri: hyper::Uri = Uri::new(socket, path).into();
    let req = Request::builder()
        .uri(uri)
        .header("accept", accept)
        .body(Empty::<Bytes>::new())?;
    let resp = client.request(req).await.context("request failed")?;
    let (parts, body) = resp.into_parts();
    let bytes = body.collect().await.context("body collect failed")?.to_bytes().to_vec();
    Ok((parts.status, bytes))
}

fn decode_body<T: for<'de> Deserialize<'de>>(bytes: &[u8], msgpack: bool) -> Result<T> {
    if msgpack {
        rmp_serde::from_slice(bytes).context("msgpack decode failed")
    } else {
        serde_json::from_slice(bytes).context("json decode failed")
    }
}

async fn cmd_health(socket: &Path, msgpack: bool) -> Result<i32> {
    let (status, bytes) = unix_get_bytes(socket, "/healthz", accept_header(msgpack)).await?;
    if !status.is_success() {
        eprintln!("{} {}", status, String::from_utf8_lossy(&bytes));
        return Ok(if status.is_client_error() { 3 } else { 4 });
    }
    let h: HealthBody = decode_body(&bytes, msgpack)?;
    println!(
        "status={}  version={}  uptime_ms={}  stt_ready={}",
        h.status, h.version, h.uptime_ms, h.stt_ready
    );
    Ok(0)
}

async fn cmd_version(socket: &Path, msgpack: bool) -> Result<i32> {
    let (status, bytes) = unix_get_bytes(socket, "/version", accept_header(msgpack)).await?;
    if !status.is_success() {
        eprintln!("{} {}", status, String::from_utf8_lossy(&bytes));
        return Ok(if status.is_client_error() { 3 } else { 4 });
    }
    let v: VersionBody = decode_body(&bytes, msgpack)?;
    println!("version={}  build={}  backend={}", v.version, v.build, v.backend);
    Ok(0)
}

async fn cmd_models(socket: &Path, msgpack: bool) -> Result<i32> {
    let (status, bytes) = unix_get_bytes(socket, "/v1/models", accept_header(msgpack)).await?;
    if !status.is_success() {
        eprintln!("{} {}", status, String::from_utf8_lossy(&bytes));
        return Ok(if status.is_client_error() { 3 } else { 4 });
    }
    let body: ModelsBody = decode_body(&bytes, msgpack)?;
    if body.models.is_empty() {
        println!("(no models loaded)");
        return Ok(0);
    }
    println!("{:<20} {:<8} {:<12} coreml loaded path", "id", "kind", "backend");
    for m in &body.models {
        println!(
            "{:<20} {:<8} {:<12} {:<6} {:<6} {}",
            m.id, m.kind, m.backend, m.coreml, m.loaded, m.path
        );
    }
    Ok(0)
}

#[allow(clippy::too_many_arguments)]
async fn cmd_stt(
    socket: &Path,
    msgpack: bool,
    file: PathBuf,
    language: Option<String>,
    translate: bool,
    segments: bool,
    print_json: bool,
) -> Result<i32> {
    let wav = tokio::fs::read(&file)
        .await
        .with_context(|| format!("read {}", file.display()))?;
    if wav.len() < 44 {
        return Err(anyhow!("input is not a WAV (too short)"));
    }

    let mut qs = Vec::new();
    if let Some(lang) = language {
        qs.push(format!("language={lang}"));
    }
    if translate {
        qs.push("translate=true".to_string());
    }
    if segments {
        qs.push("segments=true".to_string());
    }
    let q = if qs.is_empty() { String::new() } else { format!("?{}", qs.join("&")) };
    let endpoint = format!("/v1/stt{q}");

    let connector = UnixConnector;
    let client: hyper_util::client::legacy::Client<UnixConnector, Full<Bytes>> =
        hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
            .build(connector);
    let uri: hyper::Uri = Uri::new(socket, &endpoint).into();
    let req = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "audio/wav")
        .header("accept", accept_header(msgpack))
        .body(Full::new(Bytes::from(wav)))?;
    let resp = client.request(req).await.context("request failed")?;
    let (parts, body) = resp.into_parts();
    let bytes = body.collect().await.context("body collect failed")?.to_bytes().to_vec();

    if !parts.status.is_success() {
        eprintln!("{} {}", parts.status, String::from_utf8_lossy(&bytes));
        return Ok(if parts.status.is_client_error() { 3 } else { 4 });
    }
    let parsed: SttBody = decode_body(&bytes, msgpack)?;
    if print_json {
        let v = serde_json::to_value(serde_json::json!({
            "text": parsed.text,
            "language": parsed.language,
            "duration_ms": parsed.duration_ms,
            "processing_ms": parsed.processing_ms,
            "model": parsed.model,
            "backend": parsed.backend,
            "segments": parsed.segments,
        }))?;
        println!("{}", serde_json::to_string_pretty(&v)?);
    } else {
        let rtf = if parsed.duration_ms == 0 {
            0.0
        } else {
            f64::from(parsed.processing_ms) / f64::from(parsed.duration_ms)
        };
        eprintln!(
            "[{}] {} ({}) {}ms audio, {}ms processing (rtf {:.2}x)",
            parsed.backend, parsed.model, parsed.language, parsed.duration_ms, parsed.processing_ms, rtf
        );
        println!("{}", parsed.text);
    }
    Ok(0)
}

#[allow(clippy::too_many_arguments)]
async fn cmd_chat(
    socket: &Path,
    msgpack: bool,
    user: String,
    system: Option<String>,
    temperature: f32,
    max_tokens: u32,
    stop: Vec<String>,
    print_json: bool,
) -> Result<i32> {
    let mut req = serde_json::Map::new();
    req.insert("user".to_string(), serde_json::Value::String(user));
    if let Some(s) = system {
        req.insert("system".to_string(), serde_json::Value::String(s));
    }
    req.insert(
        "temperature".to_string(),
        serde_json::Value::from(temperature),
    );
    req.insert(
        "max_tokens".to_string(),
        serde_json::Value::from(max_tokens),
    );
    if !stop.is_empty() {
        req.insert("stop".to_string(), serde_json::Value::from(stop));
    }
    let body_bytes = serde_json::to_vec(&serde_json::Value::Object(req))?;

    let connector = UnixConnector;
    let client: hyper_util::client::legacy::Client<UnixConnector, Full<Bytes>> =
        hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
            .build(connector);
    let uri: hyper::Uri = Uri::new(socket, "/v1/chat").into();
    let req = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .header("accept", accept_header(msgpack))
        .body(Full::new(Bytes::from(body_bytes)))?;
    let resp = client.request(req).await.context("request failed")?;
    let (parts, body) = resp.into_parts();
    let bytes = body
        .collect()
        .await
        .context("body collect failed")?
        .to_bytes()
        .to_vec();

    if !parts.status.is_success() {
        eprintln!("{} {}", parts.status, String::from_utf8_lossy(&bytes));
        return Ok(if parts.status.is_client_error() { 3 } else { 4 });
    }
    let parsed: ChatBodyResponse = decode_body(&bytes, msgpack)?;
    if print_json {
        println!("{}", serde_json::to_string_pretty(&parsed)?);
    } else {
        eprintln!(
            "[llama] {} {}+{} tokens, stopped={}",
            parsed.model, parsed.tokens.prompt, parsed.tokens.completion, parsed.stopped_by
        );
        println!("{}", parsed.text);
    }
    Ok(0)
}

async fn cmd_clean(
    socket: &Path,
    msgpack: bool,
    text: Option<String>,
    language: String,
    temperature: f32,
    max_tokens: u32,
) -> Result<i32> {
    let input: String = match text.as_deref() {
        Some("-") | None => {
            use std::io::Read;
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .context("read stdin")?;
            if buf.trim().is_empty() {
                eprintln!(
                    "error: no input. Pass text as argument, or pipe via stdin: `lda-cli clean -` or `... | lda-cli clean`"
                );
                return Ok(5);
            }
            buf
        }
        Some(s) => s.to_string(),
    };

    let system = lda_prompts::build_clean_system_prompt(&language);

    let mut req = serde_json::Map::new();
    req.insert("user".to_string(), serde_json::Value::String(input));
    req.insert("system".to_string(), serde_json::Value::String(system));
    req.insert(
        "temperature".to_string(),
        serde_json::Value::from(temperature),
    );
    req.insert(
        "max_tokens".to_string(),
        serde_json::Value::from(max_tokens),
    );
    let body_bytes = serde_json::to_vec(&serde_json::Value::Object(req))?;

    let connector = UnixConnector;
    let client: hyper_util::client::legacy::Client<UnixConnector, Full<Bytes>> =
        hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
            .build(connector);
    let uri: hyper::Uri = Uri::new(socket, "/v1/chat").into();
    let req = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .header("accept", accept_header(msgpack))
        .body(Full::new(Bytes::from(body_bytes)))?;
    let resp = client.request(req).await.context("request failed")?;
    let (parts, body) = resp.into_parts();
    let bytes = body
        .collect()
        .await
        .context("body collect failed")?
        .to_bytes()
        .to_vec();

    if !parts.status.is_success() {
        eprintln!("{} {}", parts.status, String::from_utf8_lossy(&bytes));
        return Ok(if parts.status.is_client_error() { 3 } else { 4 });
    }
    let parsed: ChatBodyResponse = decode_body(&bytes, msgpack)?;
    println!("{}", parsed.text);
    Ok(0)
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("LDA_CLI_LOG_LEVEL").unwrap_or_else(|_| "warn".to_string()),
        )
        .with_writer(std::io::stderr)
        .init();

    let socket = match resolve_socket(cli.socket) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("failed to resolve socket: {e}");
            return std::process::ExitCode::from(2);
        }
    };
    if !socket.exists() {
        eprintln!("socket not found: {}", socket.display());
        return std::process::ExitCode::from(2);
    }

    let code = match cli.cmd {
        Cmd::Health => cmd_health(&socket, cli.msgpack).await,
        Cmd::Version => cmd_version(&socket, cli.msgpack).await,
        Cmd::Models => cmd_models(&socket, cli.msgpack).await,
        Cmd::Stt { file, language, translate, segments, json } =>
            cmd_stt(&socket, cli.msgpack, file, language, translate, segments, json).await,
        Cmd::Chat { user, system, temperature, max_tokens, stop, json } =>
            cmd_chat(&socket, cli.msgpack, user, system, temperature, max_tokens, stop, json).await,
        Cmd::Clean { text, language, temperature, max_tokens } =>
            cmd_clean(&socket, cli.msgpack, text, language, temperature, max_tokens).await,
    };
    match code {
        Ok(c) => std::process::ExitCode::from(c as u8),
        Err(e) => {
            eprintln!("error: {e:?}");
            std::process::ExitCode::from(4)
        }
    }
}
