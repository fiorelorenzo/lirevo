//! Runtime probes: latency, memory.
//!
//! Latency is backend-agnostic (any `EvalBackend` impl). Memory is macOS-only
//! today (returns `None` on other targets).

pub mod latency;
pub mod memory;
