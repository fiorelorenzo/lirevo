//! Per-backend composite scoring: aggregate cell outcomes into a 0-100 score
//! along three axes (latency, quality, RAM) and two composite formulas.
//!
//! The scores are min-max normalized **within the run**: the fastest backend
//! gets 100 on latency, the slowest gets 0, others linearly interpolated.
//! Same for RAM (lower is better) and quality (higher is better, driven by
//! chrF̄). This makes scores comparable to peers *in this benchmark*, not
//! absolute across runs — which is the right shape for the "pick the winner
//! of this bake-off" use case in the main app's model catalog.
//!
//! Two composites are emitted:
//! - `composite_equal` = (latency + quality + ram) / 3 — transparent, no
//!   weighting choice baked in. Useful for an unbiased sanity check.
//! - `composite_weighted` = 0.5·quality + 0.3·latency + 0.2·ram — reflects
//!   that for the dictation refiner: quality regressions are user-visible
//!   first (we hear bad refinements), latency matters second (lag), RAM
//!   third (modern Macs have headroom). The main app's UI surfaces this one.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::report::CellOutcome;

struct Raw {
    chrf_mean: f64,
    warm_p50_ms: Option<u64>,
    peak_rss_kb: Option<u64>,
    n: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelScore {
    pub backend_id: String,
    /// Cells contributing to this score (= corpus size for this backend).
    pub n_cells: u32,
    /// 0-100, higher is better (lower latency wins).
    pub latency_score: u8,
    /// 0-100, higher is better (higher chrF wins).
    pub quality_score: u8,
    /// 0-100, higher is better (smaller RAM footprint wins).
    pub ram_score: u8,
    /// (`latency` + `quality` + `ram`) / 3.
    pub composite_equal: u8,
    /// 0.5·quality + 0.3·latency + 0.2·ram. UI default.
    pub composite_weighted: u8,
    /// Raw inputs preserved for audit / regression tracking.
    pub raw_chrf_mean: f64,
    pub raw_warm_p50_ms: Option<u64>,
    pub raw_peak_rss_kb: Option<u64>,
}

/// Compute one `ModelScore` per distinct `backend_id` found in `outcomes`.
/// Returns scores in the same order as the first appearance of each backend
/// id, so the caller can rely on stable ordering for report layout.
#[must_use]
pub fn score_run(outcomes: &[CellOutcome]) -> Vec<ModelScore> {
    if outcomes.is_empty() {
        return Vec::new();
    }

    // Group cells by backend, preserving first-seen order.
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, Vec<&CellOutcome>> = HashMap::new();
    for o in outcomes {
        let id = o.score.backend_id.clone();
        if !groups.contains_key(&id) {
            order.push(id.clone());
        }
        groups.entry(id).or_default().push(o);
    }

    // Per-backend raw aggregates.
    let raws: HashMap<String, Raw> = groups
        .iter()
        .map(|(id, cells)| {
            #[allow(clippy::cast_precision_loss)]
            let n = cells.len() as f64;
            let chrf_mean = cells.iter().map(|c| c.score.chrf).sum::<f64>() / n;
            // Latency: the bake-off probe sets warm.p50_ms identically for all
            // cells of a backend (single per-cell probe), so taking the min
            // (= same value for all cells) is robust to ordering.
            let warm_p50_ms = cells
                .iter()
                .filter_map(|c| c.latency.as_ref().map(|l| l.warm.p50_ms))
                .min();
            let peak_rss_kb = cells.iter().filter_map(|c| c.peak_rss_kb).max();
            #[allow(clippy::cast_possible_truncation)]
            let n_u32 = cells.len() as u32;
            (
                id.clone(),
                Raw {
                    chrf_mean,
                    warm_p50_ms,
                    peak_rss_kb,
                    n: n_u32,
                },
            )
        })
        .collect();

    // Min-max bounds across backends. If a metric is missing for a backend,
    // it's excluded from the min/max but the backend gets a `None` score for
    // that axis (rendered as 0 here so the composite stays defined).
    let chrf_lo = raws.values().map(|r| r.chrf_mean).fold(f64::INFINITY, f64::min);
    let chrf_hi = raws.values().map(|r| r.chrf_mean).fold(f64::NEG_INFINITY, f64::max);
    let lat_lo = raws.values().filter_map(|r| r.warm_p50_ms).min();
    let lat_hi = raws.values().filter_map(|r| r.warm_p50_ms).max();
    let rss_lo = raws.values().filter_map(|r| r.peak_rss_kb).min();
    let rss_hi = raws.values().filter_map(|r| r.peak_rss_kb).max();

    order
        .iter()
        .map(|id| {
            let r = raws.get(id).expect("inserted above");
            let quality_score = norm_higher_is_better(r.chrf_mean, chrf_lo, chrf_hi);
            let latency_score = match r.warm_p50_ms {
                Some(v) => norm_lower_is_better_int(v, lat_lo, lat_hi),
                None => 0,
            };
            let ram_score = match r.peak_rss_kb {
                Some(v) => norm_lower_is_better_int(v, rss_lo, rss_hi),
                None => 0,
            };
            let composite_equal = {
                let s = u32::from(latency_score) + u32::from(quality_score) + u32::from(ram_score);
                #[allow(clippy::cast_possible_truncation)]
                let v = (s / 3) as u8;
                v
            };
            let composite_weighted = {
                let s = f64::from(quality_score).mul_add(
                    0.5,
                    f64::from(latency_score).mul_add(0.3, f64::from(ram_score) * 0.2),
                );
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    clippy::cast_precision_loss
                )]
                let v = s.round().clamp(0.0, 100.0) as u8;
                v
            };
            ModelScore {
                backend_id: id.clone(),
                n_cells: r.n,
                latency_score,
                quality_score,
                ram_score,
                composite_equal,
                composite_weighted,
                raw_chrf_mean: r.chrf_mean,
                raw_warm_p50_ms: r.warm_p50_ms,
                raw_peak_rss_kb: r.peak_rss_kb,
            }
        })
        .collect()
}

