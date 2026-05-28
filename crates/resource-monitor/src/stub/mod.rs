//! Non-macOS fallback. Lets the crate compile on Linux + Windows. All
//! sensors leave the shared state at its conservative defaults
//! (`ThermalState::Nominal`, `battery_pct = None`, etc.) so the
//! `ProfileSelector` scoring degenerates to "no pressure" on those
//! targets until real sensors are implemented in v2.

use std::sync::Arc;
use tokio::sync::Notify;
use tokio::task::JoinHandle;

use crate::shared::SharedState;

/// Returns `(instant_notifiers, sensor_task_handles)`. On non-macOS no
/// sensor emits instant events and no polling task is spawned, so both
/// vectors are empty. The signature mirrors the macOS implementation
/// exactly so `monitor::ResourceMonitor` doesn't need to cfg-gate.
pub(crate) fn build_platform_sensors(
    _state: Arc<SharedState>,
) -> (Vec<Arc<Notify>>, Vec<JoinHandle<()>>) {
    (Vec::new(), Vec::new())
}
