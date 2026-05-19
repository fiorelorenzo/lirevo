//! Stub — implemented in Task 14.

use crate::cli::JudgeArgs;

// `async` kept so the dispatcher in `main.rs` can `.await` uniformly across
// subcommands; the real implementation in Task 14 will call a judge backend.
#[allow(clippy::unused_async)]
pub async fn run(_args: JudgeArgs) -> anyhow::Result<()> {
    anyhow::bail!("judge: not implemented yet (Task 14)")
}
