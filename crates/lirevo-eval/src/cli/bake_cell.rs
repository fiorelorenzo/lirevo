//! `lirevo-eval bake-cell` — internal subprocess worker.
//!
//! Loads exactly one backend, runs the per-case generation loop (5-shot
//! latency probe + canonical generation), and writes results back to the
//! parent via stdout JSON. Lives in a subprocess so each backend's RSS
//! measurement is isolated from every other backend in the run — no
//! allocator residue, no shared address space.
//!
//! Wire format:
//! - Request: JSON on stdin. See [`BakeCellRequest`]. Includes the spec,
//!   the resolved per-case system prompts, and a `no_think_for` allowlist
//!   applied once the backend's `id()` is known.
//! - Response: JSON written to the file path passed via the
//!   `LIREVO_EVAL_BAKE_CELL_OUT` env var. See [`BakeCellResponse`]. The peak
//!   RSS is read with `probes::memory::peak_rss_kb()` from inside this
//!   process, so it reflects only this backend's footprint.
//!
//! stdout/stderr are intentionally NOT used for the response — they stay
//! inherited from the parent so the user sees live llama.cpp / ggml /
//! tracing progress during the run. Mixing those streams with the JSON
//! response would be brittle (both tracing-subscriber's default writer and
//! llama.cpp's C printf target stdout).
//!
//! This command is hidden in the CLI help — it's an implementation detail
//! of `lirevo-eval run` and should not be invoked by hand.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::backend::{build_from_spec, GenerateReq};
use crate::corpus::TestCase;
use crate::probes::latency::LatencyCell;
use crate::probes::{latency, memory};
use crate::util::strip_think;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BakeCellRequest {
    pub spec: String,
    pub latency_probe_runs: u32,
    pub max_tokens: u32,
    /// Backend ids whose system prompt should get `\n\n/no_think` appended.
    pub no_think_for: Vec<String>,
    pub cases: Vec<TestCase>,
    /// Resolved system prompt keyed by `case.id`. Built by the parent so the
    /// child does not need profile state.
    pub system_prompts: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BakeCellResponse {
    pub backend_id: String,
    pub backend_kind: String,
    /// Peak RSS for this child process over its entire lifetime.
    pub peak_rss_kb: Option<u64>,
    pub cells: Vec<BakeCellResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BakeCellResult {
    pub case_id: String,
    /// Raw candidate text after `strip_think`. The parent scores it.
    pub candidate: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency: Option<LatencyCell>,
}

/// Env var: path the child writes its JSON response to. The parent picks
/// this path (typically `tempfile::NamedTempFile`) and reads it back after
/// `waitpid`.
pub const OUT_PATH_ENV: &str = "LIREVO_EVAL_BAKE_CELL_OUT";

pub async fn run() -> Result<()> {
    let out_path = std::env::var_os(OUT_PATH_ENV)
        .map(PathBuf::from)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "{OUT_PATH_ENV} not set — this command is meant to be spawned by `lirevo-eval run`"
            )
        })?;
    let req = read_request().context("read bake-cell request")?;
    let mut backend = build_from_spec(&req.spec)
        .await
        .with_context(|| format!("build backend {}", req.spec))?;
    let backend_id = backend.id().to_string();
    let backend_kind = format!("{:?}", backend.kind());

    let no_think_set: HashSet<&str> = req.no_think_for.iter().map(String::as_str).collect();
    let append_no_think = no_think_set.contains(backend_id.as_str());

    let mut cells = Vec::with_capacity(req.cases.len());
    for case in &req.cases {
        let mut sys = req
            .system_prompts
            .get(&case.id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("no system prompt for case {}", case.id))?;
        if append_no_think {
            // Qwen3 / Qwen3.5 hybrid directive — see cli/run.rs for context.
            sys.push_str("\n\n/no_think");
        }
        let gen_req = GenerateReq {
            system_prompt: sys,
            transcript: case.transcript.clone(),
            max_tokens: req.max_tokens,
            // Per-model sampler best practices keyed on the resolved backend
            // id, replacing the previous fixed temperature=0.2. See
            // `best_practices_for`.
            ..best_practices_for(&backend_id)
        };
        let latency_cell = latency::probe_cell(
            backend.as_mut(),
            gen_req.clone(),
            req.latency_probe_runs as usize,
        )
        .await
        .ok();
        let final_out = backend.generate(gen_req).await?;
        let candidate = strip_think(&final_out.text);
        cells.push(BakeCellResult {
            case_id: case.id.clone(),
            candidate,
            latency: latency_cell,
        });
    }

    // Capture peak RSS BEFORE dropping the backend, so the measurement reflects
    // the high-water mark while weights + KV cache were resident.
    let peak_rss_kb = memory::peak_rss_kb();
    drop(backend);

    let resp = BakeCellResponse {
        backend_id,
        backend_kind,
        peak_rss_kb,
        cells,
    };
    let json = serde_json::to_vec(&resp).context("serialize bake-cell response")?;
    std::fs::write(&out_path, &json)
        .with_context(|| format!("write bake-cell response to {}", out_path.display()))?;
    Ok(())
}

fn read_request() -> Result<BakeCellRequest> {
    let mut buf = String::new();
    std::io::stdin()
        .lock()
        .read_to_string(&mut buf)
        .context("read stdin")?;
    serde_json::from_str(&buf).context("parse bake-cell request json")
}

