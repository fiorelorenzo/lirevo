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

use crate::hotkey_spec::{HotkeyEvent, HotkeySpec, Modifier, Side, Trigger};

/// How long a reader sleeps between non-blocking polls when no events are
/// pending. Small enough that push-to-talk latency stays imperceptible, large
/// enough to keep the idle reader threads off the CPU.
const POLL_INTERVAL: Duration = Duration::from_millis(8);

/// Map a `HotkeySpec` to the single evdev `KeyCode` the reader loop watches for.
///
/// Linux keeps its single-key reader architecture for now, so only the subset
/// the reader can source is supported; everything else is rejected:
///   - `ModifierOnly { modifier, side }` → the side-specific modifier keycode.
///     The names are macOS-flavoured (the neutral model was defined there); the
///     nearest Linux equivalents are Alt for Option, Meta (Super/Win) for
///     Command, plus Control / Shift.
///   - `Key(name)` with no extra modifiers → the matching evdev keycode
///     (F1–F12, letters A–Z) via `key_name_to_evdev`.
///   - `Key(_)` WITH modifiers (a real combo) → unsupported: the single-key
///     reader can't source a chord. Combo sourcing is a deferred follow-up.
///   - `Fn` → unsupported: the Fn key has no evdev keycode on most keyboards
///     (handled in firmware, never reaches the kernel input layer).
///   - `Mouse(_)` → unsupported: the evdev reader watches keyboards only.
fn spec_to_keycode(spec: &HotkeySpec) -> Result<KeyCode, HotkeyError> {
    match &spec.trigger {
        Trigger::ModifierOnly { modifier, side } => Ok(match (modifier, side) {
            (Modifier::Option, Side::Right) => KeyCode::KEY_RIGHTALT,
            (Modifier::Option, Side::Left) => KeyCode::KEY_LEFTALT,
            (Modifier::Command, Side::Right) => KeyCode::KEY_RIGHTMETA,
            (Modifier::Command, Side::Left) => KeyCode::KEY_LEFTMETA,
            (Modifier::Control, Side::Right) => KeyCode::KEY_RIGHTCTRL,
            (Modifier::Control, Side::Left) => KeyCode::KEY_LEFTCTRL,
            (Modifier::Shift, Side::Right) => KeyCode::KEY_RIGHTSHIFT,
            (Modifier::Shift, Side::Left) => KeyCode::KEY_LEFTSHIFT,
        }),
        Trigger::Key(name) if spec.modifiers.count() == 0 => {
            key_name_to_evdev(name).ok_or(HotkeyError::UnsupportedKey)
        }
        // A real combo (base key + modifiers), Fn, or Mouse: the single-key
        // reader can't source any of these yet.
        Trigger::Key(_) | Trigger::Fn | Trigger::Mouse(_) => Err(HotkeyError::UnsupportedKey),
    }
}

/// Map a canonical base-key name to its evdev `KeyCode`. Covers F1–F12 and the
/// letters A–Z (the keys the single-key reader can bind). Unknown names → `None`
/// (rejected by `spec_to_keycode`).
fn key_name_to_evdev(name: &str) -> Option<KeyCode> {
    let code = match name {
        "F1" => KeyCode::KEY_F1,
        "F2" => KeyCode::KEY_F2,
        "F3" => KeyCode::KEY_F3,
        "F4" => KeyCode::KEY_F4,
        "F5" => KeyCode::KEY_F5,
        "F6" => KeyCode::KEY_F6,
        "F7" => KeyCode::KEY_F7,
        "F8" => KeyCode::KEY_F8,
        "F9" => KeyCode::KEY_F9,
        "F10" => KeyCode::KEY_F10,
        "F11" => KeyCode::KEY_F11,
        "F12" => KeyCode::KEY_F12,
        "A" => KeyCode::KEY_A,
        "B" => KeyCode::KEY_B,
        "C" => KeyCode::KEY_C,
        "D" => KeyCode::KEY_D,
        "E" => KeyCode::KEY_E,
        "F" => KeyCode::KEY_F,
        "G" => KeyCode::KEY_G,
        "H" => KeyCode::KEY_H,
        "I" => KeyCode::KEY_I,
        "J" => KeyCode::KEY_J,
        "K" => KeyCode::KEY_K,
        "L" => KeyCode::KEY_L,
        "M" => KeyCode::KEY_M,
        "N" => KeyCode::KEY_N,
        "O" => KeyCode::KEY_O,
        "P" => KeyCode::KEY_P,
        "Q" => KeyCode::KEY_Q,
        "R" => KeyCode::KEY_R,
        "S" => KeyCode::KEY_S,
        "T" => KeyCode::KEY_T,
        "U" => KeyCode::KEY_U,
        "V" => KeyCode::KEY_V,
        "W" => KeyCode::KEY_W,
        "X" => KeyCode::KEY_X,
        "Y" => KeyCode::KEY_Y,
        "Z" => KeyCode::KEY_Z,
        _ => return None,
    };
    Some(code)
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
    // `spec` is taken by value to keep `install`'s signature uniform across
    // platforms (macOS/Windows consume it); the Linux reader only needs the
    // resolved keycode, so it borrows `spec` and drops the rest.
    #[allow(clippy::needless_pass_by_value)]
    pub fn install(spec: HotkeySpec) -> Result<(Self, mpsc::Receiver<HotkeyEvent>), HotkeyError> {
        let target = spec_to_keycode(&spec)?;

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

    use crate::hotkey_spec::ModifierFlags;

    fn spec(trigger: Trigger) -> HotkeySpec {
        HotkeySpec {
            modifiers: ModifierFlags::default(),
            trigger,
        }
    }

    #[test]
    fn spec_to_keycode_maps_supported_and_rejects_rest() {
        assert_eq!(
            spec_to_keycode(&spec(Trigger::ModifierOnly {
                modifier: Modifier::Option,
                side: Side::Right
            }))
            .unwrap(),
            KeyCode::KEY_RIGHTALT
        );
        assert_eq!(
            spec_to_keycode(&spec(Trigger::Key("F5".into()))).unwrap(),
            KeyCode::KEY_F5
        );
        assert!(spec_to_keycode(&spec(Trigger::Fn)).is_err());
        assert!(spec_to_keycode(&spec(Trigger::Mouse(4))).is_err());

        // A real combo (base key + modifiers) is not sourceable by the
        // single-key reader.
        let combo = HotkeySpec {
            modifiers: ModifierFlags {
                control: true,
                ..ModifierFlags::default()
            },
            trigger: Trigger::Key("F5".into()),
        };
        assert!(spec_to_keycode(&combo).is_err());
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
