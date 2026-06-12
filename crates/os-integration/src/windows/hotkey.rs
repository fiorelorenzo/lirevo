//! Push-to-talk hotkey listener via a low-level keyboard hook
//! (`SetWindowsHookEx(WH_KEYBOARD_LL)`).
//!
//! Why not `RegisterHotKey`: it delivers no key-up message and cannot bind a
//! lone modifier (e.g. Right Alt / Right Win), which is exactly Lirevo's
//! push-to-talk default. A low-level keyboard hook sees every key transition
//! globally, so we can emit `Down` on the first key-down and `Up` on key-up
//! for the mapped virtual key.
//!
//! Threading model (mirrors the macOS `CGEventTap` design):
//!   - `install` spawns a dedicated `hotkey-hook` thread. LL hooks require the
//!     installing thread to own a running message loop, so that thread installs
//!     the hook then pumps `GetMessageW` until it receives `WM_QUIT`.
//!   - LL hook procs are global and must be a non-capturing `extern "system"`
//!     fn, so per-listener state (the tokio `Sender`, the target VK, the
//!     pressed flag) lives in module statics that the worker sets before the
//!     hook goes live and clears on teardown.
//!   - The proc translates `WM_KEYDOWN/WM_SYSKEYDOWN` → `Down` and
//!     `WM_KEYUP/WM_SYSKEYUP` → `Up` for the target VK, de-duping auto-repeat
//!     via the pressed flag, and forwards over a `tokio::mpsc::Sender`.
//!   - `shutdown`/`Drop` post `WM_QUIT` to the worker thread, which unhooks and
//!     returns, then we join it.
//!
//! UNVALIDATED: written and compile-checked on macOS via CI; no Down/Up
//! delivery has been observed on real Windows.

use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use thiserror::Error;
use tokio::sync::mpsc;
use tracing::warn;
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    VK_F1, VK_F10, VK_F11, VK_F12, VK_F2, VK_F3, VK_F4, VK_F5, VK_F6, VK_F7, VK_F8, VK_F9,
    VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, PostThreadMessageW, SetWindowsHookExW,
    TranslateMessage, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN,
    WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use crate::hotkey_spec::{HotkeyEvent, HotkeySpec, Modifier, Side, Trigger};

/// Map a `HotkeySpec` to the single Windows virtual key whose low-level key
/// transitions the hook watches for.
///
/// Windows keeps its single-key hook architecture for now, so only the subset
/// the hook can source is supported; everything else is rejected. The names are
/// macOS-flavoured (the neutral model was defined there); the nearest Windows
/// equivalents are Alt for Option, the Win key for Command, plus Control /
/// Shift:
///   - `ModifierOnly { modifier, side }` → the side-specific virtual key.
///   - `Key(name)` with no extra modifiers → its virtual key (F1–F12, A–Z).
///   - `Key(_)` WITH modifiers (a real combo) → unsupported: the single-key
///     hook can't source a chord. Combo sourcing is a deferred follow-up.
///   - `Fn` → unsupported: the Fn key has no software-visible virtual key
///     (handled in keyboard firmware, never reaches the OS input stack).
///   - `Mouse(_)` → unsupported: the keyboard hook watches keys only.
fn spec_to_vk(spec: &HotkeySpec) -> Result<VIRTUAL_KEY, HotkeyError> {
    match &spec.trigger {
        Trigger::ModifierOnly { modifier, side } => Ok(match (modifier, side) {
            (Modifier::Option, Side::Right) => VK_RMENU,
            (Modifier::Option, Side::Left) => VK_LMENU,
            (Modifier::Command, Side::Right) => VK_RWIN,
            (Modifier::Command, Side::Left) => VK_LWIN,
            (Modifier::Control, Side::Right) => VK_RCONTROL,
            (Modifier::Control, Side::Left) => VK_LCONTROL,
            (Modifier::Shift, Side::Right) => VK_RSHIFT,
            (Modifier::Shift, Side::Left) => VK_LSHIFT,
        }),
        Trigger::Key(name) if spec.modifiers.count() == 0 => {
            key_name_to_vk(name).ok_or(HotkeyError::UnsupportedKey)
        }
        // A real combo (base key + modifiers), Fn, or Mouse: the single-key
        // hook can't source any of these yet.
        Trigger::Key(_) | Trigger::Fn | Trigger::Mouse(_) => Err(HotkeyError::UnsupportedKey),
    }
}

