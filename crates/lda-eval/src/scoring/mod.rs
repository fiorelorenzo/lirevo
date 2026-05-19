//! Scoring stack: lexical now, semantic + judge in later tasks.

pub mod assertions;
pub mod chrf;
pub mod embedding;
pub mod length;

/// Aggregated score record per (case, backend) — populated incrementally
/// by the run subcommand as each scorer fires.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ScoreCard {
    pub case_id: String,
    pub backend_id: String,
    pub chrf: f64,
    pub length_ratio: f64,
    pub assertions_passed: u32,
    pub assertions_total: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedding_cosine: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub judge_fidelity: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub judge_style: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub judge_rationale: Option<String>,
}
