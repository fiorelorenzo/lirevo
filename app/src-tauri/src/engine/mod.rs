//! The Engine: lazy load + resource-aware unload of the LLM and STT.
//!
//! Lives in the Tauri host (not `inference-core`) because it orchestrates
//! inference-core's `LlamaBackend`, the host's STT model + streaming
//! worker, `resource_monitor`, and `inference_core::profile::ProfileSelector`.
//! See `docs/superpowers/plans/2026-05-29-m53-engine-lifecycle.md`.

mod decision;
mod slot;
mod streak;

use std::sync::Arc;
use std::time::Instant;

use arc_swap::ArcSwap;
use inference_core::profile::{policy_for, ProfileName, ProfilePolicy};
use inference_core::{ChatRequest, ChatResponse, LlamaBackend, LlmError};
use tokio::sync::Mutex as AsyncMutex;

use crate::engine::decision::resolve_n_threads;
use crate::engine::slot::{LlmSlot, SlotSnapshot, SttSlotState};
use crate::state::SttSlot;

pub use crate::engine::decision::UnloadReason;
// `Action` is consumed by `apply_action` in C2 (lifecycle_loop); re-exported
// now so the public surface is in one place.
#[allow(unused_imports)]
pub use crate::engine::decision::Action;

/// What the Engine needs to know to load models. Plain data (not
/// `tauri::State`) so the Engine is testable.
// Engine is constructed + unit-tested in isolation here; Phase D wires it into
// the live app (replaces `AppStateInner`'s model fields). Until then the struct
// + its methods are only reachable from tests.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub llm_model_path: Option<std::path::PathBuf>,
    pub llm_ctx_size: u32,
    pub stt_model_id: Option<String>,
    pub keep_warm: bool,
}

#[allow(dead_code)]
pub struct Engine {
    config: ArcSwap<EngineConfig>,
    current_policy: ArcSwap<ProfilePolicy>,
    llm: AsyncMutex<LlmSlot>,
    stt: AsyncMutex<SttSlotState>,
    last_dictation: std::sync::Mutex<Instant>,
}

#[allow(dead_code)]
impl Engine {
    #[must_use]
    pub fn new(config: EngineConfig, initial_profile: ProfileName) -> Arc<Self> {
        Arc::new(Self {
            config: ArcSwap::from_pointee(config),
            current_policy: ArcSwap::from_pointee(policy_for(initial_profile)),
            llm: AsyncMutex::new(LlmSlot::Unloaded),
            stt: AsyncMutex::new(SttSlotState::Unloaded),
            last_dictation: std::sync::Mutex::new(Instant::now()),
        })
    }

    pub fn set_policy(&self, policy: ProfilePolicy) {
        self.current_policy.store(Arc::new(policy));
    }

    #[must_use]
    pub fn current_policy(&self) -> ProfilePolicy {
        (**self.current_policy.load()).clone()
    }

    pub fn update_config(&self, config: EngineConfig) {
        self.config.store(Arc::new(config));
    }

    pub fn llm_snapshot(&self) -> SlotSnapshot {
        // try_lock: snapshotting must never block the caller; if the slot
        // is mid-transition treat it as Loading for decision purposes.
        match self.llm.try_lock() {
            Ok(g) => g.snapshot(),
            Err(_) => SlotSnapshot::Loading,
        }
    }

    pub fn stt_snapshot(&self) -> SlotSnapshot {
        match self.stt.try_lock() {
            Ok(g) => g.snapshot(),
            Err(_) => SlotSnapshot::Loading,
        }
    }

    pub fn mark_dictation(&self) {
        *self.last_dictation.lock().expect("last_dictation") = Instant::now();
    }

    /// Lazy-load the LLM if needed, returning a handle for one chat. Returns
    /// `Ok(None)` if no model path is configured (graceful STT-only mode).
    pub async fn ensure_llm(&self) -> Result<Option<Arc<LlamaBackend>>, LlmError> {
        {
            let slot = self.llm.lock().await;
            if let LlmSlot::Loaded { backend, .. } = &*slot {
                return Ok(Some(backend.clone()));
            }
        }
        let Some(path) = self.config.load().llm_model_path.clone() else {
            return Ok(None);
        };
        let ctx = self.config.load().llm_ctx_size;
        let n_threads = resolve_n_threads(self.current_policy().n_threads);

        {
            let mut slot = self.llm.lock().await;
            *slot = LlmSlot::Loading { since: Instant::now() };
        }

        let loaded = tokio::task::spawn_blocking(move || LlamaBackend::load(path, ctx, n_threads))
            .await
            .map_err(|e| LlmError::Internal(format!("llm load join: {e}")))?;

        match loaded {
            Ok(backend) => {
                let backend = Arc::new(backend);
                let mut slot = self.llm.lock().await;
                *slot = LlmSlot::Loaded {
                    backend: backend.clone(),
                    last_use: Instant::now(),
                    loaded_n_threads: n_threads,
                };
                Ok(Some(backend))
            }
            Err(e) => {
                let mut slot = self.llm.lock().await;
                *slot = LlmSlot::Unloaded;
                Err(e)
            }
        }
    }

