use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};
use tokio::sync::watch;
use tokio::sync::Mutex as AsyncMutex;

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
/// `parakeet_cpp::Model::transcribe` takes `&mut self` — a single shared
/// handle plus this mutex lets the dictation pipeline run from many tasks
/// without re-loading weights per call.
pub type SttSlot = Arc<AsyncMutex<SttModelHandle>>;

/// Per-dictation recording metadata, captured at `handle_down` time so the
/// history-insert in `run_pipeline` can record which input device was actually
/// used and whether smart Bluetooth routing kicked in.
#[derive(Clone, Debug)]
pub struct RecordingMeta {
    /// Human-readable label of the input device actually opened.
    pub input_device: String,
    /// Whether the smart-mic-routing setting was enabled for this dictation.
    pub smart_routing_enabled: bool,
    /// Whether smart routing actually rerouted away from the configured mic.
    pub smart_routing_applied: bool,
}

pub struct AppStateInner {
    pub settings: Settings,
    /// Unified model lifecycle owner: lazy-loads + resource-aware-unloads the
    /// STT and LLM backends. Replaces the old manually-managed `stt` / `llama`
    /// fields. See `crate::engine`.
    pub engine: Arc<crate::engine::Engine>,
    pub recorder: Option<Recorder>,
    /// Metadata for the in-flight recording, set alongside `recorder` in
    /// `handle_down` and read by `run_pipeline` for the history row.
    pub recording_meta: Option<RecordingMeta>,
    pub injector: Injector,
    pub current_load_token: u64,
    /// Live streaming worker for the in-flight dictation, if any. Installed
    /// by `handle_down` and consumed by `handle_up`.
    pub streaming: Option<StreamingHandle>,
}

pub struct AppState {
    pub inner: Mutex<AppStateInner>,
    /// Generic local SQLite DB (dictation history, future features). Held as an
    /// `Arc<Db>` OUTSIDE `inner`'s mutex: `Db` has its own internal connection
    /// mutex, so a command can hit the DB without contending on the per-request
    /// `AppStateInner` lock.
    pub db: Arc<crate::db::Db>,
    /// Energy-profile selector. Installed lazily from the `setup()` async task
    /// once `ResourceMonitor::spawn()` has produced a signal stream (the
    /// selector needs `monitor.subscribe()`), so it's `None` for the brief
    /// window before that task runs — and stays `None` if the monitor failed
    /// to spawn. Held in an `ArcSwapOption` so the profile commands read it
    /// lock-free without contending on the per-request `inner` mutex.
    profile_selector: arc_swap::ArcSwapOption<inference_core::profile::ProfileSelector>,
    pub model_state_tx: watch::Sender<ModelState>,
    pub recording_state_tx: watch::Sender<bool>,
    pub audio_level_tx: watch::Sender<f32>,
}

impl AppState {
    pub fn new(
        app: &AppHandle,
        settings: Settings,
        db: Arc<crate::db::Db>,
        models_dir: std::path::PathBuf,
    ) -> Self {
        let injector = Injector::new();
        let (model_state_tx, _) = watch::channel(ModelState::Idle);
        let (recording_state_tx, _) = watch::channel(false);
        let (audio_level_tx, _) = watch::channel(0.0_f32);

        // Build the Engine from the persisted settings. The initial profile is
        // Balanced; the ProfileSelector overrides the policy once resource
        // signals start flowing (see lib.rs setup wiring).
        let engine = crate::engine::Engine::new(
            crate::engine::EngineConfig {
                llm_model_path: crate::models::effective_llm_path(app),
                llm_ctx_size: settings.llm_ctx_size,
                stt_model_id: Some(crate::stt::catalog::default_model_id().to_string()),
            },
            inference_core::profile::ProfileName::Balanced,
            models_dir,
        );

        Self {
            inner: Mutex::new(AppStateInner {
                settings,
                engine,
                recorder: None,
                recording_meta: None,
                injector,
                current_load_token: 0,
                streaming: None,
            }),
            db,
            profile_selector: arc_swap::ArcSwapOption::empty(),
            model_state_tx,
            recording_state_tx,
            audio_level_tx,
        }
    }

    /// The app's local SQLite DB. `Db` is internally synchronized, so callers
    /// don't need to lock `inner` to use it.
    pub fn db(&self) -> &crate::db::Db {
        &self.db
    }

    /// Install the energy-profile selector. Called once from the `setup()`
    /// async task after the resource monitor's signal stream exists.
    pub fn set_profile_selector(&self, selector: Arc<inference_core::profile::ProfileSelector>) {
        self.profile_selector.store(Some(selector));
    }

    /// The energy-profile selector, if it has been installed yet. `None` only
    /// during the brief startup window before the `setup()` async task runs,
    /// or permanently if the resource monitor failed to spawn.
    pub fn profile_selector(&self) -> Option<Arc<inference_core::profile::ProfileSelector>> {
        self.profile_selector.load_full()
    }

    pub fn set_model_state(&self, app: &AppHandle, s: ModelState) {
        let _ = self.model_state_tx.send(s.clone());
        let _ = app.emit("model:state", &s);
    }

    pub fn current_model_state(&self) -> ModelState {
        self.model_state_tx.borrow().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_state_serializes_with_tagged_kind() {
        let s = ModelState::Loading {
            stt: true,
            llama: false,
        };
        let j = serde_json::to_value(&s).unwrap();
        assert_eq!(
            j,
            serde_json::json!({
                "kind": "loading", "stt": true, "llama": false
            })
        );
    }

    #[test]
    fn ready_state_serializes() {
        let s = ModelState::Ready {
            stt: true,
            llama: true,
        };
        let j = serde_json::to_value(&s).unwrap();
        assert_eq!(
            j,
            serde_json::json!({
                "kind": "ready", "stt": true, "llama": true
            })
        );
    }

    #[test]
    fn idle_state_serializes() {
        let s = ModelState::Idle;
        let j = serde_json::to_value(&s).unwrap();
        assert_eq!(j, serde_json::json!({ "kind": "idle" }));
    }
}
