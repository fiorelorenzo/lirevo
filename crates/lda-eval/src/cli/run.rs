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
    for (idx, backend) in backends.iter_mut().enumerate() {
        for case in &cases {
            let outcome = run_one_cell(backend.as_mut(), case, &profiles, embedder.as_mut())
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
) -> Result<CellOutcome> {
    let profile = profiles.get(&case.profile).expect("validated upstream");
    let sys = profile
        .system_prompts
        .get(&case.language)
        .expect("validated upstream")
        .clone();

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
    let candidate = final_out.text.clone();

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
