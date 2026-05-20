//! `lda-eval bless` — promote per-backend scores from a report into the
//! committed model catalog (`crates/inference-core/data/model_catalog.json`
//! by default).
//!
//! Mapping rule: each `model_scores[].backend_id` must match an `llm[].id` in
//! the target catalog. If the bake-off ran with backend ids that don't match
//! the catalog, the command bails with a clear list — fix the bake-off config
//! rather than papering over with a flag.
//!
//! Idempotency: re-running with the same report produces the same catalog
//! bytes (modulo `last_run.ts_unix`, which always reflects the source report).

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};

use crate::cli::BlessArgs;
use crate::report::ReportData;
use crate::scoring::composite::ModelScore;
use inference_core::catalog::{self, Catalog, LastRun, ModelScores};

pub fn run(args: &BlessArgs) -> Result<()> {
    let report_text = std::fs::read_to_string(&args.report)
        .with_context(|| format!("read report {}", args.report.display()))?;
    let report: ReportData = serde_json::from_str(&report_text)
        .with_context(|| format!("parse report {}", args.report.display()))?;

    let scores = if report.model_scores.is_empty() {
        // Older reports (or hand-built fixtures) may not have scores written
        // in. Compute them on the fly from the cell outcomes — this matches
        // what `write_pair` would have done.
        crate::scoring::composite::score_run(&report.outcomes)
    } else {
        report.model_scores.clone()
    };
    if scores.is_empty() {
        anyhow::bail!("report has no model_scores and no outcomes — nothing to bless");
    }

    let catalog_path = resolve_catalog_path(args.catalog.as_deref())?;
    let catalog_text = std::fs::read_to_string(&catalog_path)
        .with_context(|| format!("read catalog {}", catalog_path.display()))?;
    let mut catalog: Catalog = catalog::parse(&catalog_text)
        .with_context(|| format!("parse catalog {}", catalog_path.display()))?;

    apply_scores(&mut catalog, &scores, &report)?;

    let written = serde_json::to_string_pretty(&catalog)
        .context("serialize updated catalog")?;
    std::fs::write(&catalog_path, format!("{written}\n"))
        .with_context(|| format!("write catalog {}", catalog_path.display()))?;

    println!(
        "blessed {} backend(s) into {}",
        scores.len(),
        catalog_path.display()
    );
    Ok(())
}

fn resolve_catalog_path(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(p) = explicit {
        return Ok(p.to_path_buf());
    }
    // Default: assume we're being run from the workspace root.
    let p = PathBuf::from("crates/inference-core/data/model_catalog.json");
    if p.exists() {
        return Ok(p);
    }
    anyhow::bail!(
        "couldn't locate model_catalog.json — pass --catalog or run from workspace root \
         (looked for {})",
        p.display()
    )
}

