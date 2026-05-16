//! Prototype dictation binary: push-to-talk → record → STT → cleanup → inject.

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "lda-prototype", version, about = "push-to-talk dictation prototype for macOS")]
struct Cli {
    /// Override hotkey. Values: right-option | left-option | right-command | fn | f5
    #[arg(long)]
    hotkey: Option<String>,

    /// Override sidecar UNIX socket path.
    #[arg(long)]
    socket: Option<PathBuf>,

    /// Language hint for cleanup. Default "auto".
    #[arg(long, default_value = "auto")]
    language: String,

    /// Skip AX injection; always use pasteboard.
    #[arg(long)]
    force_pasteboard: bool,

    /// Pasteboard paste→restore delay (ms).
    #[arg(long)]
    paste_delay_ms: Option<u64>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("LDA_PROTOTYPE_LOG_LEVEL").unwrap_or_else(|_| "info".to_string()),
        )
        .with_writer(std::io::stderr)
        .init();

    tracing::info!(?cli, "lda-prototype starting (T16 scaffold)");
    eprintln!("lda-prototype scaffold (T17-T20 land the real logic)");
    let _ = cli;
    Ok(())
}
