//! Hardcoded catalog of STT models supported by the M4 audiopipe pipeline.
//!
//! Three entries, matching the M4 spec §3:
//!   1. `parakeet-tdt-0.6b-v3` — default, 25 EU languages incl. IT, ~600 MB.
//!      On Apple Silicon the loader silently prefers the `-mlx` variant for
//!      MLX-accelerated inference; that's the routing concern of
//!      [`audiopipe_name_for_platform`], not the catalog id itself.
//!   2. `qwen3-asr-0.6b-ggml` — opt-in, 30 langs + 22 Chinese dialects.
//!   3. `whisper-large-v3-turbo` — fallback, 99 langs. **Requires the
//!      `whisper` Cargo feature on `audiopipe`**, which is OFF in the M4
//!      Phase-2 baseline (it gets re-enabled in Task 8 once `whisper-rs` is
//!      gone from `inference-core` and the native-library conflict clears).

use serde::Serialize;

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LanguageCoverage {
    /// 25 European languages.
    European25,
    /// 30 global languages (incl. CJK / Arabic / Hindi).
    Global30,
    /// Whisper-style ~99-language coverage.
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
    /// Approximate on-disk size in MiB. Used by the wizard for the
    /// "this will download N MB" copy; the canonical size comes from the
    /// HF cache at fetch time.
    pub size_mib: u32,
    /// What language family this model covers.
    pub language_coverage: LanguageCoverage,
    /// Whether this entry is the default pick for a fresh install.
    pub default: bool,
    /// Cargo-feature gate, if any.
    pub feature_requirement: FeatureRequirement,
}

const PARAKEET_V3: Metadata = Metadata {
    id: "parakeet-tdt-0.6b-v3",
    display_name: "Parakeet TDT v3",
    size_mib: 600,
    language_coverage: LanguageCoverage::European25,
    default: true,
    feature_requirement: FeatureRequirement::Always,
};

const QWEN3_ASR: Metadata = Metadata {
    id: "qwen3-asr-0.6b-ggml",
    display_name: "Qwen3-ASR (broad languages)",
    size_mib: 600,
    language_coverage: LanguageCoverage::Global30,
    default: false,
    feature_requirement: FeatureRequirement::Always,
};

const WHISPER_LARGE_V3_TURBO: Metadata = Metadata {
    id: "whisper-large-v3-turbo",
    display_name: "Whisper large-v3-turbo (other languages)",
    size_mib: 1_500,
    language_coverage: LanguageCoverage::Multilingual99,
    default: false,
    feature_requirement: FeatureRequirement::AudiopipeWhisperFeature,
};

const MODELS: &[Metadata] = &[PARAKEET_V3, QWEN3_ASR, WHISPER_LARGE_V3_TURBO];

/// Catalog id of the model used when the user hasn't picked one.
#[must_use]
pub const fn default_model_id() -> &'static str {
    PARAKEET_V3.id
}

/// Full catalog, in display order (default first). Consumed by the wizard
/// model-picker in M4 Phase 4-6; allowed dead-code in the meantime so the
/// public surface stays stable.
#[must_use]
#[allow(dead_code)]
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
/// On Apple Silicon (`target_os = "macos"` + `target_arch = "aarch64"`)
/// `parakeet-tdt-0.6b-v3` is silently upgraded to `parakeet-tdt-0.6b-v3-mlx`
/// so MLX acceleration kicks in transparently. Elsewhere it stays on the
/// ONNX path. Other ids pass through unchanged.
#[must_use]
pub fn audiopipe_name_for_platform(id: &str) -> &str {
    if id == PARAKEET_V3.id && cfg!(all(target_os = "macos", target_arch = "aarch64")) {
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
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    fn parakeet_upgrades_to_mlx_on_apple_silicon() {
        assert_eq!(
            audiopipe_name_for_platform(PARAKEET_V3.id),
            "parakeet-tdt-0.6b-v3-mlx"
        );
    }

    #[test]
    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    fn parakeet_passthrough_off_apple_silicon() {
        assert_eq!(audiopipe_name_for_platform(PARAKEET_V3.id), PARAKEET_V3.id);
    }
}