fn apply_scores(
    catalog: &mut Catalog,
    scores: &[ModelScore],
    report: &ReportData,
) -> Result<()> {
    let unknown: Vec<&str> = scores
        .iter()
        .filter(|s| !catalog.llm.iter().any(|e| e.id == s.backend_id))
        .map(|s| s.backend_id.as_str())
        .collect();
    if !unknown.is_empty() {
        let llm_ids: Vec<&str> = catalog.llm.iter().map(|e| e.id.as_str()).collect();
        return Err(anyhow!(
            "report backend ids not in catalog: [{}]\n\
             catalog llm ids: [{}]\n\
             hint: align your bake-off backend specs with catalog ids \
             (e.g. `gguf:qwen3-4b-instruct-2507-q4@/path/to/file.gguf`)",
            unknown.join(", "),
            llm_ids.join(", "),
        ));
    }

    // Winner = highest composite_weighted across all scored backends.
    // Ties broken by quality, then by backend_id for determinism.
    let winner_id = scores
        .iter()
        .max_by(|a, b| {
            a.composite_weighted
                .cmp(&b.composite_weighted)
                .then(a.quality_score.cmp(&b.quality_score))
                .then(b.backend_id.cmp(&a.backend_id))
        })
        .map(|m| m.backend_id.clone());

    for s in scores {
        let entry = catalog
            .llm
            .iter_mut()
            .find(|e| e.id == s.backend_id)
            .expect("checked above");
        entry.scores = Some(ModelScores {
            quality: s.quality_score,
            latency: s.latency_score,
            ram: s.ram_score,
            composite_equal: s.composite_equal,
            composite_weighted: s.composite_weighted,
            raw_chrf_mean: s.raw_chrf_mean,
            raw_warm_p50_ms: s.raw_warm_p50_ms,
            raw_peak_rss_kb: s.raw_peak_rss_kb,
            n_cells: s.n_cells,
        });
    }

    // Clear `recommended` on every LLM, then set on the winner. Includes
    // entries that didn't appear in this bake-off — staleness is worse than
    // a temporarily-missing badge.
    for e in &mut catalog.llm {
        e.recommended = Some(&e.id) == winner_id.as_ref();
    }

    catalog.last_run = Some(LastRun {
        run_id: report.run_id.clone(),
        host: report.host.clone(),
        corpus_path: report.corpus_path.clone(),
        profiles_path: report.profiles_path.clone(),
        ts_unix: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::apply_scores;
    use crate::report::{BackendDescriptor, ReportData};
    use crate::scoring::composite::ModelScore;
    use inference_core::catalog::{Catalog, LlmEntry};

    fn empty_report() -> ReportData {
        ReportData {
            run_id: "r1".into(),
            host: "h".into(),
            corpus_path: "c".into(),
            profiles_path: "p".into(),
            backends: vec![BackendDescriptor {
                spec: "x".into(),
                id: "x".into(),
                kind: "Gguf".into(),
            }],
            judge: None,
            outcomes: vec![],
            model_scores: vec![],
        }
    }

    fn llm_entry(id: &str, recommended: bool) -> LlmEntry {
        LlmEntry {
            id: id.into(),
            display_name: id.into(),
            description: String::new(),
            size_bytes: 0,
            filename: format!("{id}.gguf"),
            url: String::new(),
            sha256: None,
            scores: None,
            recommended,
        }
    }

    fn score(id: &str, q: u8, l: u8, r: u8, cw: u8) -> ModelScore {
        ModelScore {
            backend_id: id.into(),
            n_cells: 1,
            latency_score: l,
            quality_score: q,
            ram_score: r,
            composite_equal: (u32::from(q) + u32::from(l) + u32::from(r))
                .checked_div(3)
                .unwrap_or(0)
                .try_into()
                .unwrap_or(0),
            composite_weighted: cw,
            raw_chrf_mean: 0.0,
            raw_warm_p50_ms: None,
            raw_peak_rss_kb: None,
        }
    }

    #[test]
    fn winner_gets_recommended_others_cleared() {
        let mut c = Catalog {
            schema_version: 1,
            last_run: None,
            stt: vec![],
            llm: vec![llm_entry("a", true), llm_entry("b", false)],
        };
        let scores = vec![score("a", 50, 50, 50, 50), score("b", 90, 90, 90, 90)];
        apply_scores(&mut c, &scores, &empty_report()).unwrap();
        assert!(!c.llm.iter().find(|e| e.id == "a").unwrap().recommended);
        assert!(c.llm.iter().find(|e| e.id == "b").unwrap().recommended);
    }

    #[test]
    fn tie_on_weighted_breaks_to_higher_quality() {
        // The composite_weighted weights (0.5/0.3/0.2) can land both
        // backends on 50 when one wins quality and the other wins
        // latency+ram. In that case the user-visible default should follow
        // quality — the most heavily weighted axis. Regression test for
        // the case that bit us in production with the 2026-05-20 iso run.
        let mut c = Catalog {
            schema_version: 1,
            last_run: None,
            stt: vec![],
            llm: vec![llm_entry("quality_wins", false), llm_entry("speed_wins", false)],
        };
        let scores = vec![
            score("quality_wins", 100, 0, 0, 50),
            score("speed_wins", 0, 100, 100, 50),
        ];
        apply_scores(&mut c, &scores, &empty_report()).unwrap();
        let q = c.llm.iter().find(|e| e.id == "quality_wins").unwrap();
        let s = c.llm.iter().find(|e| e.id == "speed_wins").unwrap();
        assert!(q.recommended, "quality_wins should get the badge on tie");
        assert!(!s.recommended);
    }

    #[test]
    fn unknown_backend_id_errors_with_hint() {
        let mut c = Catalog {
            schema_version: 1,
            last_run: None,
            stt: vec![],
            llm: vec![llm_entry("a", false)],
        };
        let scores = vec![score("a", 50, 50, 50, 50), score("ghost", 90, 90, 90, 90)];
        let err = apply_scores(&mut c, &scores, &empty_report()).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("ghost"), "msg: {msg}");
        assert!(msg.contains("hint:"), "msg: {msg}");
    }

    #[test]
    fn entries_outside_report_get_recommended_false() {
        let mut c = Catalog {
            schema_version: 1,
            last_run: None,
            stt: vec![],
            llm: vec![
                llm_entry("a", false),
                llm_entry("b", true),
                llm_entry("c", false),
            ],
        };
        // Bake-off only scored a + b; c was not run this round.
        let scores = vec![score("a", 90, 90, 90, 90), score("b", 10, 10, 10, 10)];
        apply_scores(&mut c, &scores, &empty_report()).unwrap();
        assert!(c.llm.iter().find(|e| e.id == "a").unwrap().recommended);
        assert!(!c.llm.iter().find(|e| e.id == "b").unwrap().recommended);
        assert!(!c.llm.iter().find(|e| e.id == "c").unwrap().recommended);
        // c keeps its score=None (no overwrite).
        assert!(c.llm.iter().find(|e| e.id == "c").unwrap().scores.is_none());
    }

    #[test]
    fn last_run_metadata_populated() {
        let mut c = Catalog {
            schema_version: 1,
            last_run: None,
            stt: vec![],
            llm: vec![llm_entry("a", false)],
        };
        let scores = vec![score("a", 50, 50, 50, 50)];
        apply_scores(&mut c, &scores, &empty_report()).unwrap();
        let lr = c.last_run.expect("populated");
        assert_eq!(lr.run_id, "r1");
        assert_eq!(lr.host, "h");
    }
}
