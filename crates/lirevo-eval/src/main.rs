#![warn(clippy::pedantic)]
#![allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]

use clap::Parser;
use lirevo_eval::cli::{Cli, Command};

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "lirevo_eval=info,info".into()),
        )
        .init();

    let cli = Cli::parse();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(async move {
        match cli.command {
            Command::Run(args) => lirevo_eval::cli::run::run(args).await,
            Command::GenCorpus(args) => lirevo_eval::cli::gen_corpus::run(args).await,
            Command::Judge(args) => lirevo_eval::cli::judge::run(args).await,
            Command::Bless(args) => lirevo_eval::cli::bless::run(&args),
            Command::PrintShippedPrompt(args) => lirevo_eval::cli::print_shipped_prompt::run(&args),
            Command::BakeCell => lirevo_eval::cli::bake_cell::run().await,
        }
    })
}
