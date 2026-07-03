//! Windows backend for the OS integration surface.
//!
//! Mirrors the public symbols re-exported from `lib.rs` for macOS, implemented
//! against the Win32 API. The module layout intentionally parallels the macOS
//! tree (`hotkey`, `inject`, `permissions`, `clipboard`, `frontmost`) so the
//! two platforms read the same way.
//!
//! UNVALIDATED: every behaviour here was written and compile-checked on a
//! macOS host (CI cross-build only). No Windows runtime test has been run.
//!   - Global PTT hotkey Down/Up delivery via `WH_KEYBOARD_LL`.
//!   - Pasteboard-style injection (`SendInput` Ctrl+V) into real apps.
//!   - Overlay click-through (`overlay.rs`, Windows branch).
//!   - Frontmost-app lookup.
//!
//! Permission checks return `Granted` because Windows has no per-app TCC gate
//! for `SetWindowsHookEx` / `SendInput` (admin/UIPI caveats aside).

pub mod clipboard;
pub(crate) mod frontmost;
mod hotkey;
mod inject;
mod permissions;

pub use hotkey::{HotkeyError, HotkeyListener};
pub use inject::{InjectError, InjectionMethod, Injector};
pub use permissions::{
    check_accessibility, check_microphone, dev_skip_perms, prompt_accessibility, prompt_microphone,
    PermissionStatus,
};
