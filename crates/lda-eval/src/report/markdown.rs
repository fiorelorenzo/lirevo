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
    fn oneline_handles_newlines_and_pipes() {
        use super::oneline;
        let s = "first|second\nthird";
        let out = oneline(s);
        assert!(!out.contains('\n'));
        assert!(out.contains("\\|"));
    }
}
