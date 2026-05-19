//! `lda-eval run` — execute backends × corpus, score, write report.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

use crate::backend::{build_from_spec, EvalBackend, GenerateReq};
use crate::cli::RunArgs;
use crate::corpus::{load_jsonl, TestCase};
use crate::probes::{latency, memory};
use crate::profiles::{load_scoring, load_toml, validate, Profile};
use crate::report::{write_pair, BackendDescriptor, CellOutcome, ReportData};
use crate::scoring::{
    assertions::run_all,
    chrf::chrf,
    embedding::{cosine, Embedder, EmbedderConfig},
    length::length_ratio,
    ScoreCard,
};

pub async fn run(args: RunArgs) -> Result<()> {
    let cases = load_jsonl(&args.corpus).context("load corpus")?;
    let profiles = load_toml(&args.profiles).context("load profiles")?;
    validate(&cases, &profiles).context("cross-ref")?;

    let mut embedder: Option<Embedder> = None;
    if args.embed {
        let scoring_cfg = load_scoring(&args.profiles).context("load scoring config")?;
        let Some(cfg) = scoring_cfg.embedding else {
            anyhow::bail!("--embed requested but [scoring.embedding] missing from profiles file");
        };
        let cache_dir = std::env::var_os("LDA_EVAL_EMBED_CACHE")
            .map_or_else(|| PathBuf::from("crates/lda-eval/.cache"), PathBuf::from);
        let ec = EmbedderConfig {
            cache_dir,
            model_url: cfg.model_url,
            tokenizer_url: cfg.tokenizer_url,
            model_sha256: cfg.model_sha256,
            tokenizer_sha256: cfg.tokenizer_sha256,
        };
        embedder = Some(Embedder::load(&ec).context("load embedder")?);
    }

    let mut backends: Vec<Box<dyn EvalBackend>> = Vec::new();
    let mut descriptors: Vec<BackendDescriptor> = Vec::new();
    for spec in &args.backends {
        let b = build_from_spec(spec)
            .await
            .with_context(|| format!("backend {spec}"))?;
        descriptors.push(BackendDescriptor {
            spec: spec.clone(),
            id: b.id().to_string(),
            kind: format!("{:?}", b.kind()),
        });
        backends.push(b);
    }

    let mut outcomes: Vec<CellOutcome> = Vec::with_capacity(backends.len() * cases.len());
    let no_think_set: std::collections::HashSet<String> = args.no_think_for.iter().cloned().collect();
    for (idx, backend) in backends.iter_mut().enumerate() {
        let no_think = no_think_set.contains(&descriptors[idx].id);
        for case in &cases {
            let outcome = run_one_cell(
                backend.as_mut(),
                case,
                &profiles,
                embedder.as_mut(),
                no_think,
            )
            .await
            .with_context(|| format!("backend={} case={}", descriptors[idx].id, case.id))?;
            outcomes.push(outcome);
        }
    }

    let report = ReportData {
        run_id: timestamp_id(),
        host: host_string(),
        corpus_path: args.corpus.to_string_lossy().into(),
        profiles_path: args.profiles.to_string_lossy().into(),
        backends: descriptors,
        judge: None,
        outcomes,
    };

    write_pair(&report, &args.out).context("write report")?;
    println!(
        "wrote {} and {}",
        args.out.display(),
        args.out.with_extension("json").display(),
    );
    Ok(())
}

async fn run_one_cell(
    backend: &mut dyn EvalBackend,
    case: &TestCase,
    profiles: &HashMap<String, Profile>,
    embedder: Option<&mut Embedder>,
    no_think: bool,
) -> Result<CellOutcome> {
    let profile = profiles.get(&case.profile).expect("validated upstream");
    let mut sys = profile
        .system_prompts
        .get(&case.language)
        .expect("validated upstream")
        .clone();
    if no_think {
        // Qwen3 / Qwen3.5 hybrid directive: disable the `<think>…</think>` prelude
        // so the refiner output isn't polluted. Non-Qwen models silently ignore it.
        sys.push_str("\n\n/no_think");
    }

    let req = GenerateReq {
        system_prompt: sys,
        transcript: case.transcript.clone(),
        max_tokens: 800,
        temperature: 0.2,
    };

    // Latency probe drives 5 generations (1 cold + 4 warm).
    let latency_cell = latency::probe_cell(backend, req.clone(), 5).await.ok();

    // Canonical "candidate" text for scoring — one more generation after the probe
    // so we don't depend on the probe internals.
    let final_out = backend.generate(req).await?;
    // Strip <think>...</think> blocks that Qwen3-family hybrid models emit
    // before their actual answer. No-op for models that don't produce them.
    let candidate = strip_think(&final_out.text);

    let chrf_val = chrf(&candidate, &case.expected, 6, 2.0);
    let len = length_ratio(&candidate, &case.expected);
    let asserts = run_all(&candidate, &profile.post_assertions);
    let passed = u32::try_from(asserts.iter().filter(|a| a.passed).count()).unwrap_or(0);
    let total = u32::try_from(asserts.len()).unwrap_or(0);

    let embedding_cosine = if let Some(e) = embedder {
        let a = e.embed(&candidate)?;
        let b = e.embed(&case.expected)?;
        Some(cosine(&a, &b))
    } else {
        None
    };

    Ok(CellOutcome {
        case: case.clone(),
        candidate,
        score: ScoreCard {
            case_id: case.id.clone(),
            backend_id: backend.id().to_string(),
            chrf: chrf_val,
            length_ratio: len,
            assertions_passed: passed,
            assertions_total: total,
            embedding_cosine,
            judge_fidelity: None,
            judge_style: None,
            judge_rationale: None,
        },
        latency: latency_cell,
        peak_rss_kb: memory::peak_rss_kb(),
    })
}

fn timestamp_id() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("ts-{secs}")
}

fn host_string() -> String {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    format!("{os}-{arch}")
}

/// Remove `<think>…</think>` blocks (and trim leftover whitespace) from a
/// model's raw output. Used to extract the canonical answer from Qwen3 /
/// Qwen3.5 hybrid models that emit a chain-of-thought prelude. Non-thinking
/// models don't produce these tags so the function is a no-op for them.
fn strip_think(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("<think>") {
        out.push_str(&rest[..start]);
        if let Some(end_rel) = rest[start..].find("</think>") {
            let after = start + end_rel + "</think>".len();
            rest = &rest[after..];
        } else {
            // Unterminated <think>: drop everything from the tag onward.
            rest = "";
            break;
        }
    }
    out.push_str(rest);
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::strip_think;

    #[test]
    fn strip_think_removes_balanced_block() {
        let s = "<think>\nI reason here.\n</think>\n\nFinal answer.";
        assert_eq!(strip_think(s), "Final answer.");
    }

    #[test]
    fn strip_think_handles_empty_block() {
        let s = "<think>\n\n</think>\n\nOK.";
        assert_eq!(strip_think(s), "OK.");
    }

    #[test]
    fn strip_think_noop_when_absent() {
        let s = "Just an answer.";
        assert_eq!(strip_think(s), "Just an answer.");
    }

    #[test]
    fn strip_think_handles_multiple_blocks() {
        let s = "<think>a</think>between<think>b</think>after";
        assert_eq!(strip_think(s), "betweenafter");
    }

    #[test]
    fn strip_think_drops_unterminated_block() {
        let s = "Answer.\n<think>truncated...";
        assert_eq!(strip_think(s), "Answer.");
    }
}
