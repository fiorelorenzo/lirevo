//! Catalog of STT models supported by the parakeet-cpp pipeline.
//!
//! Single entry: `parakeet-tdt-0.6b-v3` (default, 25 EU languages, GGUF q4_k).
//!
//! This catalog is the single source of truth on the backend side. The
//! frontend mirrors it in `app/src/lib/models/catalog.ts` and the
//! [`crate::commands::models::get_stt_catalog`] command surfaces it via
//! Tauri so a dev-build runtime check can assert the two stay in sync.

use serde::Serialize;

/// Hugging Face repo + file for the single shipped STT model.
pub const STT_HF_REPO: &str = "mudler/parakeet-cpp-gguf";
pub const STT_GGUF_FILENAME: &str = "tdt-0.6b-v3-q4_k.gguf";

/// Direct download URL for [`STT_GGUF_FILENAME`].
#[must_use]
pub fn stt_gguf_url() -> String {
    format!("https://huggingface.co/{STT_HF_REPO}/resolve/main/{STT_GGUF_FILENAME}")
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LanguageCoverage {
    /// 25 European languages.
    European25,
    /// 30 global languages (incl. CJK / Arabic / Hindi).
    Global30,
    /// Whisper-style ~99-language coverage. The wizard expands this to a
    /// curated subset rather than dumping the full Whisper language list.
    Multilingual99,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    /// Stable catalog id used in settings + UI.
    pub id: &'static str,
    /// Human-facing display name for the wizard.
    pub display_name: &'static str,
    /// Approximate on-disk size in **bytes**. Used by the wizard for the
    /// "this will download N MB" copy; the canonical size comes from the
    /// HF cache at fetch time.
    pub size_bytes: u64,
    /// What language family this model covers.
    pub language_coverage: LanguageCoverage,
    /// One-line marketing summary shown under the display name in the
    /// wizard radio card. Keep it terse — the card is small.
    pub summary: &'static str,
    /// SPDX-style license label. Rendered as a small badge in the wizard.
    pub license: &'static str,
    /// Languages this model can decode, as ISO 639-1/2 codes. The wizard
    /// language step builds its dropdown from this list. The "auto-detect"
    /// option is added on the frontend side, not stored here.
    pub languages: &'static [&'static str],
    /// Whether this entry is the default pick for a fresh install.
    pub default: bool,
}

// Language list pinned per catalog spec. Kept as a module-level constant so
// the [`Metadata`] entry can reference it by `&'static [&'static str]`.

const PARAKEET_LANGUAGES: &[&str] = &[
    "en", "it", "de", "fr", "es", "pt", "nl", "pl", "ru", "uk", "cs", "hr",
    "bg", "da", "el", "et", "fi", "hu", "lv", "lt", "mt", "ro", "sk", "sl",
    "sv",
];

const PARAKEET_V3: Metadata = Metadata {
    id: "parakeet-tdt-0.6b-v3",
    display_name: "Parakeet TDT v3",
    // q4_k GGUF, ~644 MB on disk.
    size_bytes: 644_000_000,
    language_coverage: LanguageCoverage::European25,
    summary: "25 European languages. Runs fully on-device.",
    license: "CC-BY-4.0",
    languages: PARAKEET_LANGUAGES,
    default: true,
};

const MODELS: &[Metadata] = &[PARAKEET_V3];

#[must_use]
pub const fn default_model_id() -> &'static str {
    PARAKEET_V3.id
}

#[must_use]
pub fn list_models() -> &'static [Metadata] {
    MODELS
}

#[must_use]
pub fn model_metadata(id: &str) -> Option<&'static Metadata> {
    MODELS.iter().find(|m| m.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exactly_one_default_entry() {
        let defaults = list_models().iter().filter(|m| m.default).count();
        assert_eq!(defaults, 1, "expected exactly one default model, got {defaults}");
    }

    #[test]
    fn default_id_resolves_in_catalog() {
        assert!(model_metadata(default_model_id()).is_some());
    }

    #[test]
    fn unknown_id_returns_none() {
        assert!(model_metadata("nonexistent").is_none());
    }

    #[test]
    fn parakeet_language_list_matches_spec() {
        // Spec pins exactly 25 ISO codes for Parakeet TDT v3.
        assert_eq!(PARAKEET_V3.languages.len(), 25);
        assert!(PARAKEET_V3.languages.contains(&"it"));
        assert!(PARAKEET_V3.languages.contains(&"en"));
    }
}
