//! Non-macOS fallback. Lets the crate compile on Linux + Windows. All
//! sensors leave the shared state at its conservative defaults
//! (`ThermalState::Nominal`, `battery_pct = None`, etc.) so the
//! `ProfileSelector` scoring degenerates to "no pressure" on those
//! targets until real sensors are implemented in v2.

use std::sync::Arc;
use tokio::sync::Notify;

use crate::shared::SharedState;

/// Returns the list of "instant-change" notifiers that
/// `monitor::run_loop` should select over. On non-macOS, no sensor
/// emits instant events, so this returns an empty Vec.
pub(crate) fn build_platform_sensors(_state: Arc<SharedState>) -> Vec<Arc<Notify>> {
    Vec::new()
}
