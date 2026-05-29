use std::sync::{Arc, Mutex};
use tokio::sync::watch;
use tokio::sync::Mutex as AsyncMutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use audio_capture::Recorder;
use os_integration::Injector;

use crate::commands::inference::StreamingHandle;
use crate::settings::Settings;
use crate::stt::SttModelHandle;

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ModelState {
    Idle,
    Loading { stt: bool, llama: bool },
    Ready { stt: bool, llama: bool },
    Reloading { reason: String },
    Error { reason: String },
}

/// The STT model is held behind a `tokio::sync::Mutex` because
/// `audiopipe::Model::transcribe_with_sample_rate` takes `&mut self` — a
/// single shared handle plus this mutex lets the dictation pipeline run
/// from many tasks without re-loading weights per call.
pub type SttSlot = Arc<AsyncMutex<SttModelHandle>>;

pub struct AppStateInner {
    pub settings: Settings,
    /// Unified model lifecycle owner: lazy-loads + resource-aware-unloads the
    /// STT and LLM backends. Replaces the old manually-managed `stt` / `llama`
    /// fields. See `crate::engine`.
    pub engine: Arc<crate::engine::Engine>,
    pub recorder: Option<Recorder>,
    pub injector: Injector,
    pub current_load_token: u64,
    /// Live streaming worker for the in-flight dictation, if any. Installed
    /// by `handle_down` and consumed by `handle_up`.
    pub streaming: Option<StreamingHandle>,
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

        // Build the Engine from the persisted settings. The initial profile is
        // Balanced; the ProfileSelector overrides the policy once resource
        // signals start flowing (see lib.rs setup wiring).
        let engine = crate::engine::Engine::new(
            crate::engine::EngineConfig {
                llm_model_path: settings.llm_model_path.clone(),
                llm_ctx_size: settings.llm_ctx_size,
                stt_model_id: settings.stt_model_id.clone(),
                keep_warm: settings.keep_models_warm,
            },
            inference_core::profile::ProfileName::Balanced,
        );

        Self {
            inner: Mutex::new(AppStateInner {
                settings,
                engine,
                recorder: None,
                injector,
                current_load_token: 0,
                streaming: None,
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
        let s = ModelState::Loading { stt: true, llama: false };
        let j = serde_json::to_value(&s).unwrap();
        assert_eq!(j, serde_json::json!({
            "kind": "loading", "stt": true, "llama": false
        }));
    }

    #[test]
    fn ready_state_serializes() {
        let s = ModelState::Ready { stt: true, llama: true };
        let j = serde_json::to_value(&s).unwrap();
        assert_eq!(j, serde_json::json!({
            "kind": "ready", "stt": true, "llama": true
        }));
    }

    #[test]
    fn idle_state_serializes() {
        let s = ModelState::Idle;
        let j = serde_json::to_value(&s).unwrap();
        assert_eq!(j, serde_json::json!({ "kind": "idle" }));
    }
}
