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

use crate::permissions::{check_accessibility, PermissionStatus};

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
        if check_accessibility() != PermissionStatus::Granted {
            return Err(HotkeyError::PermissionDenied);
        }

        let (tx, rx) = mpsc::channel::<HotkeyEvent>(64);
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let runloop_slot: Arc<Mutex<Option<CFRunLoop>>> = Arc::new(Mutex::new(None));

        let shutdown_clone = shutdown_flag.clone();
        let runloop_slot_clone = runloop_slot.clone();

        let worker = thread::Builder::new()
            .name("hotkey-tap".into())
            .spawn(move || {
                hotkey_worker(hotkey, &tx, &shutdown_clone, &runloop_slot_clone);
            })
            .map_err(|e| HotkeyError::Internal(format!("spawn hotkey thread: {e}")))?;

        // Best-effort delay so the worker has a chance to install the tap and
        // publish its CFRunLoop handle before the caller starts producing events.
        thread::sleep(Duration::from_millis(50));

        Ok((
            Self {
                shutdown_flag,
                worker: Some(worker),
                runloop: runloop_slot,
            },
            rx,
        ))
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

fn hotkey_worker(
    hotkey: Hotkey,
    tx: &mpsc::Sender<HotkeyEvent>,
    shutdown: &Arc<AtomicBool>,
    runloop_slot: &Arc<Mutex<Option<CFRunLoop>>>,
) {
    let is_pressed = Arc::new(AtomicBool::new(false));
    let tx_cb = tx.clone();
    let is_pressed_cb = is_pressed.clone();
    let target_keycode = hotkey.keycode();

    let tap_callback =
        move |_proxy: CGEventTapProxy, event_type: CGEventType, event: &CGEvent| -> CallbackResult {
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
            // Always pass the event through — we are a passive observer.
            CallbackResult::Keep
        };

    let Ok(tap) = CGEventTap::new(
        CGEventTapLocation::HID,
        CGEventTapPlacement::HeadInsertEventTap,
        CGEventTapOptions::Default,
        vec![
            CGEventType::FlagsChanged,
            CGEventType::KeyDown,
            CGEventType::KeyUp,
        ],
        tap_callback,
    ) else {
        warn!("CGEventTap::new failed");
        return;
    };

    let Ok(runloop_source) = tap.mach_port().create_runloop_source(0) else {
        warn!("create_runloop_source failed");
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

    // Pump the run loop in short bursts so we can poll the shutdown flag.
    // SAFETY: see above — reading `kCFRunLoopCommonModes` static.
    let mode = unsafe { kCFRunLoopCommonModes };
    while !shutdown.load(Ordering::SeqCst) {
        let _ = CFRunLoop::run_in_mode(mode, Duration::from_millis(200), false);
    }
    // `tap` is dropped here; CFMachPort is invalidated by its Drop impl.
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
