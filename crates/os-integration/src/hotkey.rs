//! Push-to-talk hotkey listener via `CGEventTap`.
//!
//! The tap must be hosted on a thread that owns a `CFRunLoop`. cpal worker threads
//! don't have one, so we spawn a dedicated `hotkey-tap` thread, install the tap
//! there, and forward Down/Up transitions to a tokio mpsc channel.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
use core_graphics::event::{
    CGEvent, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventTapProxy, CGEventType, CallbackResult, EventField,
};
use thiserror::Error;
use tokio::sync::mpsc;
use tracing::warn;

// Accessibility status is no longer checked here — see `install()`.

// Input Monitoring TCC is a separate gate on macOS Sonoma+: even with
// Accessibility granted, CGEventTap callbacks never fire unless the app
// is also listed (and toggled on) under Privacy & Security → Input
// Monitoring. We probe it via `IOHIDCheckAccess` so we can log it next to
// the install attempt.
#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOHIDCheckAccess(request_type: u32) -> u32;
}
const IOHID_REQUEST_TYPE_LISTEN_EVENT: u32 = 1;
fn input_monitoring_label() -> &'static str {
    // IOHIDAccessType: 0 = Granted, 1 = Denied, 2 = Unknown.
    match unsafe { IOHIDCheckAccess(IOHID_REQUEST_TYPE_LISTEN_EVENT) } {
        0 => "granted",
        1 => "denied",
        _ => "unknown",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hotkey {
    RightOption,
    LeftOption,
    RightCommand,
    Fn,
    F5,
}

impl Hotkey {
    pub fn from_env() -> Self {
        match std::env::var("SIDECAR_HOTKEY").ok().as_deref() {
            Some("right-option" | "RightOption") | None => Self::RightOption,
            Some("left-option" | "LeftOption") => Self::LeftOption,
            Some("right-command" | "RightCommand") => Self::RightCommand,
            Some("fn" | "Fn") => Self::Fn,
            Some("f5" | "F5") => Self::F5,
            Some(other) => {
                warn!(value = %other, "unknown SIDECAR_HOTKEY value, falling back to RightOption");
                Self::RightOption
            }
        }
    }

    /// Virtual keycode used by `CGEvent`. Right/left modifiers have distinct codes.
    pub(crate) fn keycode(self) -> i64 {
        match self {
            Self::RightOption => 0x3D,
            Self::LeftOption => 0x3A,
            Self::RightCommand => 0x36,
            Self::Fn => 0x3F,
            Self::F5 => 0x60,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum HotkeyEvent {
    Down,
    Up,
}

#[derive(Debug, Error)]
pub enum HotkeyError {
    #[error(
        "accessibility permission denied; grant via System Settings → Privacy → Accessibility, then restart"
    )]
    PermissionDenied,
    #[error("failed to create CGEventTap")]
    TapCreationFailed,
    #[error("internal: {0}")]
    Internal(String),
}

pub struct HotkeyListener {
    shutdown_flag: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
    runloop: Arc<Mutex<Option<CFRunLoop>>>,
}

impl HotkeyListener {
    pub fn install(hotkey: Hotkey) -> Result<(Self, mpsc::Receiver<HotkeyEvent>), HotkeyError> {
        // Don't preflight with `check_accessibility()` — `AXIsProcessTrusted`
        // caches its answer for the lifetime of the process, so once the
        // user grants Accessibility in System Settings our check still
        // reports denied and we'd never get past this line. Instead, let
        // `CGEventTapCreate` itself fail (it does a fresh, uncached
        // permission check on each call) and translate that failure into
        // PermissionDenied. The worker thread reports init success/failure
        // back over a one-shot channel so we can surface it synchronously
        // here.
        let (tx, rx) = mpsc::channel::<HotkeyEvent>(64);
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let runloop_slot: Arc<Mutex<Option<CFRunLoop>>> = Arc::new(Mutex::new(None));

        let (init_tx, init_rx) = std::sync::mpsc::sync_channel::<Result<(), HotkeyError>>(1);
        let shutdown_clone = shutdown_flag.clone();
        let runloop_slot_clone = runloop_slot.clone();

        let worker = thread::Builder::new()
            .name("hotkey-tap".into())
            .spawn(move || {
                hotkey_worker(hotkey, &tx, &shutdown_clone, &runloop_slot_clone, init_tx);
            })
            .map_err(|e| HotkeyError::Internal(format!("spawn hotkey thread: {e}")))?;

        // Block on the worker reporting init status. Bounded wait so a
        // wedged worker doesn't hang setup forever.
        match init_rx.recv_timeout(Duration::from_secs(2)) {
            Ok(Ok(())) => Ok((
                Self {
                    shutdown_flag,
                    worker: Some(worker),
                    runloop: runloop_slot,
                },
                rx,
            )),
            Ok(Err(e)) => {
                shutdown_flag.store(true, std::sync::atomic::Ordering::SeqCst);
                let _ = worker.join();
                Err(e)
            }
            Err(_) => {
                shutdown_flag.store(true, std::sync::atomic::Ordering::SeqCst);
                Err(HotkeyError::Internal(
                    "hotkey worker did not signal init within 2s".into(),
                ))
            }
        }
    }

    pub fn shutdown(mut self) {
        self.stop_and_join();
    }

    fn stop_and_join(&mut self) {
        self.shutdown_flag.store(true, Ordering::SeqCst);
        if let Ok(guard) = self.runloop.lock() {
            if let Some(rl) = guard.as_ref() {
                rl.stop();
            }
        }
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

impl Drop for HotkeyListener {
    fn drop(&mut self) {
        self.stop_and_join();
    }
}

// The worker is a single conceptually-cohesive unit: QoS bump → tap install
// → run-loop pump → teardown. Splitting it would mean threading half a
// dozen captured state references through helpers for no real readability
// win. `init_tx` is owned by value so the channel closes when the worker
// returns; taking `&SyncSender` would force the caller to keep its end
// alive past the worker's lifetime.
#[allow(clippy::too_many_lines, clippy::needless_pass_by_value)]
fn hotkey_worker(
    hotkey: Hotkey,
    tx: &mpsc::Sender<HotkeyEvent>,
    shutdown: &Arc<AtomicBool>,
    runloop_slot: &Arc<Mutex<Option<CFRunLoop>>>,
    init_tx: std::sync::mpsc::SyncSender<Result<(), HotkeyError>>,
) {
    // Promote this thread to user-interactive QoS. Without this macOS App Nap
    // throttles background threads aggressively when the owning app isn't
    // focused — the symptom is that the hotkey fires while the app is in
    // the foreground but goes dead the moment focus moves elsewhere, which
    // is exactly the opposite of what a push-to-talk hotkey is for.
    // FreeFlow does the same via `thread.qualityOfService = .userInteractive`.
    #[link(name = "System", kind = "framework")]
    extern "C" {
        fn pthread_set_qos_class_self_np(qos_class: u32, relative_priority: i32) -> i32;
    }
    // From <sys/qos.h>: QOS_CLASS_USER_INTERACTIVE = 0x21.
    const QOS_CLASS_USER_INTERACTIVE: u32 = 0x21;
    let rc = unsafe { pthread_set_qos_class_self_np(QOS_CLASS_USER_INTERACTIVE, 0) };
    if rc != 0 {
        tracing::warn!(rc, "pthread_set_qos_class_self_np failed; tap may be throttled when app unfocused");
    }

    tracing::info!(
        input_monitoring = input_monitoring_label(),
        target_keycode = hotkey.keycode(),
        "hotkey_worker: about to create CGEventTap",
    );

    let is_pressed = Arc::new(AtomicBool::new(false));
    let tx_cb = tx.clone();
    let is_pressed_cb = is_pressed.clone();
    let target_keycode = hotkey.keycode();
    let callback_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let callback_count_cb = callback_count.clone();
    let tap_callback =
        move |_proxy: CGEventTapProxy, event_type: CGEventType, event: &CGEvent| -> CallbackResult {
            // The closure is invoked from a Core Foundation C trampoline.
            // Unwinding a Rust panic across that FFI boundary is undefined
            // behavior — catch any panic and convert it into "let the event
            // pass through". Any captured state is logically Send + UnwindSafe
            // (atomic flags + a tokio mpsc sender); assert it so the borrow
            // checker stays happy.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let n = callback_count_cb.fetch_add(1, Ordering::SeqCst);
                if n == 0 {
                    tracing::info!(?event_type, "tap_callback: first event received");
                }
            // Re-enable immediately on disable events. macOS sends these for
            // "secondary mouse button" presses or any callback that exceeded
            // its CPU budget; the tap stops dispatching until re-enabled.
            // FreeFlow does the equivalent synchronously inside the callback.
            if matches!(
                event_type,
                CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput
            ) {
                tracing::warn!(?event_type, "tap disabled by macOS; re-enabling synchronously");
                reenable_tap_from_callback();
                return;
            }
            if matches!(
                event_type,
                CGEventType::FlagsChanged | CGEventType::KeyDown | CGEventType::KeyUp
            ) {
                let keycode = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE);
                if keycode == target_keycode {
                    let was = is_pressed_cb.load(Ordering::SeqCst);
                    let now = match event_type {
                        CGEventType::KeyDown => true,
                        CGEventType::KeyUp => false,
                        // FlagsChanged toggles: the modifier has no explicit
                        // Down/Up; each event flips the pressed state.
                        CGEventType::FlagsChanged => !was,
                        _ => was,
                    };
                    if now != was {
                        is_pressed_cb.store(now, Ordering::SeqCst);
                        let evt = if now {
                            HotkeyEvent::Down
                        } else {
                            HotkeyEvent::Up
                        };
                        if let Err(e) = tx_cb.blocking_send(evt) {
                            warn!(error = %e, "hotkey channel closed; tap will exit");
                        }
                    }
                }
            }
        }));
        if result.is_err() {
            // Panic was caught right at the FFI boundary. Log via plain
            // eprintln rather than tracing — tracing's spans may have been
            // poisoned by whatever caused the panic.
            eprintln!("tap_callback: panicked, suppressing to keep FFI boundary intact");
        }
        // Always pass the event through — we are a passive observer.
        CallbackResult::Keep
    };

    // Session-level + Default options. We tried `ListenOnly` first (matching
    // FreeFlow's Swift code) but the tap kept getting hit with
    // TapDisabledByUserInput on every interaction and our re-enable wasn't
    // restoring event delivery. The Default (capture-capable) variant is
    // less aggressively suspended in our ad-hoc-signed dev builds.
    // Session-level still only needs Accessibility (not Input Monitoring),
    // and although Default allows modifying events we simply return Keep
    // so behavior matches ListenOnly observationally.
    let Ok(tap) = CGEventTap::new(
        CGEventTapLocation::Session,
        CGEventTapPlacement::HeadInsertEventTap,
        CGEventTapOptions::Default,
        vec![
            CGEventType::FlagsChanged,
            CGEventType::KeyDown,
            CGEventType::KeyUp,
        ],
        tap_callback,
    ) else {
        warn!(
            input_monitoring = input_monitoring_label(),
            "CGEventTap::new failed (accessibility or input-monitoring likely denied)",
        );
        let _ = init_tx.send(Err(HotkeyError::PermissionDenied));
        return;
    };
    tracing::info!("hotkey_worker: CGEventTap::new succeeded; enabling tap");

    let Ok(runloop_source) = tap.mach_port().create_runloop_source(0) else {
        warn!("create_runloop_source failed");
        let _ = init_tx.send(Err(HotkeyError::TapCreationFailed));
        return;
    };
    let runloop = CFRunLoop::get_current();
    // SAFETY: `kCFRunLoopCommonModes` is a CoreFoundation-provided extern static
    // initialized by the framework; safe to read.
    unsafe {
        runloop.add_source(&runloop_source, kCFRunLoopCommonModes);
    }
    tap.enable();

    if let Ok(mut g) = runloop_slot.lock() {
        *g = Some(runloop.clone());
    }

    // Tap is fully installed — signal success to the caller of `install`.
    let _ = init_tx.send(Ok(()));

    // Stash the mach port raw pointer so the callback can re-enable the tap
    // synchronously when macOS dispatches a TapDisabled event. Order-
    // independent and doesn't require pumping the run loop in slices.
    {
        use core_foundation::base::TCFType;
        let mach_port_ref = tap.mach_port().as_concrete_TypeRef();
        TAP_MACH_PORT.store(mach_port_ref.cast::<std::ffi::c_void>(), Ordering::SeqCst);
    }

    // Block on the run loop until someone calls `CFRunLoopStop` (the Drop
    // impl on HotkeyListener does this via the runloop slot). This matches
    // FreeFlow's CFRunLoopRun() pattern. The shutdown flag + run_current
    // race-loop we had before was pumping the loop in 200ms slices, which
    // turned out to deliver events less reliably than a single blocking
    // CFRunLoopRun().
    while !shutdown.load(Ordering::SeqCst) {
        CFRunLoop::run_current();
    }
    TAP_MACH_PORT.store(std::ptr::null_mut(), Ordering::SeqCst);
    // `tap` is dropped here; CFMachPort is invalidated by its Drop impl.
}

