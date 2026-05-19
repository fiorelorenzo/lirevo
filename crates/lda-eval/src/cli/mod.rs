pub mod gen_corpus;
pub mod judge;
pub mod run;

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "lda-eval",
    version,
    about = "Refiner-stage model evaluation harness"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Run a bake-off: candidate backends × corpus → report.
    Run(RunArgs),
    /// Expand a seed corpus using an oracle backend (review before commit).
    GenCorpus(GenCorpusArgs),
    /// Re-score an existing report with an LLM-as-judge backend.
    Judge(JudgeArgs),
}

#[derive(clap::Args, Debug)]
pub struct RunArgs {
    /// Path to corpus JSONL.
    #[arg(long)]
    pub corpus: std::path::PathBuf,
    /// Path to profiles TOML.
    #[arg(long)]
    pub profiles: std::path::PathBuf,
    /// Comma-separated backend specs, e.g. `gguf:qwen3-4b@/path/m.gguf,claude-p:sonnet`.
    #[arg(long, value_delimiter = ',')]
    pub backends: Vec<String>,
    /// Optional judge backend spec.
    #[arg(long)]
    pub judge: Option<String>,
    /// Output markdown report path. Sidecar JSON written alongside with `.json` extension.
    #[arg(long)]
    pub out: std::path::PathBuf,
    /// Enable semantic embedding cosine scoring. Requires `[scoring.embedding]`
    /// in the profiles TOML and a working network connection (or pre-warmed cache)
    /// on first run to fetch the ONNX model.
    #[arg(long, default_value_t = false)]
    pub embed: bool,
    /// Comma-separated list of backend IDs for which `/no_think` should be
    /// appended to the system prompt. Use for Qwen3 / Qwen3.5 hybrid models
    /// that default to thinking-on and would otherwise emit
    /// `<think>...</think>` blocks polluting the refiner output. Do NOT
    /// include non-Qwen IDs here — Gemma / Llama don't understand the
    /// directive and would treat it as literal text in their prompt.
    /// Example: `--no-think-for qwen3-1.7b,qwen3.5-0.8b,qwen3.5-2b`.
    #[arg(long, value_delimiter = ',')]
    pub no_think_for: Vec<String>,
}

#[derive(clap::Args, Debug)]
pub struct GenCorpusArgs {
    #[arg(long)]
    pub oracle: String,
    #[arg(long)]
    pub seeds: std::path::PathBuf,
    #[arg(long)]
    pub profiles: std::path::PathBuf,
    /// Target cases per (profile, language) cell after expansion.
    #[arg(long, default_value_t = 4)]
    pub target_per_cell: u32,
    #[arg(long)]
    pub out: std::path::PathBuf,
}

#[derive(clap::Args, Debug)]
pub struct JudgeArgs {
    /// Existing report JSON sidecar.
    #[arg(long)]
    pub report: std::path::PathBuf,
    #[arg(long)]
    pub judge: String,
    #[arg(long)]
    pub out: std::path::PathBuf,
}
