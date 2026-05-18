use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use tauri_plugin_store::StoreExt;

use crate::error::AppError;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Hotkey {
    RightOption,
    LeftOption,
    RightCommand,
    Fn,
    F5,
}

impl Default for Hotkey {
    fn default() -> Self { Hotkey::RightOption }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub whisper_model_path: Option<PathBuf>,
    pub llm_model_path: Option<PathBuf>,
    pub llm_ctx_size: u32,
    pub whisper_coreml_disable: bool,

    pub hotkey: Hotkey,
    pub language: String,
    pub force_pasteboard: bool,
    pub paste_delay_ms: u32,

    pub launch_at_login: bool,
    pub ui_language: String,
    pub onboarding_complete: bool,
    pub app_version: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            whisper_model_path: None,
            llm_model_path: None,
            llm_ctx_size: 4096,
            whisper_coreml_disable: false,
            hotkey: Hotkey::default(),
            language: "auto".into(),
            force_pasteboard: false,
            paste_delay_ms: 120,
            launch_at_login: false,
            ui_language: "en".into(),
            onboarding_complete: false,
            app_version: env!("CARGO_PKG_VERSION").to_string(),
        }
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
        Ok(s.migrate())
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
        before.whisper_model_path != after.whisper_model_path
            || before.llm_model_path != after.llm_model_path
            || before.llm_ctx_size != after.llm_ctx_size
            || before.whisper_coreml_disable != after.whisper_coreml_disable
    }

    fn migrate(mut self) -> Self {
        // M3 baseline = v1, no transformations. Bump version on every load.
        self.app_version = env!("CARGO_PKG_VERSION").to_string();
        self
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
        assert_eq!(s.language, "auto");
        assert_eq!(s.llm_ctx_size, 4096);
        assert_eq!(s.paste_delay_ms, 120);
        assert!(!s.onboarding_complete);
        assert!(!s.force_pasteboard);
    }

    #[test]
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
    fn env_affecting_diff_ignores_non_env() {
        let before = Settings::default();
        let mut after = before.clone();
        after.hotkey = Hotkey::F5;
        after.language = "it".into();
        assert!(!Settings::env_affecting_diff(&before, &after));
    }
}
