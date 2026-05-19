//! Stub — implemented in Task 13.

use crate::cli::GenCorpusArgs;

// `async` kept so the dispatcher in `main.rs` can `.await` uniformly across
// subcommands; the real implementation in Task 13 will spawn oracle backends.
#[allow(clippy::unused_async)]
pub async fn run(_args: GenCorpusArgs) -> anyhow::Result<()> {
    anyhow::bail!("gen-corpus: not implemented yet (Task 13)")
}
