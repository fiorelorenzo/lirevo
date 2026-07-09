use os_integration::{ActivationMode, HotkeySpec, Modifier, ModifierFlags, Side, Trigger};
use serde::{Deserialize, Serialize};
use tauri_plugin_store::StoreExt;

use crate::error::AppError;

/// Bump when introducing a new one-shot migration in [`Settings::migrate`].
/// Existing `settings.json` files written before the bump carry a lower
/// `schema_version` (or none at all → 0) and the migration runs once.
const SCHEMA_VERSION: u32 = 5;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub llm_ctx_size: u32,

    pub hotkey: HotkeySpec,
    #[serde(default)]
    pub activation_mode: ActivationMode,
    /// Legacy pre-capture hotkey string (`"right-option"` … `"f5"`). Read once by
    /// the migration, then left `None`. Retained so old settings.json files
    /// deserialize cleanly into the new model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_hotkey: Option<String>,
    pub language: String,
    /// System input device to capture from. `None` = system default.
    pub input_device_name: Option<String>,
    /// Keep Bluetooth audio in stereo: when audio is playing through a
    /// Bluetooth output and the dictation mic would be a Bluetooth device,
    /// dictate with the built-in mic instead so the output stays in A2DP
    /// stereo (opening a Bluetooth mic forces the whole device to mono HFP).
    /// Defaults to `true` so existing installs opt in on upgrade.
    #[serde(default = "default_true")]
    pub smart_mic_routing: bool,
    /// Microphone that smart routing falls back to when it reroutes (a Bluetooth
    /// output is playing and the configured mic is Bluetooth). `None` = auto
    /// (the built-in mic, the sensible default).
    #[serde(default)]
    pub backup_input_device: Option<String>,
    pub paste_delay_ms: u32,

    pub launch_at_login: bool,
    /// When true, the app starts silently in the menu bar without opening the
    /// main window. The tray icon is always present; the user re-opens the
    /// window from it. Ignored when `onboarding_complete` is false.
    pub start_minimized: bool,
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
            llm_ctx_size: 4096,
            hotkey: HotkeySpec {
                modifiers: ModifierFlags::default(),
                trigger: Trigger::ModifierOnly {
                    modifier: Modifier::Option,
                    side: Side::Right,
                },
            },
            activation_mode: ActivationMode::Hold,
            legacy_hotkey: None,
            language: default_dictation_language(),
            input_device_name: None,
            smart_mic_routing: true,
            backup_input_device: None,
            paste_delay_ms: 120,
            launch_at_login: false,
            start_minimized: false,
            record_history: true,
            profile_mode: default_profile_mode(),
            ui_language: "en".into(),
            onboarding_complete: false,
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            schema_version: SCHEMA_VERSION,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_profile_mode() -> String {
    "auto".into()
}

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

/// Absolute path to settings.json inside our app-name data dir (so it lives
/// alongside the db + models, and stays separate for dev vs prod), rather than
/// the store plugin's default bundle-id config dir.
fn store_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, AppError> {
    Ok(crate::paths::data_dir(app)
        .map_err(|e| AppError::Settings(e.to_string()))?
        .join(STORE_FILE))
}

