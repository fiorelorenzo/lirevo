#![warn(clippy::pedantic)]
#![allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]

use clap::Parser;
use lda_eval::cli::{Cli, Command};

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "lda_eval=info,info".into()),
        )
        .init();

    let cli = Cli::parse();
    match cli.command {
        Command::Run(_) => anyhow::bail!("run: not implemented yet"),
        Command::GenCorpus(_) => anyhow::bail!("gen-corpus: not implemented yet"),
        Command::Judge(_) => anyhow::bail!("judge: not implemented yet"),
    }
}
