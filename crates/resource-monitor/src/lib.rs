//! Cross-platform system resource monitor for Lirevo.
//!
//! Emits [`Signals`] snapshots (battery / thermal / memory / CPU /
//! foreground-app pressure) via a `tokio::sync::broadcast` channel.
//! Sensors are platform-specific; consumers receive an OS-neutral
//! `Signals` struct.
//!
//! On non-macOS targets, every sensor returns its default ("conservative")
//! value (`ThermalState::Nominal`, `battery_pct = None`, etc.) and
//! [`ResourceMonitor::spawn`] returns an error variant indicating the
//! platform is unsupported. The crate still compiles cross-platform so
//! consumers do not need their own cfg-gating.

#![warn(clippy::pedantic)]
#![allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]

mod signals;
pub use signals::{ForegroundApp, MemoryPressure, Signals, SignalsBuilder, ThermalState};

mod error;
pub use error::MonitorError;

mod shared;
