//! Linux backend for the OS integration surface.
//!
//! Mirrors the public symbols re-exported from `lib.rs` for macOS/Windows. The
//! module layout intentionally parallels the other platform trees (`hotkey`,
//! `inject`, `permissions`, `clipboard`) so all three read the same way.
//!
//! Why these mechanisms:
//!   - **Hotkey** reads raw key transitions from `/dev/input/event*` via
//!     `evdev`. This is the only approach that works on BOTH X11 and Wayland
//!     and can bind a *lone* modifier held push-to-talk style. X-server key
//!     grabs (`global-hotkey`) can't bind a bare modifier and have a
//!     release-order bug, and Wayland has no global-grab protocol at all.
//!   - **Injection** uses the pasteboard model (clipboard snapshot → set text →
//!     synthetic Ctrl+V via `enigo` → restore), matching macOS/Windows.
//!   - **Permissions** are modelled as readability of `/dev/input` (the user
//!     must be in the `input` group for the hotkey reader to work); there is no
//!     TCC-style per-app gate on Linux.
//!
//! UNVALIDATED: every behaviour here was written and compile-checked on a macOS
//! host (CI cross-build only). No Linux runtime test has been run:
//!   - Global PTT hotkey Down/Up delivery via `evdev`.
//!   - Pasteboard-style injection (`enigo` Ctrl+V) into real apps.
//!   - Clipboard round-trip under both X11 and Wayland.
//!
//! WAYLAND CAVEATS (best-effort by design — degraded, not failing):
//!   - The `evdev` hotkey reader works on Wayland too (it reads the kernel
//!     input layer, below the display server), provided the user is in the
//!     `input` group. This is the main reason we use `evdev` rather than X grabs.
//!   - `enigo`'s Wayland backend is experimental and many compositors (notably
//!     GNOME/Mutter) refuse synthetic input, so the synthetic Ctrl+V paste may
//!     silently do nothing. The clipboard is still updated, so the user can
//!     paste manually. Under X11 `enigo` uses XTEST and works.
//!   - The recording overlay's always-on-top / click-through is not guaranteed
//!     under Wayland (no layer-shell wiring yet); see `overlay.rs`.

pub mod clipboard;
mod hotkey;
mod inject;
mod permissions;

pub use hotkey::{Hotkey, HotkeyError, HotkeyListener};
pub use inject::{InjectError, InjectionMethod, Injector};
pub use permissions::{
    check_accessibility, check_microphone, dev_skip_perms, prompt_accessibility, prompt_microphone,
    PermissionStatus,
};
