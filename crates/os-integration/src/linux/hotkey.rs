//! Push-to-talk hotkey listener via `evdev` (`/dev/input/event*`).
//!
//! Why evdev (and not an X-server grab):
//!   - It works on BOTH X11 and Wayland, because it reads the kernel input
//!     layer directly — below the display server.
//!   - It can bind a *lone* modifier held down (e.g. Right Alt / Right Meta),
//!     which is Lirevo's push-to-talk default. X grabs (`global-hotkey`) can't
//!     bind a bare modifier and have a release-order bug; Wayland has no
//!     global-grab protocol at all.
//!
//! Threading model (mirrors the macOS `CGEventTap` / Windows LL-hook design):
//!   - `install` enumerates `/dev/input/event*`, keeps the devices that report
//!     keys (and ideally the target key), sets each to non-blocking, and spawns
//!     one `hotkey-evdev` reader thread per device. There can be several
//!     keyboards (laptop builtin + external + virtual), and the same logical
//!     key may arrive on any of them, so we watch all of them.
//!   - Each reader polls its device for `EV_KEY` events matching the target
//!     `KeyCode`, translating value `1` (down) / `0` (up) into `HotkeyEvent`,
//!     swallowing value `2` (auto-repeat). A single shared `KEY_PRESSED` flag
//!     de-dups so that, across all devices, the consumer sees exactly one
//!     `Down` then one `Up` per physical press.
//!   - Events are forwarded over a `tokio::mpsc::Sender` with `try_send` (never
//!     block a reader thread).
//!   - `shutdown`/`Drop` set a shared stop flag; the readers observe it between
//!     non-blocking polls and exit, then we join them.
//!
//! PERMISSIONS: reading `/dev/input/event*` requires membership in the `input`
//! group (or root). If no event device can be opened, `install` returns
//! `HotkeyError::PermissionDenied` so the setup wizard can explain the
//! `input`-group requirement.
//!
//! UNVALIDATED: written and compile-checked on macOS via CI; no Down/Up
//! delivery has been observed on real Linux (X11 or Wayland).

use std::io::ErrorKind;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use evdev::{Device, EventSummary, EventType, KeyCode};
use thiserror::Error;
use tokio::sync::mpsc;
use tracing::{debug, warn};

/// How long a reader sleeps between non-blocking polls when no events are
/// pending. Small enough that push-to-talk latency stays imperceptible, large
/// enough to keep the idle reader threads off the CPU.
const POLL_INTERVAL: Duration = Duration::from_millis(8);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hotkey {
    RightOption,
    LeftOption,
    RightCommand,
    Fn,
    F5,
}

impl Hotkey {
    #[must_use]
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

    /// Map the cross-platform `Hotkey` to the evdev `KeyCode` we watch for. The
    /// names are macOS-flavoured (the shared enum was defined there); the
    /// nearest Linux equivalents are:
    ///   - `RightOption`  → Right Alt   (`KEY_RIGHTALT`)
    ///   - `LeftOption`   → Left Alt    (`KEY_LEFTALT`)
    ///   - `RightCommand` → Right Meta  (`KEY_RIGHTMETA`, the "Super"/Win key)
    ///   - `F5`           → F5          (`KEY_F5`)
    ///
    /// `Fn` has no evdev keycode on the vast majority of keyboards (the Fn key
    /// is handled in firmware and never reaches the kernel input layer), so it
    /// is unsupported here — see `install`.
    fn keycode(self) -> Option<KeyCode> {
        match self {
            Self::RightOption => Some(KeyCode::KEY_RIGHTALT),
            Self::LeftOption => Some(KeyCode::KEY_LEFTALT),
            Self::RightCommand => Some(KeyCode::KEY_RIGHTMETA),
            Self::F5 => Some(KeyCode::KEY_F5),
            Self::Fn => None,
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
        "the Fn key has no evdev keycode on most keyboards; choose a different push-to-talk key"
    )]
    UnsupportedKey,
    #[error(
        "cannot read /dev/input (need the `input` group): add your user with \
         `sudo usermod -aG input $USER`, then log out and back in"
    )]
    PermissionDenied,
    #[error("no readable input device exposes the chosen push-to-talk key")]
    NoMatchingDevice,
    #[error("internal: {0}")]
    Internal(String),
}

pub struct HotkeyListener {
    stop: Arc<AtomicBool>,
    readers: Vec<thread::JoinHandle<()>>,
}

