//! Runtime probes: latency, memory.
//!
//! Latency is backend-agnostic (any `EvalBackend` impl). Memory currently
//! depends on macOS APIs and is added in Task 10.

pub mod latency;
