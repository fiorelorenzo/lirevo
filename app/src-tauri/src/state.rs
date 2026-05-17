use std::sync::{Arc, Mutex};
use tokio::sync::watch;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use inference_core::{WhisperBackend, LlamaBackend};
use audio_capture::Recorder;
use os_integration::Injector;

use crate::settings::Settings;

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ModelState {
    Idle,
    Loading { whisper: bool, llama: bool },
    Ready { whisper: bool, llama: bool },
    Reloading { reason: String },
    Error { reason: String },
}

pub struct AppStateInner {
    pub settings: Settings,
    pub whisper: Option<Arc<WhisperBackend>>,
    pub llama: Option<Arc<LlamaBackend>>,
    pub recorder: Option<Recorder>,
    pub injector: Injector,
    pub current_load_token: u64,
}

pub struct AppState {
    pub inner: Mutex<AppStateInner>,
    pub model_state_tx: watch::Sender<ModelState>,
    pub recording_state_tx: watch::Sender<bool>,
    pub audio_level_tx: watch::Sender<f32>,
}

impl AppState {
    pub fn new(settings: Settings) -> Self {
        let injector = if settings.force_pasteboard {
            Injector::with_force_pasteboard(true)
        } else {
            Injector::new()
        };
        let (model_state_tx, _) = watch::channel(ModelState::Idle);
        let (recording_state_tx, _) = watch::channel(false);
        let (audio_level_tx, _) = watch::channel(0.0_f32);

        Self {
            inner: Mutex::new(AppStateInner {
                settings,
                whisper: None,
                llama: None,
                recorder: None,
                injector,
                current_load_token: 0,
            }),
            model_state_tx,
            recording_state_tx,
            audio_level_tx,
        }
    }

    pub fn set_model_state(&self, app: &AppHandle, s: ModelState) {
        let _ = self.model_state_tx.send(s.clone());
        let _ = app.emit("model:state", &s);
    }

    pub fn current_model_state(&self) -> ModelState {
        self.model_state_tx.borrow().clone()
    }

    pub fn rebuild_injector(&self, force_pasteboard: bool) {
        let mut inner = self.inner.lock().unwrap();
        inner.injector = if force_pasteboard {
            Injector::with_force_pasteboard(true)
        } else {
            Injector::new()
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_state_serializes_with_tagged_kind() {
        let s = ModelState::Loading { whisper: true, llama: false };
        let j = serde_json::to_value(&s).unwrap();
        assert_eq!(j, serde_json::json!({
            "kind": "loading", "whisper": true, "llama": false
        }));
    }

    #[test]
    fn ready_state_serializes() {
        let s = ModelState::Ready { whisper: true, llama: true };
        let j = serde_json::to_value(&s).unwrap();
        assert_eq!(j, serde_json::json!({
            "kind": "ready", "whisper": true, "llama": true
        }));
    }

    #[test]
    fn idle_state_serializes() {
        let s = ModelState::Idle;
        let j = serde_json::to_value(&s).unwrap();
        assert_eq!(j, serde_json::json!({ "kind": "idle" }));
    }
}
