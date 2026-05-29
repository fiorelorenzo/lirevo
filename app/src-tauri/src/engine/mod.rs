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
// `Action` is consumed by `apply_action` (lifecycle_loop); re-exported here so
// the public surface is in one place.
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

    async fn reload_llm_for_threads(self: &Arc<Self>, n_threads: i32) {
        // Only reload if currently loaded and idle. Unload then lazy-load
        // happens on next use; for an eager swap we reload immediately.
        let path = self.config.load().llm_model_path.clone();
        let ctx = self.config.load().llm_ctx_size;
        let Some(path) = path else { return };
        {
            let mut slot = self.llm.lock().await;
            if matches!(&*slot, LlmSlot::Unloaded | LlmSlot::Loading { .. }) {
                return;
            }
            *slot = LlmSlot::Loading { since: Instant::now() };
        }
        let loaded =
            tokio::task::spawn_blocking(move || LlamaBackend::load(path, ctx, n_threads)).await;
        let mut slot = self.llm.lock().await;
        match loaded {
            Ok(Ok(backend)) => {
                tracing::info!(n_threads, "engine: reloaded LLM for thread-count change");
                *slot = LlmSlot::Loaded {
                    backend: Arc::new(backend),
                    last_use: Instant::now(),
                    loaded_n_threads: n_threads,
                };
            }
            _ => {
                tracing::warn!("engine: LLM reload failed; left unloaded");
                *slot = LlmSlot::Unloaded;
            }
        }
    }

    async fn preload_llm(self: &Arc<Self>) {
        // Best-effort: ignore errors (next chat will retry / surface).
        let _ = self.ensure_llm().await;
    }

    async fn apply_action(self: &Arc<Self>, action: Action) {
        match action {
            Action::UnloadLlm(r) => self.unload_llm(r).await,
            Action::UnloadStt(r) => {
                self.unload_stt(r).await;
            }
            Action::ReloadLlmForThreads { n_threads } => {
                self.reload_llm_for_threads(n_threads).await;
            }
            Action::PreloadLlm => self.preload_llm().await,
        }
    }

    /// Background lifecycle loop: ticks every 5s and reacts to signals.
    /// Spawn this once on Engine construction (Phase D wires it).
    ///
    /// Action application is best-effort and re-evaluated every tick: because
    /// `reload_llm_for_threads`/`preload_llm`/`ensure_llm` drop the slot lock
    /// during their `spawn_blocking` load, an unload issued concurrently can
    /// be silently overwritten by an in-flight load. That is acceptable for
    /// v0.6 — the next tick re-runs the decision and re-issues the unload if
    /// the condition still holds (no epoch/generation guard needed).
    pub async fn lifecycle_loop(
        self: Arc<Self>,
        mut signals: tokio::sync::broadcast::Receiver<resource_monitor::Signals>,
    ) {
        use crate::engine::streak::ForegroundHeavyStreak;
        let mut streak = ForegroundHeavyStreak::new();
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(5));
        // Keep the latest signals for tick-driven checks.
        let mut latest: Option<resource_monitor::Signals> = None;

        loop {
            let sig = tokio::select! {
                _ = ticker.tick() => latest.clone(),
                r = signals.recv() => match r {
                    Ok(s) => { latest = Some(s.clone()); Some(s) }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                },
            };
            let Some(sig) = sig else { continue };

            let now = Instant::now();
            let fg_streak = streak.observe(&sig, now);
            let policy = self.current_policy();
            let llm = self.llm_snapshot();
            let stt = self.stt_snapshot();
            let last_dictation = *self.last_dictation.lock().expect("last_dictation");

            let actions = crate::engine::decision::lifecycle_decision(
                llm,
                stt,
                &sig,
                &policy,
                now,
                last_dictation,
                fg_streak,
            );
            for action in actions {
                self.apply_action(action).await;
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

    #[tokio::test(flavor = "multi_thread")]
    async fn lifecycle_loop_preloads_then_pressure_unloads() {
        use resource_monitor::{MemoryPressure, Signals, ThermalState};
        use std::time::SystemTime;
        // No model path → preload + load are no-ops (ensure_llm returns
        // None), so this test exercises the loop plumbing + action dispatch
        // without needing a GGUF. We assert it doesn't panic and exits on
        // channel close.
        let (tx, rx) = tokio::sync::broadcast::channel(16);
        let e = Engine::new(cfg(None), ProfileName::Balanced);
        let handle = tokio::spawn(e.clone().lifecycle_loop(rx));

        let sig = Signals {
            ts: SystemTime::UNIX_EPOCH,
            battery_pct: None,
            on_ac: true,
            power_saver_user_pref: false,
            thermal: ThermalState::Nominal,
            mem_pressure: MemoryPressure::Critical,
            mem_free_pct: 5,
            mem_free_mb: 500,
            cpu_used_pct: 5,
            foreground: None,
        };
        tx.send(sig).unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Dropping the sender closes the channel → loop breaks → task ends.
        drop(tx);
        tokio::time::timeout(std::time::Duration::from_secs(2), handle)
            .await
            .expect("loop exits on channel close")
            .expect("no panic");
    }
}
