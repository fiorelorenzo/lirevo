//! Battery + AC + `lowPowerMode` sensor.
//!
//! Two sub-tasks:
//!
//! 1. **Battery + AC** — polled every 30 seconds via
//!    `IOPSCopyPowerSourcesInfo` / `IOPSCopyPowerSourcesList`. The
//!    "`InternalBattery`" power source carries `kIOPSCurrentCapacityKey`
//!    (current percent) and `kIOPSPowerSourceStateKey`
//!    ("AC Power" vs "Battery Power").
//!
//! 2. **`lowPowerMode`** — read once on init, then KVO via
//!    `NSProcessInfoPowerStateDidChangeNotification`.

use std::ptr::NonNull;
use std::sync::Arc;
use std::time::Duration;

use block2::RcBlock;
use core_foundation::array::{CFArray, CFArrayRef};
use core_foundation::base::{CFType, CFTypeRef, TCFType};
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use objc2::rc::Retained;
use objc2_foundation::{
    NSNotification, NSNotificationCenter, NSOperationQueue, NSProcessInfo,
    NSProcessInfoPowerStateDidChangeNotification,
};
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tracing::warn;

use crate::shared::SharedState;

const POLL_INTERVAL: Duration = Duration::from_secs(30);

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOPSCopyPowerSourcesInfo() -> CFTypeRef;
    fn IOPSCopyPowerSourcesList(blob: CFTypeRef) -> CFArrayRef;
    fn IOPSGetPowerSourceDescription(blob: CFTypeRef, ps: CFTypeRef) -> CFDictionaryRef;
}

/// Spawn the power sensor. Performs an initial battery poll + initial
/// `lowPowerMode` read synchronously so the first `Signals` snapshot has
/// real values, then spawns a tokio task polling every 30 s for
/// battery/AC and registers a KVO observer for `lowPowerMode` changes.
///
/// Returns `(Some(notify), handles)` where `notify_one` fires whenever
/// the `lowPowerMode` KVO callback runs, so `monitor::run_loop` emits an
/// extra off-tick snapshot, and `handles` carries the battery polling
/// task so [`crate::ResourceMonitor`] can abort it on drop. The `Option`
/// on the notifier matches the other sensor modules.
#[allow(clippy::needless_pass_by_value, clippy::unnecessary_wraps)]
pub(super) fn spawn(state: Arc<SharedState>) -> (Option<Arc<Notify>>, Vec<JoinHandle<()>>) {
    let notify = Arc::new(Notify::new());

    // Initial read of lowPowerMode + register KVO.
    spawn_low_power_mode_observer(&state, &notify);

    // Initial poll of battery + AC, then a tokio task polling every 30s.
    poll_battery_once(&state);
    let state_for_poll = state.clone();
    let handle = tokio::spawn(async move {
        let mut ticker = tokio::time::interval(POLL_INTERVAL);
        // First tick fires immediately and we already polled above; skip it.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            poll_battery_once(&state_for_poll);
        }
    });

    (Some(notify), vec![handle])
}

fn spawn_low_power_mode_observer(state: &Arc<SharedState>, notify: &Arc<Notify>) {
    let pi = NSProcessInfo::processInfo();
    state.set_power_saver_user_pref(pi.isLowPowerModeEnabled());

    let state_for_block = state.clone();
    let notify_for_block = notify.clone();
    let block = RcBlock::new(move |_note: NonNull<NSNotification>| {
        let pi = NSProcessInfo::processInfo();
        state_for_block.set_power_saver_user_pref(pi.isLowPowerModeEnabled());
        notify_for_block.notify_one();
    });

    // SAFETY: `addObserverForName:object:queue:usingBlock:` retains the
    // block and returns an opaque observer object we must keep alive for
    // the duration we want the callback. We `Retained::into_raw` it
    // because the `ResourceMonitor` lives for the lifetime of the app;
    // wiring `removeObserver:` on shutdown is not part of v0.6. The
    // `NSProcessInfoPowerStateDidChangeNotification` global is loaded
    // via a typed `&'static NSNotificationName` static, mirroring how
    // the thermal sensor handles `NSProcessInfoThermalStateDidChangeNotification`.
    unsafe {
        let nc: Retained<NSNotificationCenter> = NSNotificationCenter::defaultCenter();
        let observer = nc.addObserverForName_object_queue_usingBlock(
            Some(NSProcessInfoPowerStateDidChangeNotification),
            None,
            None::<&NSOperationQueue>,
            &block,
        );
        // Leak intentionally — see comment above.
        let _ = Retained::into_raw(observer);
    }
}

/// One `IOKit` query → write battery + AC into `SharedState`. Failure is
/// logged at `warn!` and leaves the previous values in place.
fn poll_battery_once(state: &Arc<SharedState>) {
    // SAFETY: IOPSCopyPowerSourcesInfo returns a retained CFTypeRef we
    // must release; wrapping via `CFType::wrap_under_create_rule` does
    // that on drop. IOPSCopyPowerSourcesList likewise returns a retained
    // CFArrayRef. IOPSGetPowerSourceDescription is a "Get" function,
    // hence `wrap_under_get_rule`.
    unsafe {
        let blob_raw = IOPSCopyPowerSourcesInfo();
        if blob_raw.is_null() {
            warn!("IOPSCopyPowerSourcesInfo returned null");
            return;
        }
        let blob: CFType = CFType::wrap_under_create_rule(blob_raw);

        let list_raw = IOPSCopyPowerSourcesList(blob.as_concrete_TypeRef());
        if list_raw.is_null() {
            warn!("IOPSCopyPowerSourcesList returned null");
            return;
        }
        let list: CFArray = CFArray::wrap_under_create_rule(list_raw);

        let mut found_battery = false;
        let mut on_ac_known = false;
        let mut on_ac = true;

        for ps_raw in list.iter() {
            let ps_ptr: CFTypeRef = *ps_raw;
            if ps_ptr.is_null() {
                continue;
            }
            let desc_raw = IOPSGetPowerSourceDescription(blob.as_concrete_TypeRef(), ps_ptr);
            if desc_raw.is_null() {
                continue;
            }
            // GetPowerSourceDescription is a "Get" function — no retain to release.
            let desc: CFDictionary<CFString, CFType> = CFDictionary::wrap_under_get_rule(desc_raw);

            // Capacity (CFNumber, Int 0..100)
            let key_cap = CFString::from_static_string("Current Capacity");
            if let Some(v) = desc.find(&key_cap) {
                if let Some(num) = v.downcast::<CFNumber>() {
                    if let Some(i) = num.to_i32() {
                        let pct = u8::try_from(i.clamp(0, 100)).unwrap_or(0);
                        state.set_battery_pct(Some(pct));
                        found_battery = true;
                    }
                }
            }

            // Power source state (CFString "AC Power" / "Battery Power")
            let key_state = CFString::from_static_string("Power Source State");
            if let Some(v) = desc.find(&key_state) {
                if let Some(s) = v.downcast::<CFString>() {
                    on_ac = s == "AC Power";
                    on_ac_known = true;
                }
            }
        }

        if !found_battery {
            state.set_battery_pct(None);
            // No battery present means a desktop / iMac-class machine — by
            // definition on AC. Safe to assert true.
            state.set_on_ac(true);
        } else if on_ac_known {
            state.set_on_ac(on_ac);
        } else {
            // Battery present but the "Power Source State" key was missing
            // or malformed. Preserving the previous value is more honest
            // than defaulting to AC on a laptop that may actually be on
            // battery.
            warn!(
                "battery present but Power Source State key absent or malformed; \
                 keeping previous on_ac"
            );
        }
    }
}
