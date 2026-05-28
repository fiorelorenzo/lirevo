//! macOS sensor implementations.
//!
//! Each submodule owns one sensor: it writes to `SharedState` from a
//! background task or Objective-C callback, and may return a
//! `tokio::sync::Notify` so `monitor::run_loop` emits an extra snapshot
//! when that sensor fires off-tick (e.g. KVO). Sensors that own a
//! `tokio::spawn`ed polling task also return its `JoinHandle` so
//! [`crate::ResourceMonitor`] can abort it on drop and avoid leaking
//! background work across monitor lifecycles.

use std::sync::Arc;
use tokio::sync::Notify;
use tokio::task::JoinHandle;

use crate::shared::SharedState;

mod power;
mod thermal;

// Signature matches the non-macOS stub exactly (`Arc<SharedState>` by
// value) so the `pub(crate) use ...::build_platform_sensors;` re-export
// in `lib.rs` stays a single symbol.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn build_platform_sensors(
    state: Arc<SharedState>,
) -> (Vec<Arc<Notify>>, Vec<JoinHandle<()>>) {
    let mut notifiers = Vec::new();
    let mut handles = Vec::new();

    let (notify, mut h) = thermal::spawn(state.clone());
    if let Some(n) = notify {
        notifiers.push(n);
    }
    handles.append(&mut h);

    let (notify, mut h) = power::spawn(state.clone());
    if let Some(n) = notify {
        notifiers.push(n);
    }
    handles.append(&mut h);

    (notifiers, handles)
}