impl HotkeyListener {
    pub fn install(hotkey: Hotkey) -> Result<(Self, mpsc::Receiver<HotkeyEvent>), HotkeyError> {
        let Some(target) = hotkey.keycode() else {
            return Err(HotkeyError::UnsupportedKey);
        };

        // Open every keyboard-like device that can emit the target key. We may
        // legitimately watch several at once (builtin + external + virtual).
        let devices = open_candidate_devices(target)?;

        let (tx, rx) = mpsc::channel::<HotkeyEvent>(64);
        let stop = Arc::new(AtomicBool::new(false));
        // Shared across all reader threads so a press observed on one device and
        // released on another (or duplicated across devices) still yields a
        // single Down/Up pair to the consumer.
        let pressed = Arc::new(AtomicBool::new(false));

        let mut readers = Vec::with_capacity(devices.len());
        for (path, device) in devices {
            let tx = tx.clone();
            let stop = Arc::clone(&stop);
            let pressed = Arc::clone(&pressed);
            let name = path.clone();
            let handle = thread::Builder::new()
                .name("hotkey-evdev".into())
                .spawn(move || {
                    reader_loop(&name, device, target, &tx, &stop, &pressed);
                })
                .map_err(|e| HotkeyError::Internal(format!("spawn hotkey thread: {e}")))?;
            readers.push(handle);
        }

        if readers.is_empty() {
            // open_candidate_devices already guards this, but stay defensive: a
            // listener with no readers would silently never fire.
            return Err(HotkeyError::NoMatchingDevice);
        }

        Ok((Self { stop, readers }, rx))
    }

    pub fn shutdown(mut self) {
        self.stop_and_join();
    }

    fn stop_and_join(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        for handle in self.readers.drain(..) {
            // Each reader wakes at most POLL_INTERVAL after the flag is set.
            let _ = handle.join();
        }
    }
}

impl Drop for HotkeyListener {
    fn drop(&mut self) {
        self.stop_and_join();
    }
}

/// Enumerate `/dev/input/event*` and return the devices that report keys and
/// (where the device advertises a key set) can emit `target`. Each returned
/// device is switched to non-blocking mode so the reader loop can poll it and
/// still observe the shutdown flag promptly.
///
/// Error contract:
///   - `PermissionDenied` if `/dev/input` exists but nothing in it is readable
///     (the classic "user not in the `input` group" case).
///   - `NoMatchingDevice` if devices are readable but none expose `target`.
fn open_candidate_devices(target: KeyCode) -> Result<Vec<(String, Device)>, HotkeyError> {
    let mut matching = Vec::new();
    let mut saw_device = false;
    let mut saw_readable = false;
    let mut saw_permission_denied = false;

    for (path, device) in evdev::enumerate() {
        saw_device = true;
        saw_readable = true; // it opened, so it's readable
        let path_str = path.to_string_lossy().into_owned();

        if let Some(dev) = probe_and_prepare(device, target) {
            debug!(device = %path_str, "hotkey: watching device");
            matching.push((path_str, dev));
        }
    }

    // `enumerate` won't surface EACCES (it just omits the device), so if it
    // yielded nothing OR yielded only devices we couldn't use, do a direct open
    // of the well-known event nodes to detect the permission case.
    if matching.is_empty() {
        for idx in 0..32 {
            let node = format!("/dev/input/event{idx}");
            match Device::open(&node) {
                Ok(dev) => {
                    saw_device = true;
                    saw_readable = true;
                    if let Some(dev) = probe_and_prepare(dev, target) {
                        debug!(device = %node, "hotkey: watching device (direct open)");
                        matching.push((node, dev));
                    }
                }
                Err(e) => {
                    if e.kind() == ErrorKind::PermissionDenied {
                        saw_device = true;
                        saw_permission_denied = true;
                    }
                    // NotFound (no such node) and any other transient error are
                    // simply skipped.
                }
            }
        }
    }

    if !matching.is_empty() {
        return Ok(matching);
    }
    if saw_permission_denied && !saw_readable {
        return Err(HotkeyError::PermissionDenied);
    }
    if saw_device {
        return Err(HotkeyError::NoMatchingDevice);
    }
    // No /dev/input nodes at all — treat as permission/availability problem and
    // point the user at the usual cause.
    Err(HotkeyError::PermissionDenied)
}

