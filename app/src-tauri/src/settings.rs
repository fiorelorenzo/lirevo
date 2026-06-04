use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use tauri_plugin_store::StoreExt;

use crate::error::AppError;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Hotkey {
    #[default]
    RightOption,
    LeftOption,
    RightCommand,
    Fn,
    F5,
}

/// Bump when introducing a new one-shot migration in [`Settings::migrate`].
/// Existing `settings.json` files written before the bump carry a lower
/// `schema_version` (or none at all → 0) and the migration runs once.
const SCHEMA_VERSION: u32 = 3;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// M4: catalog id of the STT model to load. `None` falls back to
    /// [`crate::stt::catalog::default_model_id`]. Replaces the pre-M4
    /// `whisper_model_path` field, which is kept in the struct for
    /// backward compatibility with older `settings.json` files but is
    /// no longer read by the loader.
    pub stt_model_id: Option<String>,
    /// Legacy: ggml whisper model path. Pre-M4 loader used this; M4
    /// loader uses [`Settings::stt_model_id`] instead. Retained so
    /// older settings files deserialize cleanly.
    pub whisper_model_path: Option<PathBuf>,
    pub llm_model_path: Option<PathBuf>,
    pub llm_ctx_size: u32,
    pub whisper_coreml_disable: bool,

    pub hotkey: Hotkey,
    pub language: String,
    /// System input device to capture from. `None` = system default.
    pub input_device_name: Option<String>,
    pub force_pasteboard: bool,
    pub paste_delay_ms: u32,

    pub launch_at_login: bool,
    /// Start the app without opening any window. Combined with
    /// `launch_at_login`, this gives a true background-only experience —
    /// the tray icon is the only UI affordance until the user clicks it.
    /// Has no effect on subsequent re-launches once a window is already
    /// open. Defaults to `false` (visible window at startup) so first-run
    /// users always see the wizard / home.
    #[serde(default)]
    pub launch_minimized: bool,
    /// Hide the main window on close instead of destroying it; keep the
    /// app running in the tray so the hotkey stays live. The macOS
    /// convention for menu-bar-first apps. Defaults to `true`.
    #[serde(default = "default_true")]
    pub stay_running_on_window_close: bool,
    /// After load_models succeeds, run a single tiny warm-up inference on
    /// each loaded backend so the first real dictation doesn't pay the
    /// one-time GPU-kernel-compile + KV-cache-allocate cost. Trades a few
    /// hundred ms at startup for snappier first hotkey press. Defaults to
    /// `true`.
    #[serde(default = "default_true")]
    pub keep_models_warm: bool,
    /// Keep a local history of dictations on this device. Gates whether the
    /// dictation pipeline persists transcripts to the on-device history store
    /// (the capture itself lands in a later change). Defaults to `true` so
    /// existing installs upgrading without the key opt in by default.
    #[serde(default = "default_true")]
    pub record_history: bool,
    /// Energy-profile selection mode. `"auto"` lets the [`ProfileSelector`]
    /// decide; `"power_saver"` / `"balanced"` / `"performance"` pin a profile.
    /// Parsed by `inference_core::profile::mode_from_str`. Defaults to
    /// `"auto"`.
    #[serde(default = "default_profile_mode")]
    pub profile_mode: String,
    pub ui_language: String,
    pub onboarding_complete: bool,
    pub app_version: String,
    /// Persisted schema version — see [`SCHEMA_VERSION`]. Defaults to 0 for
    /// settings.json files written before this field existed.
    #[serde(default)]
    pub schema_version: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            stt_model_id: None,
            whisper_model_path: None,
            llm_model_path: None,
            llm_ctx_size: 4096,
            whisper_coreml_disable: false,
            hotkey: Hotkey::default(),
            language: default_dictation_language(),
            input_device_name: None,
            force_pasteboard: false,
            paste_delay_ms: 120,
            launch_at_login: false,
            launch_minimized: false,
            stay_running_on_window_close: true,
            keep_models_warm: true,
            record_history: true,
            profile_mode: default_profile_mode(),
            ui_language: "en".into(),
            onboarding_complete: false,
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            schema_version: SCHEMA_VERSION,
        }
    }
}

fn default_true() -> bool { true }

fn default_profile_mode() -> String { "auto".into() }