impl Settings {
    pub fn load(app: &tauri::AppHandle) -> Result<Self, AppError> {
        let store = app
            .store(store_path(app)?)
            .map_err(|e| AppError::Settings(e.to_string()))?;
        let s = if let Some(value) = store.get(STORE_KEY) {
            let mut raw = value.clone();
            if let Some(obj) = raw.as_object_mut() {
                if obj.get("hotkey").is_some_and(serde_json::Value::is_string) {
                    let legacy = obj.remove("hotkey").unwrap();
                    obj.insert("legacyHotkey".into(), legacy);
                }
            }
            match serde_json::from_value::<Settings>(raw) {
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
        let dirty = migrated.migrate();
        if dirty {
            migrated.persist(app)?;
        }
        Ok(migrated)
    }

    pub fn persist(&self, app: &tauri::AppHandle) -> Result<(), AppError> {
        let store = app
            .store(store_path(app)?)
            .map_err(|e| AppError::Settings(e.to_string()))?;
        store.set(STORE_KEY, serde_json::to_value(self)?);
        store
            .save()
            .map_err(|e| AppError::Settings(e.to_string()))?;
        Ok(())
    }

    pub fn merge_patch(&mut self, patch: &serde_json::Value) -> Result<(), AppError> {
        let mut current = serde_json::to_value(&*self)?;
        let obj = current
            .as_object_mut()
            .ok_or_else(|| AppError::Settings("internal: settings not an object".into()))?;
        let patch_obj = patch
            .as_object()
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
                "llmCtxSize {} out of range (512..32768)",
                self.llm_ctx_size
            )));
        }
        if self.paste_delay_ms > 2000 {
            return Err(AppError::Settings(format!(
                "pasteDelayMs {} out of range (0..2000)",
                self.paste_delay_ms
            )));
        }
        validate_hotkey(&self.hotkey)?;
        Ok(())
    }

    pub fn env_affecting_diff(before: &Self, after: &Self) -> bool {
        before.llm_ctx_size != after.llm_ctx_size
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
        if self.schema_version < 4 {
            // Pre-capture builds stored `hotkey` as a kebab string; serde parks it
            // under `legacy_hotkey` (see load()'s pre-pass). Map it once.
            if let Some(legacy) = self.legacy_hotkey.take() {
                let trigger = match legacy.as_str() {
                    "left-option" => Trigger::ModifierOnly {
                        modifier: Modifier::Option,
                        side: Side::Left,
                    },
                    "right-command" => Trigger::ModifierOnly {
                        modifier: Modifier::Command,
                        side: Side::Right,
                    },
                    "fn" => Trigger::Fn,
                    "f5" => Trigger::Key("F5".into()),
                    _ => Trigger::ModifierOnly {
                        modifier: Modifier::Option,
                        side: Side::Right,
                    },
                };
                self.hotkey = HotkeySpec {
                    modifiers: ModifierFlags::default(),
                    trigger,
                };
                dirty = true;
            }
        }
        if self.schema_version != SCHEMA_VERSION {
            self.schema_version = SCHEMA_VERSION;
            dirty = true;
        }
        dirty
    }
}