// CGEventTap mach port shared with the callback so it can call
// `CGEventTapEnable` directly when macOS sends a TapDisabled event. macOS
// can do this for "secondary mouse" or callback-timeout reasons and the
// tap stops dispatching events until we re-enable it. Doing it in the
// callback (synchronously) is what FreeFlow does — pumping a flag from
// outside the run loop drops events while we wait for the next iteration.
static TAP_MACH_PORT: std::sync::atomic::AtomicPtr<std::ffi::c_void> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventTapEnable(tap: *mut std::ffi::c_void, enable: bool);
}

fn reenable_tap_from_callback() {
    let port = TAP_MACH_PORT.load(Ordering::SeqCst);
    if !port.is_null() {
        unsafe { CGEventTapEnable(port, true) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_env_defaults_to_right_option_when_unset() {
        let prev = std::env::var("SIDECAR_HOTKEY").ok();
        std::env::remove_var("SIDECAR_HOTKEY");
        assert_eq!(Hotkey::from_env(), Hotkey::RightOption);
        if let Some(p) = prev {
            std::env::set_var("SIDECAR_HOTKEY", p);
        }
    }

    #[test]
    fn from_env_parses_known_values() {
        for (input, expected) in [
            ("right-option", Hotkey::RightOption),
            ("left-option", Hotkey::LeftOption),
            ("right-command", Hotkey::RightCommand),
            ("fn", Hotkey::Fn),
            ("f5", Hotkey::F5),
        ] {
            std::env::set_var("SIDECAR_HOTKEY", input);
            assert_eq!(Hotkey::from_env(), expected, "input={input}");
        }
        std::env::remove_var("SIDECAR_HOTKEY");
    }

    #[test]
    fn from_env_falls_back_on_unknown() {
        std::env::set_var("SIDECAR_HOTKEY", "garbage");
        assert_eq!(Hotkey::from_env(), Hotkey::RightOption);
        std::env::remove_var("SIDECAR_HOTKEY");
    }

    #[test]
    fn keycodes_are_distinct() {
        let codes = [
            Hotkey::RightOption.keycode(),
            Hotkey::LeftOption.keycode(),
            Hotkey::RightCommand.keycode(),
            Hotkey::Fn.keycode(),
            Hotkey::F5.keycode(),
        ];
        let mut sorted = codes.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), codes.len(), "duplicate keycodes: {codes:?}");
    }
}
