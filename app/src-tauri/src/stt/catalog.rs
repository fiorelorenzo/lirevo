//! Hardcoded catalog of STT models supported by the M4 audiopipe pipeline.
//!
//! Three entries, matching the M4 spec §3:
//!   1. `parakeet-tdt-0.6b-v3` — default, 25 EU languages.
//!      On Apple Silicon the loader silently prefers the `-mlx` variant for
//!      MLX-accelerated inference; that's the routing concern of
//!      [`audiopipe_name_for_platform`], not the catalog id itself. The MLX
//!      variant loads its 0.6B weights in bf16 (~1.2 GB resident; fp32 was
//!      ~2.4 GB).
//!   2. `qwen3-asr-0.6b-ggml` — opt-in, 30 languages, broad coverage.
//!   3. `whisper-large-v3-turbo` — fallback, 99 langs. **Requires the
//!      `whisper` Cargo feature on `audiopipe`**, which is OFF in the M4
//!      Phase-2 baseline (it gets re-enabled in Task 8 once `whisper-rs` is
//!      gone from `inference-core` and the native-library conflict clears).
//!
//! This catalog is the single source of truth on the backend side. The
//! frontend mirrors it in `app/src/lib/models/catalog.ts` and the
//! [`crate::commands::models::get_stt_catalog`] command surfaces it via
//! Tauri so a dev-build runtime check can assert the two stay in sync.

use serde::Serialize;

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

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FeatureRequirement {
    /// Always loadable with the M4 baseline feature set.
    Always,
    /// Requires the `whisper` Cargo feature on `audiopipe`. Off in the
    /// Phase-2 baseline; re-enabled in T8 once `whisper-rs` is dropped
    /// from `inference-core`.
    AudiopipeWhisperFeature,
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
    ///
    /// Special-cased: for [`LanguageCoverage::Multilingual99`] the list is
    /// the single placeholder `"multilingual-99"`, and the wizard expands
    /// it to a curated subset.
    pub languages: &'static [&'static str],
    /// Whether this entry is the default pick for a fresh install.
    pub default: bool,
    /// Cargo-feature gate, if any.
    pub feature_requirement: FeatureRequirement,
}

// Language lists pinned per audiopipe spec §3 catalog v2. Kept as
// module-level constants so the [`Metadata`] entries can reference them by
// `&'static [&'static str]`.

const PARAKEET_LANGUAGES: &[&str] = &[
    "en", "it", "de", "fr", "es", "pt", "nl", "pl", "ru", "uk", "cs", "hr",
    "bg", "da", "el", "et", "fi", "hu", "lv", "lt", "mt", "ro", "sk", "sl",
    "sv",
];

const QWEN3_LANGUAGES: &[&str] = &[
    "zh", "en", "yue", "ar", "de", "fr", "es", "pt", "id", "it", "ko", "ru",
    "th", "vi", "ja", "tr", "hi", "ms", "nl", "sv", "da", "fi", "pl", "cs",
    "fil", "fa", "el", "hu", "mk", "ro",
];

const WHISPER_LANGUAGES: &[&str] = &["multilingual-99"];

const PARAKEET_V3: Metadata = Metadata {
    id: "parakeet-tdt-0.6b-v3",
    display_name: "Parakeet TDT v3",
    size_bytes: 600_000_000,
    language_coverage: LanguageCoverage::European25,
    summary: "25 European languages. Lowest latency.",
    license: "CC-BY-4.0",
    languages: PARAKEET_LANGUAGES,
    default: true,
    feature_requirement: FeatureRequirement::Always,
};

const QWEN3_ASR: Metadata = Metadata {
    id: "qwen3-asr-0.6b-ggml",
    display_name: "Qwen3-ASR (broad languages)",
    size_bytes: 700_000_000,
    language_coverage: LanguageCoverage::Global30,
    summary: "30 languages with broad Asian, Arabic, and European coverage.",
    license: "Apache-2.0",
    languages: QWEN3_LANGUAGES,
    default: false,
    feature_requirement: FeatureRequirement::Always,
};

