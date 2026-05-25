//! `lirevo-eval run` — orchestrate one bake-off across N backends × corpus.
//!
//! Each backend is loaded and generated inside its own `bake-cell`
//! subprocess (see `cli::bake_cell`). The parent fans the same corpus + the
//! resolved system prompts out to each child via stdin, reads candidate
//! strings + per-cell latency back via stdout, and scores chrF / length /
//! assertions / cosine in this process — which keeps the heavy model loaders
//! out of the parent's address space.
//!
//! The per-backend RSS reported by each child is therefore the high-water
//! mark of an address space that contains exactly one model — no allocator
//! residue from a previously-dropped backend, no shared embedder weights.
//! That's what makes the catalog's RAM score comparable across backends.
//!
//! Bringing the embedder up in the parent (when `--embed`) is intentional:
//! it loads once for the whole run instead of being paid per child, and the
//! ONNX session shouldn't bias an LLM's RSS measurement either way since
//! each LLM lives in its own process.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use tokio::io::AsyncWriteExt;

use crate::cli::bake_cell::{BakeCellRequest, BakeCellResponse, OUT_PATH_ENV};
use crate::cli::RunArgs;
use crate::corpus::{load_jsonl, TestCase};
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
        let cache_dir = std::env::var_os("LIREVO_EVAL_EMBED_CACHE")
            .map_or_else(|| PathBuf::from("crates/lirevo-eval/.cache"), PathBuf::from);
        let ec = EmbedderConfig {
            cache_dir,
            model_url: cfg.model_url,
            tokenizer_url: cfg.tokenizer_url,
            model_sha256: cfg.model_sha256,
            tokenizer_sha256: cfg.tokenizer_sha256,
        };
        embedder = Some(Embedder::load(&ec).context("load embedder")?);
    }

    let mut descriptors: Vec<BackendDescriptor> = Vec::with_capacity(args.backends.len());
    let mut outcomes: Vec<CellOutcome> = Vec::with_capacity(args.backends.len() * cases.len());

    // Build the case-id → resolved system-prompt map once. The mapping is
    // backend-independent (the /no_think suffix is appended by the child if
    // applicable), and reusing it avoids re-cloning the profile state per
    // subprocess.
    let system_prompts = build_system_prompts(&cases, &profiles)?;

    for spec in &args.backends {
        let resp = run_one_backend(
            spec,
            &cases,
            &system_prompts,
            &args.no_think_for,
        )
        .await
        .with_context(|| format!("backend {spec}"))?;

        let descriptor = BackendDescriptor {
            spec: spec.clone(),
            id: resp.backend_id.clone(),
            kind: resp.backend_kind.clone(),
        };

        // Map child results back onto cases by case id.
        let by_case: HashMap<&str, &crate::cli::bake_cell::BakeCellResult> =
            resp.cells.iter().map(|c| (c.case_id.as_str(), c)).collect();
        for case in &cases {
            let cell = by_case.get(case.id.as_str()).ok_or_else(|| {
                anyhow::anyhow!("child returned no result for case {}", case.id)
            })?;
            let profile = profiles.get(&case.profile).expect("validated upstream");
            let outcome = score_one_cell(
                case,
                profile,
                &descriptor.id,
                &cell.candidate,
                cell.latency.clone(),
                resp.peak_rss_kb,
                embedder.as_mut(),
            )?;
            outcomes.push(outcome);
        }

        descriptors.push(descriptor);
    }

    let report = ReportData {
        run_id: timestamp_id(),
        host: host_string(),
        corpus_path: args.corpus.to_string_lossy().into(),
        profiles_path: args.profiles.to_string_lossy().into(),
        backends: descriptors,
        judge: None,
        outcomes,
        model_scores: Vec::new(),
    };

    write_pair(&report, &args.out).context("write report")?;
    println!(
        "wrote {} and {}",
        args.out.display(),
        args.out.with_extension("json").display(),
    );
    Ok(())
}

