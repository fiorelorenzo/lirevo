//! Latency probe: N runs per cell, separated into cold (run 1) and warm
//! (runs 2..N, with system prompt KV cached when the backend supports it).

use crate::backend::{EvalBackend, GenerateReq};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LatencyStats {
    pub runs: usize,
    pub p50_ms: u64,
    pub p99_ms: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LatencyCell {
    pub cold_ms: u64,
    pub warm: LatencyStats,
    pub warm_tokens_per_sec: Option<f64>,
}

/// Probe a backend with `runs` generations of the same request.
///
/// Run 1 is treated as "cold" — the backend's KV cache (if any) is empty
/// and the system prompt is unseen. Before the warm runs we ask the
/// backend to pre-cache the system prompt (best-effort; backends without
/// local state return `Unsupported` and we ignore that). Runs 2..N are
/// summarized as `LatencyStats` with p50 / p99 / mean tokens-per-sec.
pub async fn probe_cell(
    backend: &mut dyn EvalBackend,
    base: GenerateReq,
    runs: usize,
) -> anyhow::Result<LatencyCell> {
    if runs < 2 {
        anyhow::bail!("need at least 2 runs (1 cold + N-1 warm)");
    }
    let cold = backend.generate(base.clone()).await?;
    let _ = backend.warm_system_prompt(&base.system_prompt).await; // best-effort

    let mut warm_samples = Vec::with_capacity(runs - 1);
    let mut total_completion_tokens: u64 = 0;
    let mut total_warm_ms: u64 = 0;
    for _ in 1..runs {
        let out = backend.generate(base.clone()).await?;
        warm_samples.push(out.latency_ms);
        total_warm_ms = total_warm_ms.saturating_add(out.latency_ms);
        total_completion_tokens =
            total_completion_tokens.saturating_add(u64::from(out.completion_tokens));
    }
    let warm = stats_from(warm_samples);
    let tps = if total_warm_ms == 0 {
        None
    } else {
        // Use f64 throughout; precision loss is negligible for ms/token counts.
        #[allow(clippy::cast_precision_loss)]
        let tps = (total_completion_tokens as f64) * 1000.0 / total_warm_ms as f64;
        Some(tps)
    };
    Ok(LatencyCell {
        cold_ms: cold.latency_ms,
        warm,
        warm_tokens_per_sec: tps,
    })
}

#[must_use]
pub fn stats_from(mut samples: Vec<u64>) -> LatencyStats {
    samples.sort_unstable();
    let n = samples.len();
    let p50 = samples[n / 2];
    // Use a saturating ceiling-1 index for p99 so we always land inside
    // the slice even for n in {1, 2}.
    let p99_idx = {
        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss
        )]
        let i = (n as f64 * 0.99).ceil() as usize;
        i.min(n.saturating_sub(1))
    };
    let p99 = samples[p99_idx];
    LatencyStats {
        runs: n,
        p50_ms: p50,
        p99_ms: p99,
    }
}

#[cfg(test)]
mod tests {
    use super::stats_from;

    #[test]
    fn stats_p50_and_p99_from_sorted() {
        let s = stats_from(vec![10, 20, 30, 40, 100]);
        assert_eq!(s.p50_ms, 30);
        assert_eq!(s.p99_ms, 100);
        assert_eq!(s.runs, 5);
    }

    #[test]
    fn stats_single_sample() {
        let s = stats_from(vec![42]);
        assert_eq!(s.p50_ms, 42);
        assert_eq!(s.p99_ms, 42);
    }

    #[test]
    fn stats_two_samples() {
        // ensure p99 indexing doesn't panic with n=2
        let s = stats_from(vec![10, 20]);
        assert_eq!(s.runs, 2);
        // p50 with n=2 → samples[1] (right-of-center) per the current convention
        assert_eq!(s.p50_ms, 20);
    }
}