/// Official sampling-parameter best practices per model family, mapped by
/// backend id (the `<id>` part of the `<kind>:<id>[@<path>]` spec). Sources
/// in the HF model cards' "Best Practices > Sampling Parameters" sections —
/// **except Gemma, see below**. Falls back to a permissive default
/// (temperature 0.7, `top_p` 0.9, `top_k` 40, no penalties) for backend ids
/// we don't recognize — covers Claude CLI and any newly-added GGUF before we
/// record its recommended values.
///
/// Updated 2026-05-19. When new model families are added to the bake-off,
/// re-read the model card here:
/// - Qwen3 / Qwen3-Instruct-2507: `temp=0.7, top_p=0.8, top_k=20, min_p=0`
/// - Qwen3.5 (non-thinking text):  `temp=1.0, top_p=1.0, top_k=20, min_p=0,
///   presence_penalty=2.0, repetition_penalty=1.0`
/// - Gemma 3 1B / 270M: **not** from a model card — see the branch below.
fn best_practices_for(backend_id: &str) -> GenerateReq {
    // Match by family prefix so per-instance ids like "lms-q3.5-2b" or
    // "qwen3.5-2b@<path>" route to the right preset.
    let id = backend_id.to_lowercase();
    if id.contains("qwen3.5") || id.contains("q3.5") {
        return GenerateReq {
            temperature: 1.0,
            top_p: 1.0,
            top_k: 20,
            min_p: 0.0,
            presence_penalty: 2.0,
            repetition_penalty: 1.0,
            ..GenerateReq::default()
        };
    }
    if id.contains("qwen3") || id.contains("q3-") {
        return GenerateReq {
            temperature: 0.7,
            top_p: 0.8,
            top_k: 20,
            min_p: 0.0,
            presence_penalty: 0.0,
            repetition_penalty: 1.0,
            ..GenerateReq::default()
        };
    }
    if id.contains("gemma") {
        // Deliberately NOT a model-card value — Google publishes no sampling
        // guidance for Gemma 3 1B/270M. Since v0.7 the shipped app has a
        // fixed catalog of exactly one cleanup model, Gemma 3 1B (see
        // `AGENTS.md`), so for Gemma the eval's job is to predict the
        // product, not to guess at generic best practice. These are the
        // app's real sampler values, sourced from the app itself rather than
        // re-typed as literals: `app/src-tauri/src/hotkey.rs` sets
        // `temperature: 0.2` explicitly and takes the rest from
        // `inference_core::ChatRequest::default()`, which we read directly
        // below (top_p 0.9, top_k 40, min_p 0.0, presence_penalty 0.0,
        // repetition_penalty 1.0). Do not "upgrade" this back to a generic
        // HF-Transformers default; that was the previous bug (see git
        // history on this branch).
        let app_defaults = inference_core::ChatRequest::default();
        return GenerateReq {
            temperature: 0.2,
            top_p: app_defaults.top_p,
            top_k: app_defaults.top_k,
            min_p: app_defaults.min_p,
            presence_penalty: app_defaults.presence_penalty,
            repetition_penalty: app_defaults.repetition_penalty,
            ..GenerateReq::default()
        };
    }
    GenerateReq::default()
}

#[cfg(test)]
mod tests {
    use super::{best_practices_for, BakeCellRequest, BakeCellResponse, BakeCellResult};
    use std::collections::HashMap;

    /// Gemma has no HF model-card guidance, so its sampler params must match
    /// the shipped app (`hotkey.rs` temperature + `ChatRequest::default()`
    /// for the rest) rather than a generic fallback — see the comment on
    /// `best_practices_for`. Pins the values so a future edit can't silently
    /// drift the eval away from what v0.7+ actually ships.
    #[test]
    fn gemma_sampler_params_match_the_shipped_app() {
        let req = best_practices_for("gguf:gemma-3-1b-it-q4@/tmp/m.gguf");
        assert!(
            (req.temperature - 0.2).abs() < 1e-6,
            "got {}",
            req.temperature
        );
        assert!((req.top_p - 0.9).abs() < 1e-6, "got {}", req.top_p);
        assert_eq!(req.top_k, 40);
        assert!(req.min_p.abs() < 1e-6, "got {}", req.min_p);
        assert!(
            req.presence_penalty.abs() < 1e-6,
            "got {}",
            req.presence_penalty
        );
        assert!(
            (req.repetition_penalty - 1.0).abs() < 1e-6,
            "got {}",
            req.repetition_penalty
        );
    }

    #[test]
    fn request_roundtrips_through_json() {
        let req = BakeCellRequest {
            spec: "gguf:demo@/tmp/m.gguf".into(),
            latency_probe_runs: 5,
            max_tokens: 800,
            no_think_for: vec!["qwen3.5-2b".into()],
            cases: vec![],
            system_prompts: HashMap::new(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: BakeCellRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.spec, req.spec);
        assert_eq!(back.no_think_for, req.no_think_for);
    }

    #[test]
    fn response_roundtrips_through_json() {
        let resp = BakeCellResponse {
            backend_id: "demo".into(),
            backend_kind: "Gguf".into(),
            peak_rss_kb: Some(2_400_000),
            cells: vec![BakeCellResult {
                case_id: "x".into(),
                candidate: "ok".into(),
                latency: None,
            }],
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: BakeCellResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.backend_id, "demo");
        assert_eq!(back.peak_rss_kb, Some(2_400_000));
        assert_eq!(back.cells.len(), 1);
    }
}
