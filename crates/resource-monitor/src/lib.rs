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

// macOS uses the real sensor stack; every other target falls back to
// the no-op `stub` so `ResourceMonitor::spawn` keeps compiling. Both
// expose `build_platform_sensors` with the same signature.
#[cfg(not(target_os = "macos"))]
mod stub;
#[cfg(not(target_os = "macos"))]
pub(crate) use stub::build_platform_sensors;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub(crate) use macos::build_platform_sensors;

mod monitor;
pub use monitor::ResourceMonitor;
