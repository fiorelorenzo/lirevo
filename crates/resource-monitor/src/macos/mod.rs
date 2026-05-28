//! macOS sensor implementations.
//!
//! Each submodule owns one sensor: it writes to `SharedState` from a
//! background task or Objective-C callback, and may return a
//! `tokio::sync::Notify` so `monitor::run_loop` emits an extra snapshot
//! when that sensor fires off-tick (e.g. KVO).

use std::sync::Arc;
use tokio::sync::Notify;

use crate::shared::SharedState;

mod thermal;

// Signature matches the non-macOS stub exactly (`Arc<SharedState>` by
// value) so the `pub(crate) use ...::build_platform_sensors;` re-export
// in `lib.rs` stays a single symbol.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn build_platform_sensors(state: Arc<SharedState>) -> Vec<Arc<Notify>> {
    let mut notifiers = Vec::new();
    if let Some(n) = thermal::spawn(state.clone()) {
        notifiers.push(n);
    }
    notifiers
}