/// Map a canonical base-key name to its Windows virtual key. Covers F1–F12 and
/// the letters A–Z (the keys the single-key hook can bind). Unknown names →
/// `None` (rejected by `spec_to_vk`). Letter VKs are the ASCII uppercase code
/// points (`'A'..='Z'`), which is exactly how Win32 numbers `VK_A`..`VK_Z`.
fn key_name_to_vk(name: &str) -> Option<VIRTUAL_KEY> {
    let code = match name {
        "F1" => VK_F1,
        "F2" => VK_F2,
        "F3" => VK_F3,
        "F4" => VK_F4,
        "F5" => VK_F5,
        "F6" => VK_F6,
        "F7" => VK_F7,
        "F8" => VK_F8,
        "F9" => VK_F9,
        "F10" => VK_F10,
        "F11" => VK_F11,
        "F12" => VK_F12,
        _ => {
            let mut chars = name.chars();
            match (chars.next(), chars.next()) {
                (Some(c @ 'A'..='Z'), None) => VIRTUAL_KEY(c as u16),
                _ => return None,
            }
        }
    };
    Some(code)
}

#[derive(Debug, Error)]
pub enum HotkeyError {
    #[error("the Fn key is not software-visible on Windows; choose a different push-to-talk key")]
    UnsupportedKey,
    #[error("a hotkey listener is already installed")]
    AlreadyInstalled,
    #[error("failed to install the low-level keyboard hook: {0}")]
    HookInstallFailed(String),
    #[error("internal: {0}")]
    Internal(String),
}

// --- Globals shared with the non-capturing hook proc -------------------------
//
// LL hook procs cannot capture environment, so per-listener state lives here.
// Only one listener exists at a time (the host drops the previous one before
// installing a replacement); `INSTALLED` enforces that invariant.

/// Currently-mapped target virtual key, stored as i64 (-1 = none).
static TARGET_VK: AtomicI64 = AtomicI64::new(-1);
/// Tracks whether the target key is currently held, to de-dup auto-repeat
/// `WM_KEYDOWN` floods into a single `Down`/`Up` pair.
static KEY_PRESSED: AtomicBool = AtomicBool::new(false);
/// Whether a listener is live. Guards against two concurrent installs.
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// Sender end of the tokio channel the hook proc forwards events to. Wrapped in
/// a mutex so the worker can set it before the hook goes live and clear it on
/// teardown; the proc only `try_send`s through a brief lock.
fn event_sender() -> &'static Mutex<Option<mpsc::Sender<HotkeyEvent>>> {
    static SENDER: OnceLock<Mutex<Option<mpsc::Sender<HotkeyEvent>>>> = OnceLock::new();
    SENDER.get_or_init(|| Mutex::new(None))
}

/// The installed hook handle, set on the worker thread once
/// `SetWindowsHookExW` succeeds so teardown can unhook it. Stored as a raw
/// pointer in an atomic because `HHOOK` isn't directly atomic; null = none.
static HOOK_HANDLE: std::sync::atomic::AtomicPtr<core::ffi::c_void> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

