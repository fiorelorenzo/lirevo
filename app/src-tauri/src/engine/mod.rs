//! The Engine: lazy load + resource-aware unload of the LLM and STT.
//!
//! Lives in the Tauri host (not `inference-core`) because it orchestrates
//! inference-core's `LlamaBackend`, the host's STT model + streaming
//! worker, `resource_monitor`, and `inference_core::profile::ProfileSelector`.
//! See `docs/superpowers/plans/2026-05-29-m53-engine-lifecycle.md`.

mod decision;
mod slot;
mod streak;
