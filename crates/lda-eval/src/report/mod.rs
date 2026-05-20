//! Report writer: markdown summary + JSON sidecar.

pub mod json;
pub mod markdown;

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::corpus::TestCase;
use crate::probes::latency::LatencyCell;
use crate::scoring::composite::{score_run, ModelScore};
use crate::scoring::ScoreCard;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CellOutcome {
    pub case: TestCase,
    pub candidate: String,
    pub score: ScoreCard,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency: Option<LatencyCell>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peak_rss_kb: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportData {
    pub run_id: String,
    pub host: String,
    pub corpus_path: String,
    pub profiles_path: String,
    pub backends: Vec<BackendDescriptor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub judge: Option<BackendDescriptor>,
    pub outcomes: Vec<CellOutcome>,
    /// Per-backend aggregated scores, computed at write time from `outcomes`.
    /// Populated by `write_pair`; deserialized reports may carry this field
    /// from disk so downstream tooling (e.g. `lda-eval bless`) can avoid
    /// recomputing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub model_scores: Vec<ModelScore>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendDescriptor {
    pub spec: String,
    pub id: String,
    pub kind: String,
}

pub fn write_pair(data: &ReportData, md_path: &Path) -> std::io::Result<()> {
    // Populate `model_scores` so the JSON sidecar carries them for later
    // consumption (e.g. `lda-eval bless`).
    let mut data = data.clone();
    if data.model_scores.is_empty() {
        data.model_scores = score_run(&data.outcomes);
    }
    std::fs::write(md_path, markdown::render(&data))?;
    let json_path = md_path.with_extension("json");
    let json = serde_json::to_string_pretty(&data).map_err(std::io::Error::other)?;
    std::fs::write(&json_path, json)?;
    Ok(())
}

/// Group outcomes by `backend.id`.
#[must_use]
pub fn group_by_backend(data: &ReportData) -> HashMap<String, Vec<&CellOutcome>> {
    let mut m: HashMap<String, Vec<&CellOutcome>> = HashMap::new();
    for o in &data.outcomes {
        m.entry(o.score.backend_id.clone()).or_default().push(o);
    }
    m
}