/// The low-level keyboard hook procedure. Runs on the worker thread inside its
/// message loop. Must be fast and must always chain via `CallNextHookEx`:
/// Windows silently removes an LL hook that takes longer than
/// `LowLevelHooksTimeout`, so we use `try_send` (never block) and never panic
/// across this FFI boundary.
unsafe extern "system" fn keyboard_hook_proc(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // HC_ACTION is 0; any negative code means "don't process, just chain".
    if code >= 0 {
        let result = std::panic::catch_unwind(|| {
            let target = TARGET_VK.load(Ordering::SeqCst);
            if target >= 0 {
                // SAFETY: for WH_KEYBOARD_LL, lParam points to a KBDLLHOOKSTRUCT
                // owned by the OS for the duration of the call.
                let kb = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
                #[allow(clippy::cast_possible_wrap)]
                if i64::from(kb.vkCode) == target {
                    // The window-message id occupies the low bits of WPARAM;
                    // truncating to u32 to compare against the WM_* constants is
                    // exactly what the Win32 message contract intends.
                    #[allow(clippy::cast_possible_truncation)]
                    let msg = wparam.0 as u32;
                    let is_down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
                    let is_up = msg == WM_KEYUP || msg == WM_SYSKEYUP;
                    if is_down {
                        // First transition only — swallow auto-repeat.
                        if !KEY_PRESSED.swap(true, Ordering::SeqCst) {
                            send_event(HotkeyEvent::Down);
                        }
                    } else if is_up && KEY_PRESSED.swap(false, Ordering::SeqCst) {
                        send_event(HotkeyEvent::Up);
                    }
                }
            }
        });
        if result.is_err() {
            // Never let a panic unwind across the C boundary.
            eprintln!("keyboard_hook_proc: panicked, suppressing to keep FFI boundary intact");
        }
    }

    // Always pass the event on — we are a passive observer and must not eat the
    // user's keystrokes.
    // SAFETY: ncode/wparam/lparam are forwarded verbatim; passing None for hhk
    // is valid and lets the system use the next hook in the chain.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Forward an event through the current sender (best-effort, non-blocking).
fn send_event(evt: HotkeyEvent) {
    if let Ok(guard) = event_sender().lock() {
        if let Some(tx) = guard.as_ref() {
            // try_send: dropping an event under backpressure is preferable to
            // stalling the global LL hook (which Windows would then evict).
            match tx.try_send(evt) {
                Ok(()) => {}
                Err(mpsc::error::TrySendError::Full(_)) => {
                    warn!("hotkey channel full; dropping event");
                }
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    warn!("hotkey channel closed; consumer gone");
                }
            }
        }
    }
}

pub struct HotkeyListener {
    worker_thread_id: u32,
    worker: Option<thread::JoinHandle<()>>,
}

impl HotkeyListener {
    // `spec` is taken by value to keep `install`'s signature uniform across
    // platforms (macOS consumes it); the Windows hook only needs the resolved
    // virtual key, so it borrows `spec` and drops the rest.
    #[allow(clippy::needless_pass_by_value)]
    pub fn install(spec: HotkeySpec) -> Result<(Self, mpsc::Receiver<HotkeyEvent>), HotkeyError> {
        let vk = spec_to_vk(&spec)?;

        // Enforce single-listener: the host's reinstall drops the previous
        // listener (joining its worker + clearing globals) before building a
        // new one, so this should never trip in normal flow.
        if INSTALLED.swap(true, Ordering::SeqCst) {
            return Err(HotkeyError::AlreadyInstalled);
        }

        let (tx, rx) = mpsc::channel::<HotkeyEvent>(64);

        // Publish target VK + sender + reset pressed state BEFORE the hook is
        // installed so the very first callback sees consistent state.
        TARGET_VK.store(i64::from(vk.0), Ordering::SeqCst);
        KEY_PRESSED.store(false, Ordering::SeqCst);
        *event_sender().lock().unwrap() = Some(tx);

        // Hand back the worker's thread id (for PostThreadMessage on shutdown)
        // and the install success/failure over a bounded handshake channel.
        let (init_tx, init_rx) =
            std::sync::mpsc::sync_channel::<Result<u32, HotkeyError>>(1);

        let worker = thread::Builder::new()
            .name("hotkey-hook".into())
            .spawn(move || {
                hook_worker(&init_tx);
            })
            .map_err(|e| {
                Self::clear_globals();
                HotkeyError::Internal(format!("spawn hotkey thread: {e}"))
            })?;

        match init_rx.recv_timeout(Duration::from_secs(2)) {
            Ok(Ok(worker_thread_id)) => Ok((
                Self {
                    worker_thread_id,
                    worker: Some(worker),
                },
                rx,
            )),
            Ok(Err(e)) => {
                let _ = worker.join();
                Self::clear_globals();
                Err(e)
            }
            Err(_) => {
                // Worker wedged during init: best-effort cleanup. We can't post
                // WM_QUIT without its thread id, so just detach and clear state.
                Self::clear_globals();
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
        if self.worker_thread_id != 0 {
            // SAFETY: posting WM_QUIT to the worker's message loop is always
            // valid; if the thread already exited it simply fails, which we
            // ignore.
            let _ = unsafe {
                PostThreadMessageW(self.worker_thread_id, WM_QUIT, WPARAM(0), LPARAM(0))
            };
            self.worker_thread_id = 0;
        }
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
        Self::clear_globals();
    }

    fn clear_globals() {
        TARGET_VK.store(-1, Ordering::SeqCst);
        KEY_PRESSED.store(false, Ordering::SeqCst);
        if let Ok(mut g) = event_sender().lock() {
            *g = None;
        }
        INSTALLED.store(false, Ordering::SeqCst);
    }
}

impl Drop for HotkeyListener {
    fn drop(&mut self) {
        self.stop_and_join();
    }
}

/// Worker: install the LL keyboard hook, report its thread id, then pump the
/// message loop until `WM_QUIT`. The hook MUST be installed on the same thread
/// that runs the loop — that is the whole reason this thread exists.
fn hook_worker(init_tx: &std::sync::mpsc::SyncSender<Result<u32, HotkeyError>>) {
    // SAFETY: SetWindowsHookExW with a static, non-null proc and hmod=None
    // (process-local hook) / dwThreadId=0 (system-wide) is the documented way
    // to install a global LL keyboard hook.
    let hook = unsafe {
        SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook_proc), None, 0)
    };