    /// One cleanup chat. Lazy-loads, runs, marks the LLM used.
    pub async fn chat(&self, req: ChatRequest) -> Result<Option<ChatResponse>, LlmError> {
        let Some(backend) = self.ensure_llm().await? else {
            return Ok(None);
        };
        let res = tokio::task::block_in_place(|| backend.chat_sync(req));
        if let Ok(mut slot) = self.llm.try_lock() {
            if let LlmSlot::Loaded { last_use, .. } = &mut *slot {
                *last_use = Instant::now();
            }
        }
        res.map(Some)
    }

    /// Unload the LLM. Idempotent.
    pub async fn unload_llm(&self, reason: UnloadReason) {
        let mut slot = self.llm.lock().await;
        if !matches!(&*slot, LlmSlot::Unloaded) {
            tracing::info!(?reason, "engine: unloading LLM");
            *slot = LlmSlot::Unloaded;
        }
    }

    /// Unload the STT, but ONLY if not currently in use (the streaming
    /// worker holds the slot lock during a dictation). Returns true if it
    /// actually unloaded.
    pub async fn unload_stt(&self, reason: UnloadReason) -> bool {
        let mut state = self.stt.lock().await;
        match &*state {
            SttSlotState::Loaded { slot, .. } => {
                // Skip if the model is mid-dictation (worker holds it).
                if slot.try_lock().is_err() {
                    tracing::debug!(?reason, "engine: STT in use, deferring unload");
                    return false;
                }
                tracing::info!(?reason, "engine: unloading STT");
                *state = SttSlotState::Unloaded;
                true
            }
            _ => false,
        }
    }

    /// Lazy-load the STT model. Returns the slot, or `Ok(None)` if no model
    /// id is configured or the weights are still downloading.
    pub async fn ensure_stt(&self) -> Result<Option<SttSlot>, String> {
        {
            let state = self.stt.lock().await;
            if let SttSlotState::Loaded { slot, .. } = &*state {
                return Ok(Some(slot.clone()));
            }
        }
        let Some(id) = self.config.load().stt_model_id.clone() else {
            return Ok(None);
        };

        {
            let mut state = self.stt.lock().await;
            *state = SttSlotState::Loading { since: Instant::now() };
        }

        let outcome = tokio::task::spawn_blocking(move || crate::stt::load(&id))
            .await
            .map_err(|e| format!("stt load join: {e}"))?
            .map_err(|e| e.to_string())?;

        match outcome {
            crate::stt::LoadOutcome::Ready(handle) => {
                let slot: SttSlot = Arc::new(AsyncMutex::new(handle));
                let mut state = self.stt.lock().await;
                *state = SttSlotState::Loaded {
                    slot: slot.clone(),
                    last_use: Instant::now(),
                };
                Ok(Some(slot))
            }
            // Real variant carries `audiopipe_name` (see stt::LoadOutcome);
            // the slot stays Unloaded — caller surfaces a "downloading"
            // status and retries once the background fetch finishes.
            crate::stt::LoadOutcome::Downloading { .. } => {
                let mut state = self.stt.lock().await;
                *state = SttSlotState::Unloaded;
                Ok(None)
            }
        }
    }

    pub fn mark_stt_used(&self) {
        if let Ok(mut state) = self.stt.try_lock() {
            if let SttSlotState::Loaded { last_use, .. } = &mut *state {
                *last_use = Instant::now();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(path: Option<std::path::PathBuf>) -> EngineConfig {
        EngineConfig {
            llm_model_path: path,
            llm_ctx_size: 4096,
            stt_model_id: None,
            keep_warm: false,
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn ensure_llm_none_when_no_path() {
        let e = Engine::new(cfg(None), ProfileName::Balanced);
        assert!(e.ensure_llm().await.unwrap().is_none());
        assert_eq!(e.llm_snapshot(), SlotSnapshot::Unloaded);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn ensure_llm_errors_and_resets_on_bad_path() {
        let e = Engine::new(
            cfg(Some("/definitely/missing.gguf".into())),
            ProfileName::Balanced,
        );
        let r = e.ensure_llm().await;
        assert!(r.is_err());
        // Slot must be back to Unloaded so a retry / auto-recover can run.
        assert_eq!(e.llm_snapshot(), SlotSnapshot::Unloaded);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn unload_llm_is_idempotent() {
        let e = Engine::new(cfg(None), ProfileName::Balanced);
        e.unload_llm(UnloadReason::IdleTimeout).await; // no-op, no panic
        assert_eq!(e.llm_snapshot(), SlotSnapshot::Unloaded);
    }
}