const WHISPER_LARGE_V3_TURBO: Metadata = Metadata {
    id: "whisper-large-v3-turbo",
    display_name: "Whisper large-v3-turbo (other languages)",
    size_bytes: 1_500_000_000,
    language_coverage: LanguageCoverage::Multilingual99,
    summary: "99 languages. Slower but broadest coverage.",
    license: "MIT/Apache-2.0",
    languages: WHISPER_LANGUAGES,
    default: false,
    feature_requirement: FeatureRequirement::AudiopipeWhisperFeature,
};

const MODELS: &[Metadata] = &[PARAKEET_V3, QWEN3_ASR, WHISPER_LARGE_V3_TURBO];

/// Catalog id of the model used when the user hasn't picked one.
#[must_use]
pub const fn default_model_id() -> &'static str {
    PARAKEET_V3.id
}

/// Full catalog, in display order (default first).
#[must_use]
pub fn list_models() -> &'static [Metadata] {
    MODELS
}

/// Metadata lookup by catalog id. Returns `None` for unknown ids — the
/// caller decides whether that's a hard error (settings carry a stale id
/// from a future build) or a soft fallback to [`default_model_id`].
#[must_use]
pub fn model_metadata(id: &str) -> Option<&'static Metadata> {
    MODELS.iter().find(|m| m.id == id)
}

/// Resolve a catalog id to the actual audiopipe model name to load.
///
/// On Apple Silicon (`target_os = "macos"` + `target_arch = "aarch64"`),
/// when the `audiopipe-mlx` Cargo feature is on, `parakeet-tdt-0.6b-v3`
/// is silently upgraded to `parakeet-tdt-0.6b-v3-mlx` so MLX acceleration
/// kicks in transparently. With the feature off (current default —
/// upstream MLX doesn't build on Xcode 17) the id passes through and the
/// loader uses the ONNX engine with CoreML execution provider, which is
/// still hardware-accelerated on Apple Silicon. Other ids pass through.
#[must_use]
pub fn audiopipe_name_for_platform(id: &str) -> &str {
    if id == PARAKEET_V3.id
        && cfg!(all(target_os = "macos", target_arch = "aarch64"))
        && cfg!(feature = "audiopipe-mlx")
    {
        return "parakeet-tdt-0.6b-v3-mlx";
    }
    id
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
    fn audiopipe_name_passthrough_for_non_parakeet() {
        assert_eq!(audiopipe_name_for_platform(QWEN3_ASR.id), QWEN3_ASR.id);
        assert_eq!(
            audiopipe_name_for_platform(WHISPER_LARGE_V3_TURBO.id),
            WHISPER_LARGE_V3_TURBO.id
        );
    }

    #[test]
    #[cfg(all(target_os = "macos", target_arch = "aarch64", feature = "audiopipe-mlx"))]
    fn parakeet_upgrades_to_mlx_on_apple_silicon() {
        assert_eq!(
            audiopipe_name_for_platform(PARAKEET_V3.id),
            "parakeet-tdt-0.6b-v3-mlx"
        );
    }

    #[test]
    #[cfg(not(all(target_os = "macos", target_arch = "aarch64", feature = "audiopipe-mlx")))]
    fn parakeet_passthrough_without_mlx_feature() {
        assert_eq!(audiopipe_name_for_platform(PARAKEET_V3.id), PARAKEET_V3.id);
    }

    #[test]
    fn parakeet_language_list_matches_spec() {
        // Spec §3 catalog v2 pins exactly 25 ISO codes for Parakeet TDT v3.
        assert_eq!(PARAKEET_V3.languages.len(), 25);
        assert!(PARAKEET_V3.languages.contains(&"it"));
        assert!(PARAKEET_V3.languages.contains(&"en"));
    }

    #[test]
    fn qwen3_language_list_matches_spec() {
        // Spec §3 catalog v2 pins exactly 30 ISO codes for Qwen3-ASR.
        assert_eq!(QWEN3_ASR.languages.len(), 30);
        assert!(QWEN3_ASR.languages.contains(&"zh"));
        assert!(QWEN3_ASR.languages.contains(&"ja"));
    }

    #[test]
    fn whisper_language_list_uses_multilingual_placeholder() {
        // Whisper covers ~99 languages; the wizard expands the placeholder
        // to a curated subset rather than dumping the full list. Backend
        // just stores the marker.
        assert_eq!(WHISPER_LARGE_V3_TURBO.languages, &["multilingual-99"]);
    }
}