/// Decide whether `device` should be watched for `target`. Returns the device
/// (now switched to non-blocking mode so the reader can poll it) if it reports
/// the target key, or `None` for devices that don't qualify (a mouse/touchpad,
/// a keyboard lacking that key, or one we couldn't put in non-blocking mode).
fn probe_and_prepare(device: Device, target: KeyCode) -> Option<Device> {
    // Must be a key-reporting device at all.
    if !device.supported_events().contains(EventType::KEY) {
        return None;
    }
    // If the device advertises a key set, require it to include the target key.
    // Some virtual devices don't advertise a key set; we keep watching those
    // (None default = true) since they may still emit the key.
    let has_key = device
        .supported_keys()
        .is_none_or(|keys| keys.contains(target));
    if !has_key {
        return None;
    }
    if let Err(e) = device.set_nonblocking(true) {
        warn!(error = %e, "hotkey: failed to set non-blocking; skipping device");
        return None;
    }
    Some(device)
}

/// Per-device reader: poll for the target key's transitions until told to stop.
fn reader_loop(
    name: &str,
    mut device: Device,
    target: KeyCode,
    tx: &mpsc::Sender<HotkeyEvent>,
    stop: &AtomicBool,
    pressed: &AtomicBool,
) {
    while !stop.load(Ordering::SeqCst) {
        match device.fetch_events() {
            Ok(events) => {
                for ev in events {
                    if let EventSummary::Key(_, code, value) = ev.destructure() {
                        if code == target {
                            handle_transition(value, tx, pressed);
                        }
                    }
                }
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                // No events pending — nap, then re-check the stop flag.
                thread::sleep(POLL_INTERVAL);
            }
            Err(e) => {
                // Device went away (unplugged) or another fatal read error:
                // stop watching this one. The other readers keep running.
                warn!(device = %name, error = %e, "hotkey: device read error; reader exiting");
                break;
            }
        }
    }
    debug!(device = %name, "hotkey: reader stopped");
}

/// Translate an evdev key value into a deduplicated `Down`/`Up` event.
/// value 1 = press, 0 = release, 2 = auto-repeat (ignored).
fn handle_transition(value: i32, tx: &mpsc::Sender<HotkeyEvent>, pressed: &AtomicBool) {
    match value {
        1 => {
            // First transition only — swallow auto-repeat and cross-device dups.
            if !pressed.swap(true, Ordering::SeqCst) {
                send_event(tx, HotkeyEvent::Down);
            }
        }
        0 => {
            if pressed.swap(false, Ordering::SeqCst) {
                send_event(tx, HotkeyEvent::Up);
            }
        }
        _ => {} // value 2 = auto-repeat; ignore.
    }
}

/// Forward an event through the sender (best-effort, non-blocking).
fn send_event(tx: &mpsc::Sender<HotkeyEvent>, evt: HotkeyEvent) {
    match tx.try_send(evt) {
        Ok(()) => {}
        Err(mpsc::error::TrySendError::Full(_)) => {
            warn!("hotkey channel full; dropping event");
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            // Consumer gone; readers will stop on the next stop-flag check.
            debug!("hotkey channel closed; consumer gone");
        }
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
    fn supported_keys_map_to_distinct_keycodes() {
        let codes: Vec<u16> = [
            Hotkey::RightOption,
            Hotkey::LeftOption,
            Hotkey::RightCommand,
            Hotkey::F5,
        ]
        .into_iter()
        .map(|h| h.keycode().expect("supported key").code())
        .collect();
        let mut sorted = codes.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), codes.len(), "duplicate keycodes: {codes:?}");
    }

    #[test]
    fn fn_key_is_unsupported() {
        assert!(Hotkey::Fn.keycode().is_none());
    }

    #[test]
    fn transition_dedups_down_and_up() {
        let (tx, mut rx) = mpsc::channel::<HotkeyEvent>(8);
        let pressed = AtomicBool::new(false);

        handle_transition(1, &tx, &pressed); // down
        handle_transition(2, &tx, &pressed); // auto-repeat, ignored
        handle_transition(1, &tx, &pressed); // already pressed, ignored
        handle_transition(0, &tx, &pressed); // up
        handle_transition(0, &tx, &pressed); // already released, ignored

        assert!(matches!(rx.try_recv(), Ok(HotkeyEvent::Down)));
        assert!(matches!(rx.try_recv(), Ok(HotkeyEvent::Up)));
        assert!(rx.try_recv().is_err());
    }
}
