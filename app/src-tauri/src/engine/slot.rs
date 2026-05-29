//! Engine slot state machines for the LLM and STT models.
//!
//! Each model is `Unloaded`, `Loading`, or `Loaded`. The pure
//! [`crate::engine::decision`] layer never sees the real backend handles —
//! only a lightweight [`SlotSnapshot`] (is-loaded, last-use, loaded thread
//! count) so the lifecycle logic stays clock-injectable and testable
//! without real models.

#![allow(dead_code)] // consumed by the Engine shell in Phase C

use std::sync::Arc;
use std::time::Instant;

use inference_core::LlamaBackend;

use crate::state::SttSlot;

/// LLM slot. `loaded_n_threads` records the thread count the context was
/// built with, so the lifecycle layer can detect a profile-driven change.
pub enum LlmSlot {
    Unloaded,
    Loading { since: Instant },
    Loaded {
        backend: Arc<LlamaBackend>,
        last_use: Instant,
        loaded_n_threads: i32,
    },
}

/// STT slot. The handle is the same `Arc<AsyncMutex<SttModelHandle>>` the
/// streaming worker borrows; the Engine owns it but the worker holds the
/// lock during a dictation.
pub enum SttSlotState {
    Unloaded,
    Loading { since: Instant },
    Loaded { slot: SttSlot, last_use: Instant },
}

/// A clock/handle-free view of a slot for the pure decision layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotSnapshot {
    Unloaded,
    Loading,
    /// Loaded since this `last_use`; `loaded_n_threads` is `None` for STT
    /// (threads are not a tunable for the STT backend in v0.6).
    Loaded {
        last_use: Instant,
        loaded_n_threads: Option<i32>,
    },
}

impl LlmSlot {
    pub fn snapshot(&self) -> SlotSnapshot {
        match self {
            LlmSlot::Unloaded => SlotSnapshot::Unloaded,
            LlmSlot::Loading { .. } => SlotSnapshot::Loading,
            LlmSlot::Loaded { last_use, loaded_n_threads, .. } => SlotSnapshot::Loaded {
                last_use: *last_use,
                loaded_n_threads: Some(*loaded_n_threads),
            },
        }
    }
}

impl SttSlotState {
    pub fn snapshot(&self) -> SlotSnapshot {
        match self {
            SttSlotState::Unloaded => SlotSnapshot::Unloaded,
            SttSlotState::Loading { .. } => SlotSnapshot::Loading,
            SttSlotState::Loaded { last_use, .. } => SlotSnapshot::Loaded {
                last_use: *last_use,
                loaded_n_threads: None,
            },
        }
    }
}