/// Default dictation language derived from the OS locale (e.g. `it-IT` →
/// `it`). Whisper auto-detect on very short utterances often hallucinates
/// English fillers like "Thank you" — pinning to the user's actual locale
/// from first launch avoids that for the 99% case. Falls back to `auto` if
/// we can't read the locale or it isn't in the catalog.
fn default_dictation_language() -> String {
    const SUPPORTED: &[&str] = &["en", "it", "fr", "de", "es"];
    let locale = sys_locale::get_locale().unwrap_or_default();
    let primary = locale
        .split(['-', '_'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if SUPPORTED.contains(&primary.as_str()) {
        primary
    } else {
        "auto".into()
    }
}

const STORE_FILE: &str = "settings.json";
const STORE_KEY: &str = "settings";

impl Settings {
    pub fn load(app: &tauri::AppHandle) -> Result<Self, AppError> {
        let store = app.store(STORE_FILE)
            .map_err(|e| AppError::Settings(e.to_string()))?;
        let s = if let Some(value) = store.get(STORE_KEY) {
            match serde_json::from_value::<Settings>(value.clone()) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(?e, "settings.json corrupt, resetting to defaults");
                    let defaults = Settings::default();
                    defaults.persist(app)?;
                    defaults
                }
            }
        } else {
            let defaults = Settings::default();
            defaults.persist(app)?;
            defaults
        };
        let mut migrated = s;
        let mut dirty = migrated.migrate();
        // Clear stale model paths that point to files which no longer exist on
        // disk. Otherwise the UI shows a picker with a path to nothing and
        // load_models surfaces a confusing "all configured models failed to
        // load" — happens when the user deletes a model from the data folder
        // or switches between dev / release builds whose models dirs differ.
        if let Some(p) = &migrated.whisper_model_path {
            if !p.exists() {
                tracing::warn!(path = %p.display(), "whisper_model_path missing on disk — clearing");
                migrated.whisper_model_path = None;
                dirty = true;
            }
        }
        if let Some(p) = &migrated.llm_model_path {
            if !p.exists() {
                tracing::warn!(path = %p.display(), "llm_model_path missing on disk — clearing");
                migrated.llm_model_path = None;
                dirty = true;
            }
        }
        if dirty {
            migrated.persist(app)?;
        }
        Ok(migrated)
    }

    pub fn persist(&self, app: &tauri::AppHandle) -> Result<(), AppError> {
        let store = app.store(STORE_FILE)
            .map_err(|e| AppError::Settings(e.to_string()))?;
        store.set(STORE_KEY, serde_json::to_value(self)?);
        store.save().map_err(|e| AppError::Settings(e.to_string()))?;
        Ok(())
    }

    pub fn merge_patch(&mut self, patch: &serde_json::Value) -> Result<(), AppError> {
        let mut current = serde_json::to_value(&*self)?;
        let obj = current.as_object_mut()
            .ok_or_else(|| AppError::Settings("internal: settings not an object".into()))?;
        let patch_obj = patch.as_object()
            .ok_or_else(|| AppError::Settings("patch must be a JSON object".into()))?;
        for (k, v) in patch_obj {
            obj.insert(k.clone(), v.clone());
        }
        let merged: Settings = serde_json::from_value(current)
            .map_err(|e| AppError::Settings(format!("invalid patch: {e}")))?;
        merged.validate()?;
        *self = merged;
        Ok(())
    }

    pub fn validate(&self) -> Result<(), AppError> {
        if !(512..=32768).contains(&self.llm_ctx_size) {
            return Err(AppError::Settings(format!(
                "llmCtxSize {} out of range (512..32768)", self.llm_ctx_size
            )));
        }
        if self.paste_delay_ms > 2000 {
            return Err(AppError::Settings(format!(
                "pasteDelayMs {} out of range (0..2000)", self.paste_delay_ms
            )));
        }
        Ok(())
    }

    pub fn env_affecting_diff(before: &Self, after: &Self) -> bool {
        before.stt_model_id != after.stt_model_id
            || before.whisper_model_path != after.whisper_model_path
            || before.llm_model_path != after.llm_model_path
            || before.llm_ctx_size != after.llm_ctx_size
            || before.whisper_coreml_disable != after.whisper_coreml_disable
    }

    /// One-shot upgrades for settings.json files written by older versions of
    /// the app. Returns `true` if anything changed and the caller should
    /// re-persist. Always refreshes `app_version` to the running binary's
    /// version, but does not flag that alone as dirty.
    fn migrate(&mut self) -> bool {
        self.app_version = env!("CARGO_PKG_VERSION").to_string();

        let mut dirty = false;
        if self.schema_version < 1 {
            // `language: "auto"` was the hardcoded default before
            // `default_dictation_language()` existed. Pre-existing installs
            // were stuck on it even after we started deriving from the OS
            // locale. Re-derive once on first launch after the upgrade.
            if self.language == "auto" {
                let derived = default_dictation_language();
                if derived != self.language {
                    tracing::info!(from = %self.language, to = %derived, "migrating dictation language from OS locale");
                    self.language = derived;
                    dirty = true;
                }
            }
        }
        if self.schema_version < 3 {
            // M4: introduce `stt_model_id`. We deliberately leave it `None`
            // for users with a pre-M4 `whisper_model_path` configured — the
            // wizard's new model picker (see M4 plan Phase 4) is the right
            // place to ask which audiopipe model they want, rather than
            // silently mapping their old ggml path to a different model.
            // `None` means "use the catalog default" at load time.
            if self.stt_model_id.is_none() {
                tracing::info!(
                    "M4 migration: stt_model_id left unset; loader will use the catalog default"
                );
            }
        }
        if self.schema_version != SCHEMA_VERSION {
            self.schema_version = SCHEMA_VERSION;
            dirty = true;
        }
        dirty
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_match_spec() {
        let s = Settings::default();
        assert_eq!(s.hotkey, Hotkey::RightOption);
        // `language` is derived from the host OS locale at first run, so
        // its concrete value is environment-dependent. All we can pin is
        // that it lands in the catalog of supported codes.
        assert!(["auto", "en", "it", "fr", "de", "es"].contains(&s.language.as_str()));
        assert_eq!(s.llm_ctx_size, 4096);
        assert_eq!(s.paste_delay_ms, 120);
        assert!(!s.onboarding_complete);
        assert!(!s.force_pasteboard);
        // Background-mode defaults: warm + stay-on-close ON, launch-minimized OFF
        // so first-run still sees the wizard.
        assert!(s.keep_models_warm);
        assert!(s.stay_running_on_window_close);
        assert!(!s.launch_minimized);
        assert!(s.record_history);
        assert_eq!(s.profile_mode, "auto");
    }

    #[test]
    fn legacy_settings_without_new_fields_get_correct_defaults() {
        // Simulates a v1 settings.json from before this change: missing the
        // three new fields. Serde defaults must populate them so the user
        // doesn't lose the keep-warm / stay-running behaviors on upgrade.
        let legacy = json!({
            "whisperModelPath": null,
            "llmModelPath": null,
            "llmCtxSize": 4096,
            "whisperCoreMLDisable": false,
            "hotkey": "right-option",
            "language": "en",
            "inputDeviceName": null,
            "forcePasteboard": false,
            "pasteDelayMs": 120,
            "launchAtLogin": false,
            "uiLanguage": "en",
            "onboardingComplete": true,
            "appVersion": "0.4.0",
            "schemaVersion": 1,
        });
        let s: Settings = serde_json::from_value(legacy).unwrap();
        assert!(s.keep_models_warm);
        assert!(s.stay_running_on_window_close);
        assert!(!s.launch_minimized);
        assert!(s.record_history);
        assert_eq!(s.profile_mode, "auto");
    }

    #[test]
    // Multiple sequential reassignments to exercise validation thresholds;
    // a single struct expression would obscure intent.
    #[allow(clippy::field_reassign_with_default)]
    fn validate_rejects_out_of_range() {
        let mut s = Settings::default();
        s.llm_ctx_size = 100;
        assert!(s.validate().is_err());
        s.llm_ctx_size = 99999;
        assert!(s.validate().is_err());
        s.llm_ctx_size = 4096;
        s.paste_delay_ms = 5000;
        assert!(s.validate().is_err());
        s.paste_delay_ms = 120;
        assert!(s.validate().is_ok());
    }

    #[test]
    fn merge_patch_applies_partial() {
        let mut s = Settings::default();
        s.merge_patch(&json!({ "language": "it", "hotkey": "f5" })).unwrap();
        assert_eq!(s.language, "it");
        assert_eq!(s.hotkey, Hotkey::F5);
        assert_eq!(s.llm_ctx_size, 4096);
    }

    #[test]
    fn merge_patch_rejects_invalid_value() {
        let mut s = Settings::default();
        let result = s.merge_patch(&json!({ "llmCtxSize": 100 }));
        assert!(result.is_err());
    }

    #[test]
    fn env_affecting_diff_detects_path_change() {
        let before = Settings::default();
        let mut after = before.clone();
        after.whisper_model_path = Some("/new/path.bin".into());
        assert!(Settings::env_affecting_diff(&before, &after));
    }

    #[test]
    fn migrate_v0_auto_language_rederives_from_locale() {
        // Simulate a settings.json from before SCHEMA_VERSION was introduced.
        let mut s = Settings {
            schema_version: 0,
            language: "auto".into(),
            ..Settings::default()
        };
        let dirty = s.migrate();
        assert_eq!(s.schema_version, SCHEMA_VERSION);
        // If the host locale derives to a supported code, the migration
        // should have upgraded "auto" → "<code>"; if not, the value stays
        // "auto" but the version still bumps.
        let derived = default_dictation_language();
        if derived == "auto" {
            assert_eq!(s.language, "auto");
        } else {
            assert_eq!(s.language, derived);
        }
        assert!(dirty, "version bump alone should flag the settings as dirty");
    }

    #[test]
    fn migrate_v1_leaves_explicit_auto_alone() {
        // A user on the current schema who explicitly picked "auto" must
        // not have it silently overwritten on subsequent loads.
        let mut s = Settings {
            schema_version: SCHEMA_VERSION,
            language: "auto".into(),
            ..Settings::default()
        };
        let dirty = s.migrate();
        assert_eq!(s.language, "auto");
        assert!(!dirty, "no-op migration must not flag dirty");
    }

    #[test]
    fn env_affecting_diff_ignores_non_env() {
        let before = Settings::default();
        let mut after = before.clone();
        after.hotkey = Hotkey::F5;
        after.language = "it".into();
        assert!(!Settings::env_affecting_diff(&before, &after));
    }
}