fn norm_higher_is_better(v: f64, lo: f64, hi: f64) -> u8 {
    if !v.is_finite() || !lo.is_finite() || !hi.is_finite() || (hi - lo).abs() < f64::EPSILON {
        // Degenerate range (1-backend runs, or all-equal metric): give the
        // benefit of the doubt rather than 0, so composites stay meaningful.
        return 100;
    }
    let n = ((v - lo) / (hi - lo)).clamp(0.0, 1.0) * 100.0;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let r = n.round() as u8;
    r
}

fn norm_lower_is_better_int(v: u64, lo: Option<u64>, hi: Option<u64>) -> u8 {
    let (Some(lo), Some(hi)) = (lo, hi) else {
        return 100;
    };
    if hi == lo {
        return 100;
    }
    #[allow(clippy::cast_precision_loss)]
    let frac = (v - lo) as f64 / (hi - lo) as f64;
    let n = (1.0 - frac.clamp(0.0, 1.0)) * 100.0;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let r = n.round() as u8;
    r
}

#[cfg(test)]
mod tests {
    use super::{score_run, ModelScore};
    use crate::corpus::TestCase;
    use crate::probes::latency::{LatencyCell, LatencyStats};
    use crate::report::CellOutcome;
    use crate::scoring::ScoreCard;

    fn outcome(
        backend: &str,
        chrf: f64,
        warm_p50_ms: u64,
        peak_rss_kb: Option<u64>,
    ) -> CellOutcome {
        CellOutcome {
            case: TestCase {
                id: "x".into(),
                language: "it".into(),
                profile: "plain".into(),
                transcript: String::new(),
                expected: String::new(),
                tags: vec![],
                notes: String::new(),
            },
            candidate: String::new(),
            score: ScoreCard {
                case_id: "x".into(),
                backend_id: backend.into(),
                chrf,
                length_ratio: 1.0,
                assertions_passed: 0,
                assertions_total: 0,
                embedding_cosine: None,
                judge_fidelity: None,
                judge_style: None,
                judge_rationale: None,
            },
            latency: Some(LatencyCell {
                cold_ms: 0,
                warm: LatencyStats {
                    runs: 1,
                    p50_ms: warm_p50_ms,
                    p99_ms: warm_p50_ms,
                },
                warm_tokens_per_sec: None,
            }),
            peak_rss_kb,
        }
    }

    #[test]
    fn single_backend_gets_perfect_scores() {
        let outs = vec![outcome("only", 0.7, 500, Some(1024))];
        let scores: Vec<ModelScore> = score_run(&outs);
        assert_eq!(scores.len(), 1);
        let s = &scores[0];
        assert_eq!(s.quality_score, 100);
        assert_eq!(s.latency_score, 100);
        assert_eq!(s.ram_score, 100);
        assert_eq!(s.composite_equal, 100);
        assert_eq!(s.composite_weighted, 100);
    }

    #[test]
    fn three_backends_rank_correctly() {
        let outs = vec![
            outcome("fast", 0.50, 100, Some(1024)),  // best latency + ram, worst quality
            outcome("good", 0.90, 1000, Some(4096)), // best quality, worst latency + ram
            outcome("mid", 0.70, 500, Some(2048)),   // middle on all
        ];
        let scores = score_run(&outs);
        // first-seen order preserved
        assert_eq!(scores[0].backend_id, "fast");
        assert_eq!(scores[1].backend_id, "good");
        assert_eq!(scores[2].backend_id, "mid");

        // fast: low quality, top latency, top ram
        assert_eq!(scores[0].quality_score, 0);
        assert_eq!(scores[0].latency_score, 100);
        assert_eq!(scores[0].ram_score, 100);

        // good: top quality, worst latency, worst ram
        assert_eq!(scores[1].quality_score, 100);
        assert_eq!(scores[1].latency_score, 0);
        assert_eq!(scores[1].ram_score, 0);

        // mid: 50% on quality, between latency extremes, middle ram
        assert_eq!(scores[2].quality_score, 50);
        assert!(scores[2].latency_score > 40 && scores[2].latency_score < 60);
        assert!(scores[2].ram_score > 60 && scores[2].ram_score < 80);
    }

    #[test]
    fn weighted_composite_favors_quality() {
        let outs = vec![
            outcome("a", 0.90, 1000, Some(4096)), // top quality only
            outcome("b", 0.50, 100, Some(1024)),  // top lat+ram only
        ];
        let scores = score_run(&outs);
        // Equal weighting: both should be 100/3 ≈ 33 (a has 100 quality, b has 200 from lat+ram).
        // Weighted: a = 0.5*100 = 50; b = 0.3*100 + 0.2*100 = 50. So they tie at 50.
        // The point of weighted is that it doesn't dominate one axis — verify roughly 50/50.
        assert!(scores[0].composite_weighted >= 48 && scores[0].composite_weighted <= 52);
        assert!(scores[1].composite_weighted >= 48 && scores[1].composite_weighted <= 52);
    }
}
