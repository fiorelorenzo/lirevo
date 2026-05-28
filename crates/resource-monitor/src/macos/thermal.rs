//! `NSProcessInfo.thermalState` reader + KVO observer.
//!
//! On init we read the current value once. We then register a block
//! callback for `NSProcessInfoThermalStateDidChangeNotification` on the
//! default `NSNotificationCenter`; the block writes the new value into
//! `SharedState` and fires the `Notify` so the monitor emits an extra
//! snapshot off-tick.

use std::ptr::NonNull;
use std::sync::Arc;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2_foundation::{
    NSNotification, NSNotificationCenter, NSOperationQueue, NSProcessInfo,
    NSProcessInfoThermalState, NSProcessInfoThermalStateDidChangeNotification,
};
use tokio::sync::Notify;
use tracing::warn;

use crate::ThermalState;
use crate::shared::SharedState;

/// Spawn the sensor. Returns `Some(notify)` whose `notify_one` is called
/// every time the thermal state changes. Returns `None` if the system
/// API isn't reachable (extremely unlikely on macOS but defensive — and
/// other sensors landing in T8+ will legitimately return `None` when
/// their underlying API is missing, so the `Option` shape is shared).
#[allow(clippy::needless_pass_by_value, clippy::unnecessary_wraps)]
pub(super) fn spawn(state: Arc<SharedState>) -> Option<Arc<Notify>> {
    let pi = NSProcessInfo::processInfo();
    let current = thermal_from_ns(pi.thermalState());
    state.set_thermal(current);

    let notify = Arc::new(Notify::new());

    let state_for_block = state.clone();
    let notify_for_block = notify.clone();

    // Block fires on the queue we pass in (None == "post on whichever
    // thread posted the notification", which for this notification is
    // an internal Foundation queue — fine for atomic writes).
    let block = RcBlock::new(move |_note: NonNull<NSNotification>| {
        let pi = NSProcessInfo::processInfo();
        let val = thermal_from_ns(pi.thermalState());
        state_for_block.set_thermal(val);
        notify_for_block.notify_one();
    });

    // SAFETY: `addObserverForName:object:queue:usingBlock:` retains the
    // block and returns an opaque observer object we must keep alive for
    // the duration we want the callback. We `Retained::into_raw` it
    // because the `ResourceMonitor` lives for the lifetime of the app;
    // wiring `removeObserver:` on shutdown is not part of v0.6. The
    // `NSProcessInfoThermalStateDidChangeNotification` global is a
    // `extern "C" static` so the load is also unsafe.
    unsafe {
        let nc: Retained<NSNotificationCenter> = NSNotificationCenter::defaultCenter();
        let observer = nc.addObserverForName_object_queue_usingBlock(
            Some(NSProcessInfoThermalStateDidChangeNotification),
            None,
            None::<&NSOperationQueue>,
            &block,
        );
        // Leak intentionally — see comment above.
        let _ = Retained::into_raw(observer);
    }

    Some(notify)
}

fn thermal_from_ns(raw: NSProcessInfoThermalState) -> ThermalState {
    match raw {
        NSProcessInfoThermalState::Nominal => ThermalState::Nominal,
        NSProcessInfoThermalState::Fair => ThermalState::Fair,
        NSProcessInfoThermalState::Serious => ThermalState::Serious,
        NSProcessInfoThermalState::Critical => ThermalState::Critical,
        other => {
            warn!(
                "unknown NSProcessInfoThermalState value {:?}, defaulting to Nominal",
                other.0
            );
            ThermalState::Nominal
        }
    }
}
