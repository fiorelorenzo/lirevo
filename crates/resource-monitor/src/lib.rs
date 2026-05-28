//! Cross-platform system resource monitor for Lirevo.
//!
//! Emits [`Signals`] snapshots (battery / thermal / memory / CPU /
//! foreground-app pressure) via a `tokio::sync::broadcast` channel.
//! Sensors are platform-specific; consumers receive an OS-neutral
//! `Signals` struct.
//!
//! On non-macOS targets, every sensor returns its default ("conservative")
//! value (`ThermalState::Nominal`, `battery_pct = None`, etc.) and
//! [`ResourceMonitor::spawn`] succeeds with a no-op sensor stack —
//! consumers don't need their own cfg-gating.

#![warn(clippy::pedantic)]
#![allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]

mod signals;
pub use signals::{ForegroundApp, MemoryPressure, Signals, SignalsBuilder, ThermalState};

mod error;
pub use error::MonitorError;

mod shared;

// T7 swaps in a real macOS sensor module that supersedes `stub` on
// `target_os = "macos"`. Until then the stub provides the no-op
// `build_platform_sensors` on every target so `ResourceMonitor::spawn`
// compiles.
mod stub;
pub(crate) use stub::build_platform_sensors;

mod monitor;
pub use monitor::ResourceMonitor;