fn validate_hotkey(spec: &HotkeySpec) -> Result<(), AppError> {
    // trigger always contributes one "key"; plus held modifiers.
    if spec.modifiers.count() + 1 > 3 {
        return Err(AppError::Settings("hotkey uses more than 3 keys".into()));
    }
    if let Trigger::Key(name) = &spec.trigger {
        let bare_alnum = name.len() == 1 && name.chars().all(|c| c.is_ascii_alphanumeric());
        if bare_alnum && spec.modifiers.count() == 0 {
            return Err(AppError::Settings(
                "a plain letter/number needs a modifier".into(),
            ));
        }
        if os_integration::hotkey_spec::key_to_macos_keycode(name).is_none() {
            return Err(AppError::Settings(format!("unknown key: {name}")));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_match_spec() {
        let s = Settings::default();
        assert_eq!(
            s.hotkey.trigger,
            Trigger::ModifierOnly {
                modifier: Modifier::Option,
                side: Side::Right
            }
        );
        assert_eq!(s.activation_mode, ActivationMode::Hold);
        // `language` is derived from the host OS locale at first run, so
        // its concrete value is environment-dependent. All we can pin is
        // that it lands in the catalog of supported codes.
        assert!(["auto", "en", "it", "fr", "de", "es"].contains(&s.language.as_str()));
        assert_eq!(s.llm_ctx_size, 4096);
        assert_eq!(s.paste_delay_ms, 120);
        assert!(!s.onboarding_complete);
        assert!(s.record_history);
        assert!(s.smart_mic_routing);
        assert_eq!(s.profile_mode, "auto");
    }

    #[test]
    fn legacy_settings_without_new_fields_get_correct_defaults() {
        // Simulates a v1 settings.json from before this change: missing the
        // new fields. Serde defaults must populate them so the user doesn't
        // lose the history behavior on upgrade.
        let legacy = json!({
            "llmCtxSize": 4096,
            "legacyHotkey": "right-option",
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
        assert!(s.record_history);
        assert!(s.smart_mic_routing);
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
        s.merge_patch(&json!({
            "language": "it",
            "hotkey": { "modifiers": {}, "trigger": { "key": "F5" } }
        }))
        .unwrap();
        assert_eq!(s.language, "it");
        assert_eq!(s.hotkey.trigger, Trigger::Key("F5".into()));
        assert_eq!(s.llm_ctx_size, 4096);
    }

    #[test]
    fn merge_patch_rejects_invalid_value() {
        let mut s = Settings::default();
        let result = s.merge_patch(&json!({ "llmCtxSize": 100 }));
        assert!(result.is_err());
    }

    #[test]
    fn env_affecting_diff_detects_ctx_change() {
        let before = Settings::default();
        let mut after = before.clone();
        after.llm_ctx_size = 8192;
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
        assert!(
            dirty,
            "version bump alone should flag the settings as dirty"
        );
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
    fn migrate_legacy_hotkey_strings_to_specs() {
        let cases = [
            (
                "right-option",
                Trigger::ModifierOnly {
                    modifier: Modifier::Option,
                    side: Side::Right,
                },
            ),
            (
                "left-option",
                Trigger::ModifierOnly {
                    modifier: Modifier::Option,
                    side: Side::Left,
                },
            ),
            (
                "right-command",
                Trigger::ModifierOnly {
                    modifier: Modifier::Command,
                    side: Side::Right,
                },
            ),
            ("fn", Trigger::Fn),
            ("f5", Trigger::Key("F5".into())),
        ];
        for (legacy, expected) in cases {
            let mut s = Settings {
                schema_version: 3,
                legacy_hotkey: Some(legacy.into()),
                ..Settings::default()
            };
            let dirty = s.migrate();
            assert_eq!(
                s.hotkey.trigger, expected,
                "legacy {legacy} should map to its spec"
            );
            assert!(
                s.legacy_hotkey.is_none(),
                "legacy {legacy} should be consumed"
            );
            assert_eq!(s.schema_version, SCHEMA_VERSION);
            assert!(dirty, "legacy {legacy} migration should flag dirty");
        }
    }

    #[test]
    fn load_pre_pass_maps_string_hotkey_then_migrates() {
        // End-to-end: an old settings.json with a string `hotkey` field. The
        // load() pre-pass parks it under `legacyHotkey`; migrate() then maps it.
        let mut raw = json!({
            "schemaVersion": 3,
            "hotkey": "left-option",
        });
        if let Some(obj) = raw.as_object_mut() {
            if obj.get("hotkey").is_some_and(serde_json::Value::is_string) {
                let legacy = obj.remove("hotkey").unwrap();
                obj.insert("legacyHotkey".into(), legacy);
            }
        }
        let mut s: Settings = serde_json::from_value(raw).unwrap();
        s.migrate();
        assert_eq!(
            s.hotkey.trigger,
            Trigger::ModifierOnly {
                modifier: Modifier::Option,
                side: Side::Left
            }
        );
        assert!(s.legacy_hotkey.is_none());
    }

    #[test]
    fn deserializes_settings_json_containing_removed_model_selection_keys() {
        // An old settings.json written before the fixed-model-catalog change
        // still has the now-removed model-selection keys on disk (serde
        // config doesn't rewrite files it hasn't loaded yet). `Settings` no
        // longer declares these fields, so this locks in that serde's
        // default "ignore unknown fields" behavior lets such a file
        // deserialize cleanly — a future `#[serde(deny_unknown_fields)]`
        // would silently break upgrades for every existing install.
        let legacy = json!({
            "sttModelId": "parakeet-tdt-0.6b-v3",
            "whisperModelPath": "/Users/example/Library/Application Support/Lirevo/models/ggml-base.bin",
            "llmModelPath": "/Users/example/Library/Application Support/Lirevo/models/old-llm.gguf",
            "whisperCoreMLDisable": false,
            "schemaVersion": 4,
            "language": "it",
            "llmCtxSize": 8192,
        });
        let s: Settings =
            serde_json::from_value(legacy).expect("removed keys must not break deserialization");
        assert_eq!(s.language, "it");
        assert_eq!(s.llm_ctx_size, 8192);
    }

    #[test]
    fn env_affecting_diff_ignores_non_env() {
        let before = Settings::default();
        let mut after = before.clone();
        after.hotkey = HotkeySpec {
            modifiers: ModifierFlags::default(),
            trigger: Trigger::Key("F5".into()),
        };
        after.language = "it".into();
        assert!(!Settings::env_affecting_diff(&before, &after));
    }
}
