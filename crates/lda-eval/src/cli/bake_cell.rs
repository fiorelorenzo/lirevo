//! `lda-eval bake-cell` — internal subprocess worker.
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
//!   `LDA_EVAL_BAKE_CELL_OUT` env var. See [`BakeCellResponse`]. The peak
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
//! of `lda-eval run` and should not be invoked by hand.

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
    pub temperature: f32,
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
pub const OUT_PATH_ENV: &str = "LDA_EVAL_BAKE_CELL_OUT";

pub async fn run() -> Result<()> {
    let out_path = std::env::var_os(OUT_PATH_ENV)
        .map(PathBuf::from)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "{OUT_PATH_ENV} not set — this command is meant to be spawned by `lda-eval run`"
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
            temperature: req.temperature,
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

#[cfg(test)]
mod tests {
    use super::{BakeCellRequest, BakeCellResponse, BakeCellResult};
    use std::collections::HashMap;

    #[test]
    fn request_roundtrips_through_json() {
        let req = BakeCellRequest {
            spec: "gguf:demo@/tmp/m.gguf".into(),
            latency_probe_runs: 5,
            max_tokens: 800,
            temperature: 0.2,
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