    let hook = match hook {
        Ok(h) => h,
        Err(e) => {
            let _ = init_tx.send(Err(HotkeyError::HookInstallFailed(e.to_string())));
            return;
        }
    };

    HOOK_HANDLE.store(hook.0, Ordering::SeqCst);

    // SAFETY: documented; returns this thread's id.
    let thread_id = unsafe { GetCurrentThreadId() };

    // Hook is live — report success + this thread's id for shutdown.
    let _ = init_tx.send(Ok(thread_id));

    // Pump the message loop. GetMessageW returns 0 on WM_QUIT (our shutdown
    // signal), -1 on error, otherwise >0. The loop also lets the OS deliver our
    // hook callbacks on this thread.
    let mut msg = MSG::default();
    loop {
        // SAFETY: msg is a valid MSG; hwnd=None pumps thread messages too.
        let r = unsafe { GetMessageW(&raw mut msg, None, 0, 0) };
        if r.0 <= 0 {
            // 0 = WM_QUIT, -1 = error. Either way, stop pumping.
            break;
        }
        // SAFETY: msg was just filled by GetMessageW.
        unsafe {
            let _ = TranslateMessage(&raw const msg);
            DispatchMessageW(&raw const msg);
        }
    }

    // Tear the hook down on the same thread that installed it.
    let stored = HOOK_HANDLE.swap(std::ptr::null_mut(), Ordering::SeqCst);
    if !stored.is_null() {
        // SAFETY: unhooking a handle we installed and have not yet unhooked.
        let _ = unsafe { UnhookWindowsHookEx(HHOOK(stored)) };
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
    fn spec_to_vk_maps_supported_and_rejects_rest() {
        assert_eq!(
            spec_to_vk(&spec(Trigger::ModifierOnly {
                modifier: Modifier::Option,
                side: Side::Right
            }))
            .unwrap(),
            VK_RMENU
        );
        assert_eq!(spec_to_vk(&spec(Trigger::Key("F5".into()))).unwrap(), VK_F5);
        assert!(spec_to_vk(&spec(Trigger::Fn)).is_err());
        assert!(spec_to_vk(&spec(Trigger::Mouse(4))).is_err());

        // A real combo (base key + modifiers) is not sourceable by the
        // single-key hook.
        let combo = HotkeySpec {
            modifiers: ModifierFlags {
                control: true,
                ..ModifierFlags::default()
            },
            trigger: Trigger::Key("F5".into()),
        };
        assert!(spec_to_vk(&combo).is_err());
    }
}
