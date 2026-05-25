//! Model catalog schema, shared by the main app (read), the dev CLIs, and the
//! `lirevo-eval bless` subcommand (write). The canonical instance lives at
//! `crates/inference-core/data/model_catalog.json` and is loaded into the
//! binary at compile time via `include_str!`.
//!
//! Versioning: `schema_version` is bumped on breaking changes only. New
//! optional fields can be added with `#[serde(default)]` and don't require a
//! bump.

use serde::{Deserialize, Serialize};

pub const CURRENT_SCHEMA_VERSION: u32 = 1;

/// Embedded JSON catalog. Single source of truth at build time.
pub const EMBEDDED_JSON: &str =
    include_str!("../data/model_catalog.json");

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub schema_version: u32,
    /// Metadata about the bake-off whose scores were last blessed into this
    /// file. `None` until the first `lirevo-eval bless` has been run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run: Option<LastRun>,
    #[serde(default)]
    pub stt: Vec<SttEntry>,
    #[serde(default)]
    pub llm: Vec<LlmEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LastRun {
    pub run_id: String,
    pub host: String,
    pub corpus_path: String,
    pub profiles_path: String,
    /// Seconds since UNIX epoch.
    pub ts_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SttEntry {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub size_bytes: u64,
    pub filename: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coreml_encoder: Option<CoremlEncoder>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoremlEncoder {
    pub url: String,
    pub filename: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmEntry {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub size_bytes: u64,
    pub filename: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// Bake-off scores. `None` if this LLM has never been blessed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scores: Option<ModelScores>,
    /// Set to `true` for the weighted-composite winner of the last bake-off.
    /// Defaults to `false`; updated by `lirevo-eval bless`.
    #[serde(default)]
    pub recommended: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelScores {
    /// 0-100, higher is better.
    pub quality: u8,
    pub latency: u8,
    pub ram: u8,
    /// Unweighted mean of the three axes.
    pub composite_equal: u8,
    /// 0.5·quality + 0.3·latency + 0.2·ram. UI default.
    pub composite_weighted: u8,
    pub raw_chrf_mean: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_warm_p50_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_peak_rss_kb: Option<u64>,
    /// Cells contributing to this score.
    pub n_cells: u32,
}

/// Parse the embedded JSON. Panics if the JSON is malformed — it's checked in
/// at build time, so a malformed file is a compile-time error masquerading as
/// a runtime one. Tests exercise this on every CI run.
#[must_use]
pub fn load_embedded() -> Catalog {
    serde_json::from_str(EMBEDDED_JSON)
        .expect("model_catalog.json is malformed — fix the committed file")
}

/// Parse arbitrary JSON. Used by `lirevo-eval bless` (read existing → mutate → write back).
pub fn parse(s: &str) -> Result<Catalog, serde_json::Error> {
    serde_json::from_str(s)
}

#[cfg(test)]
mod tests {
    use super::{load_embedded, CURRENT_SCHEMA_VERSION};

    #[test]
    fn embedded_catalog_parses() {
        let c = load_embedded();
        assert_eq!(c.schema_version, CURRENT_SCHEMA_VERSION);
        assert!(!c.stt.is_empty(), "expected ≥1 STT entry");
        assert!(!c.llm.is_empty(), "expected ≥1 LLM entry");
    }

    #[test]
    fn embedded_catalog_ids_unique() {
        let c = load_embedded();
        let mut seen = std::collections::HashSet::new();
        for e in &c.stt {
            assert!(seen.insert(e.id.clone()), "duplicate stt id: {}", e.id);
        }
        for e in &c.llm {
            assert!(seen.insert(e.id.clone()), "duplicate llm id: {}", e.id);
        }
    }

    #[test]
    fn embedded_catalog_filenames_match_kind() {
        let c = load_embedded();
        for e in &c.stt {
            assert!(
                std::path::Path::new(&e.filename)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("bin")),
                "stt filename should end in .bin: {}",
                e.filename
            );
        }
        for e in &c.llm {
            assert!(
                std::path::Path::new(&e.filename)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("gguf")),
                "llm filename should end in .gguf: {}",
                e.filename
            );
        }
    }

    #[test]
    fn embedded_catalog_has_at_most_one_recommended_llm() {
        let c = load_embedded();
        let n = c.llm.iter().filter(|e| e.recommended).count();
        assert!(n <= 1, "expected ≤1 recommended LLM, got {n}");
    }
}