async fn run_one_backend(
    spec: &str,
    cases: &[TestCase],
    system_prompts: &HashMap<String, String>,
    no_think_for: &[String],
) -> Result<BakeCellResponse> {
    let exe = std::env::current_exe().context("locate current_exe for subprocess spawn")?;
    let req = BakeCellRequest {
        spec: spec.to_string(),
        latency_probe_runs: 5,
        max_tokens: 800,
        temperature: 0.2,
        no_think_for: no_think_for.to_vec(),
        cases: cases.to_vec(),
        system_prompts: system_prompts.clone(),
    };
    let req_json = serde_json::to_vec(&req).context("serialize bake-cell request")?;

    // The child's JSON response lands in this temp file. stdin carries the
    // request; stdout/stderr stay inherited so the user sees llama.cpp +
    // tracing progress live (and so that output can't corrupt the response).
    let out_file = tempfile::NamedTempFile::new().context("create bake-cell tempfile")?;
    let out_path = out_file.path().to_path_buf();

    let mut child = tokio::process::Command::new(&exe)
        .arg("bake-cell")
        .env(OUT_PATH_ENV, &out_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("spawn {} bake-cell", exe.display()))?;

    {
        let stdin = child
            .stdin
            .as_mut()
            .context("child stdin missing")?;
        stdin.write_all(&req_json).await.context("write child stdin")?;
        stdin.shutdown().await.context("close child stdin")?;
    }

    let status = child.wait().await.context("await child")?;
    if !status.success() {
        anyhow::bail!(
            "bake-cell subprocess for {spec} exited with status {status}; check stderr above",
        );
    }
    let buf = std::fs::read(&out_path).with_context(|| {
        format!(
            "read bake-cell response from {} (child exited 0 but produced no output file)",
            out_path.display()
        )
    })?;
    serde_json::from_slice(&buf).context("parse bake-cell response")
}

fn build_system_prompts(
    cases: &[TestCase],
    profiles: &HashMap<String, Profile>,
) -> Result<HashMap<String, String>> {
    let mut out = HashMap::with_capacity(cases.len());
    for case in cases {
        let profile = profiles
            .get(&case.profile)
            .ok_or_else(|| anyhow::anyhow!("unknown profile: {}", case.profile))?;
        let sys = profile
            .system_prompts
            .get(&case.language)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "profile {} missing system prompt for language {}",
                    case.profile,
                    case.language
                )
            })?
            .clone();
        out.insert(case.id.clone(), sys);
    }
    Ok(out)
}

fn score_one_cell(
    case: &TestCase,
    profile: &Profile,
    backend_id: &str,
    candidate: &str,
    latency: Option<crate::probes::latency::LatencyCell>,
    peak_rss_kb: Option<u64>,
    embedder: Option<&mut Embedder>,
) -> Result<CellOutcome> {
    let chrf_val = chrf(candidate, &case.expected, 6, 2.0);
    let len = length_ratio(candidate, &case.expected);
    let asserts = run_all(candidate, &profile.post_assertions);
    let passed = u32::try_from(asserts.iter().filter(|a| a.passed).count()).unwrap_or(0);
    let total = u32::try_from(asserts.len()).unwrap_or(0);
    let embedding_cosine = if let Some(e) = embedder {
        let a = e.embed(candidate)?;
        let b = e.embed(&case.expected)?;
        Some(cosine(&a, &b))
    } else {
        None
    };
    Ok(CellOutcome {
        case: case.clone(),
        candidate: candidate.to_string(),
        score: ScoreCard {
            case_id: case.id.clone(),
            backend_id: backend_id.to_string(),
            chrf: chrf_val,
            length_ratio: len,
            assertions_passed: passed,
            assertions_total: total,
            embedding_cosine,
            judge_fidelity: None,
            judge_style: None,
            judge_rationale: None,
        },
        latency,
        peak_rss_kb,
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
