use std::fmt::Write;

use crate::report::{group_by_backend, ReportData};

#[must_use]
pub fn render(data: &ReportData) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "# Refiner bake-off — {}", data.run_id);
    let _ = writeln!(s);
    let _ = writeln!(s, "## Setup");
    let _ = writeln!(s, "- corpus: `{}`", data.corpus_path);
    let _ = writeln!(s, "- profiles: `{}`", data.profiles_path);
    let _ = writeln!(s, "- host: {}", data.host);
    let _ = writeln!(s, "- backends:");
    for b in &data.backends {
        let _ = writeln!(s, "  - `{}` (id={}, kind={})", b.spec, b.id, b.kind);
    }
    if let Some(j) = &data.judge {
        let _ = writeln!(s, "- judge: `{}`", j.spec);
    }
    let _ = writeln!(s);
    let _ = writeln!(s, "## Summary");
    let _ = writeln!(
        s,
        "| backend | chrF̄ | cos̄ | judge fid̄ | judge stȳ | RSS peak | warm p50 | warm tps | assert ok |"
    );
    let _ = writeln!(s, "|---|---|---|---|---|---|---|---|---|");

    let groups = group_by_backend(data);
    // Stable backend ordering = order of declaration.
    for desc in &data.backends {
        let Some(outs) = groups.get(&desc.id) else {
            continue;
        };
        let n = u32::try_from(outs.len().max(1)).unwrap_or(u32::MAX);
        let chrf_sum: f64 = outs.iter().map(|o| o.score.chrf).sum();
        let chrf_avg = chrf_sum / f64::from(n);
        let cos_avg = avg_opt(outs.iter().map(|o| o.score.embedding_cosine));
        let fid_avg = avg_opt_u8(outs.iter().map(|o| o.score.judge_fidelity));
        let sty_avg = avg_opt_u8(outs.iter().map(|o| o.score.judge_style));
        let rss = outs.iter().filter_map(|o| o.peak_rss_kb).max();
        let warm_p50 = outs
            .iter()
            .filter_map(|o| o.latency.as_ref().map(|l| l.warm.p50_ms))
            .min();
        let warm_tps = avg_opt(
            outs.iter()
                .filter_map(|o| o.latency.as_ref().and_then(|l| l.warm_tokens_per_sec))
                .map(Some),
        );
        let pass: u32 = outs.iter().map(|o| o.score.assertions_passed).sum();
        let tot: u32 = outs.iter().map(|o| o.score.assertions_total).sum();
        let _ = writeln!(
            s,
            "| {id} | {chrf_avg:.2} | {} | {} | {} | {} | {} | {} | {pass}/{tot} |",
            cos_avg.map_or_else(|| "—".into(), |v| format!("{v:.2}")),
            fid_avg.map_or_else(|| "—".into(), |v| format!("{v:.1}")),
            sty_avg.map_or_else(|| "—".into(), |v| format!("{v:.1}")),
            rss.map_or_else(|| "—".into(), format_kb),
            warm_p50.map_or_else(|| "—".into(), |v| format!("{v} ms")),
            warm_tps.map_or_else(|| "—".into(), |v| format!("{v:.0}")),
            id = desc.id,
        );
    }
    render_scores_section(&mut s, data);

    let _ = writeln!(s);
    let _ = writeln!(s, "## Worst 10 cases by chrF (per backend)");
    for desc in &data.backends {
        let Some(outs_ref) = groups.get(&desc.id) else {
            continue;
        };
        let mut outs: Vec<&_> = outs_ref.clone();
        outs.sort_by(|a, b| {
            a.score
                .chrf
                .partial_cmp(&b.score.chrf)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let _ = writeln!(s, "### backend: {}", desc.id);
        let _ = writeln!(
            s,
            "| id | lang | profile | chrF | transcript | expected | candidate |"
        );
        let _ = writeln!(s, "|---|---|---|---|---|---|---|");
        for o in outs.iter().take(10) {
            let _ = writeln!(
                s,
                "| {id} | {lang} | {profile} | {chrf:.2} | {t} | {e} | {c} |",
                id = o.case.id,
                lang = o.case.language,
                profile = o.case.profile,
                chrf = o.score.chrf,
                t = oneline(&o.case.transcript),
                e = oneline(&o.case.expected),
                c = oneline(&o.candidate),
            );
        }
    }
    s
}

fn render_scores_section(s: &mut String, data: &ReportData) {
    if data.model_scores.is_empty() {
        return;
    }
    let _ = writeln!(s);
    let _ = writeln!(s, "## Scores (0-100, higher is better)");
    let _ = writeln!(
        s,
        "| backend | quality | latency | RAM | composite (equal) | composite (weighted) |"
    );
    let _ = writeln!(s, "|---|---|---|---|---|---|");
    let by_id: std::collections::HashMap<&str, &crate::scoring::composite::ModelScore> = data
        .model_scores
        .iter()
        .map(|m| (m.backend_id.as_str(), m))
        .collect();
    // Winner shares the helper with `lirevo-eval bless` — same tiebreaker,
    // same quality floor — so the ⭐ and the catalog's `recommended`
    // flag never disagree.
    let winner_id =
        crate::scoring::composite::pick_winner(&data.model_scores).map(|m| m.backend_id.as_str());
    for desc in &data.backends {
        let Some(ms) = by_id.get(desc.id.as_str()) else {
            continue;
        };
        // Winner row is bolded and tagged "(recommended)" in plain text — no
        // emoji per project convention. The trailing-text marker also makes
        // the row easy to grep / diff in CI.
        let is_winner = Some(desc.id.as_str()) == winner_id;
        let label = if is_winner {
            format!("**{}** (recommended)", desc.id)
        } else {
            desc.id.clone()
        };
        let _ = writeln!(
            s,
            "| {label} | {q} | {l} | {r} | {ce} | {cw} |",
            q = ms.quality_score,
            l = ms.latency_score,
            r = ms.ram_score,
            ce = ms.composite_equal,
            cw = ms.composite_weighted,
        );
    }
}

fn avg_opt<I>(it: I) -> Option<f64>
where
    I: Iterator<Item = Option<f64>>,
{
    let mut sum = 0.0_f64;
    let mut n: u32 = 0;
    for v in it.flatten() {
        sum += v;
        n = n.saturating_add(1);
    }
    if n == 0 {
        None
    } else {
        Some(sum / f64::from(n))
    }
}

fn avg_opt_u8<I>(it: I) -> Option<f64>
where
    I: Iterator<Item = Option<u8>>,
{
    avg_opt(it.map(|x| x.map(f64::from)))
}

#[allow(clippy::cast_precision_loss)] // KB counts << 2^53, lossless
fn format_kb(kb: u64) -> String {
    if kb >= 1024 * 1024 {
        format!("{:.1} GB", kb as f64 / (1024.0 * 1024.0))
    } else if kb >= 1024 {
        format!("{:.0} MB", kb as f64 / 1024.0)
    } else {
        format!("{kb} KB")
    }
}

pub(crate) fn oneline(s: &str) -> String {
    s.replace('\n', " ⏎ ").replace('|', "\\|")
}

#[cfg(test)]
mod tests {
    use super::render;
    use crate::corpus::TestCase;
    use crate::probes::latency::{LatencyCell, LatencyStats};
    use crate::report::{BackendDescriptor, CellOutcome, ReportData};
    use crate::scoring::ScoreCard;

    fn fixture() -> ReportData {
        ReportData {
            run_id: "test-run".into(),
            host: "test-host".into(),
            corpus_path: "data/corpus/v1.jsonl".into(),
            profiles_path: "data/profiles/v1.toml".into(),
            backends: vec![BackendDescriptor {
                spec: "gguf:demo@/tmp/m.gguf".into(),
                id: "demo".into(),
                kind: "Gguf".into(),
            }],
            judge: None,
            outcomes: vec![CellOutcome {
                case: TestCase {
                    id: "it-plain-001".into(),
                    language: "it".into(),
                    profile: "plain".into(),
                    transcript: "ciao".into(),
                    expected: "Ciao.".into(),
                    tags: vec![],
                    notes: String::new(),
                },
                candidate: "Ciao.".into(),
                score: ScoreCard {
                    case_id: "it-plain-001".into(),
                    backend_id: "demo".into(),
                    chrf: 0.92,
                    length_ratio: 1.0,
                    assertions_passed: 1,
                    assertions_total: 1,
                    embedding_cosine: Some(0.97),
                    judge_fidelity: None,
                    judge_style: None,
                    judge_rationale: None,
                },
                latency: Some(LatencyCell {
                    cold_ms: 500,
                    warm: LatencyStats {
                        runs: 4,
                        p50_ms: 200,
                        p99_ms: 220,
                    },
                    warm_tokens_per_sec: Some(50.0),
                }),
                peak_rss_kb: Some(720 * 1024),
            }],
            model_scores: Vec::new(),
        }
    }

    #[test]
    fn renders_summary_table() {
        let md = render(&fixture());
        assert!(md.contains("# Refiner bake-off"));
        assert!(md.contains("## Summary"));
        assert!(md.contains("demo"));
        assert!(md.contains("chrF"));
    }

    #[test]
    fn renders_worst_cases() {
        let md = render(&fixture());
        assert!(md.contains("Worst") || md.contains("worst"));
        assert!(md.contains("it-plain-001"));
    }

    #[test]
    fn scores_star_picks_quality_winner_on_weighted_tie() {
        // Mirrors bless's tiebreaker so the markdown ⭐ and the catalog's
        // `recommended` flag never disagree. See the matching test in
        // cli::bless::tests.
        use crate::scoring::composite::ModelScore;
        let mut data = fixture();
        data.backends = vec![
            BackendDescriptor {
                spec: "x".into(),
                id: "quality_wins".into(),
                kind: "Gguf".into(),
            },
            BackendDescriptor {
                spec: "y".into(),
                id: "speed_wins".into(),
                kind: "Gguf".into(),
            },
        ];
        data.model_scores = vec![
            ModelScore {
                backend_id: "quality_wins".into(),
                n_cells: 1,
                latency_score: 0,
                quality_score: 100,
                ram_score: 0,
                composite_equal: 33,
                composite_weighted: 50,
                raw_chrf_mean: 0.7,
                raw_warm_p50_ms: None,
                raw_peak_rss_kb: None,
            },
            ModelScore {
                backend_id: "speed_wins".into(),
                n_cells: 1,
                latency_score: 100,
                quality_score: 0,
                ram_score: 100,
                composite_equal: 66,
                composite_weighted: 50,
                raw_chrf_mean: 0.5,
                raw_warm_p50_ms: None,
                raw_peak_rss_kb: None,
            },
        ];
        let md = render(&data);
        // Winner row in the scores table is the one tagged "(recommended)".
        // Match the table row specifically (starts with `|`, not the backend
        // descriptor bullet list at the top of the report).
        let q_line = md
            .lines()
            .find(|l| l.starts_with("| **quality_wins**"))
            .expect("quality table row present");
        let s_line = md
            .lines()
            .find(|l| l.starts_with("| speed_wins"))
            .expect("speed table row present");
        assert!(
            q_line.contains("(recommended)"),
            "expected (recommended) tag on quality_wins: {q_line}"
        );
        assert!(
            !s_line.contains("(recommended)"),
            "speed_wins should NOT be tagged (recommended): {s_line}"
        );
    }

    #[test]
    fn scores_star_skips_models_below_chrf_floor() {
        // Same shape as bless's `degenerate_quality_does_not_get_recommended`.
        // If this test and the bless one ever disagree, the markdown render
        // is using a different policy than the catalog — fix the shared
        // helper, not these tests.
        use crate::scoring::composite::ModelScore;
        let mut data = fixture();
        data.backends = vec![
            BackendDescriptor {
                spec: "x".into(),
                id: "broken_fast".into(),
                kind: "Gguf".into(),
            },
            BackendDescriptor {
                spec: "y".into(),
                id: "decent".into(),
                kind: "Gguf".into(),
            },
        ];
        data.model_scores = vec![
            ModelScore {
                backend_id: "broken_fast".into(),
                n_cells: 1,
                latency_score: 100,
                quality_score: 0,
                ram_score: 100,
                composite_equal: 66,
                composite_weighted: 50,
                raw_chrf_mean: 0.22,
                raw_warm_p50_ms: None,
                raw_peak_rss_kb: None,
            },
            ModelScore {
                backend_id: "decent".into(),
                n_cells: 1,
                latency_score: 0,
                quality_score: 100,
                ram_score: 0,
                composite_equal: 33,
                composite_weighted: 50,
                raw_chrf_mean: 0.60,
                raw_warm_p50_ms: None,
                raw_peak_rss_kb: None,
            },
        ];
        let md = render(&data);
        let broken = md
            .lines()
            .find(|l| l.starts_with("| broken_fast"))
            .expect("broken_fast row present");
        let decent = md
            .lines()
            .find(|l| l.starts_with("| **decent**"))
            .expect("decent winner row present");
        assert!(
            !broken.contains("(recommended)"),
            "model below chrF floor should not be tagged recommended: {broken}"
        );
        assert!(
            decent.contains("(recommended)"),
            "eligible peer should pick up the recommended tag: {decent}"
        );
    }

    #[test]
    fn oneline_handles_newlines_and_pipes() {
        use super::oneline;
        let s = "first|second\nthird";
        let out = oneline(s);
        assert!(!out.contains('\n'));
        assert!(out.contains("\\|"));
    }
}
