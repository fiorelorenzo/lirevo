//! OS integration for the dictation app: hotkey, text injection,
//! accessibility / microphone permissions, and small native helpers
//! (audio cue, overlay window tweaks).
//!
//! macOS, Windows, and Linux have real implementations; remaining targets get
//! a stub module that returns `NotSupported` errors and `Denied` permission
//! status so the workspace compiles everywhere. Adding a new platform means
//! filling in a sibling module — consumer code keeps the same imports.
//!
//! NOTE: the Windows backend (`windows/`) and the Linux backend (`linux/`) are
//! both implemented on a macOS host and have only been compile-validated via
//! CI. None of their runtime behaviour (hotkey Down/Up delivery, synthetic
//! paste, overlay click-through, foreground-app lookup) has been exercised on
//! real Windows / Linux hardware. Treat them as unvalidated until smoke-tested
//! there. On Linux specifically, Wayland is best-effort by design (no global
//! input grab; synthetic paste depends on the compositor) — see `linux/mod.rs`.

#![warn(clippy::pedantic)]
#![allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]

pub mod hotkey_spec;
pub use hotkey_spec::{
    ActivationMode, CaptureEvent, EdgeDetector, HotkeyEvent, HotkeySpec, LiveState, ModOnly,
    Modifier, ModifierFlags, Side, Trigger,
};

#[cfg(target_os = "macos")]
mod frontmost;
#[cfg(target_os = "macos")]
mod hotkey;
#[cfg(target_os = "macos")]
mod inject;
#[cfg(target_os = "macos")]
mod pasteboard;
#[cfg(target_os = "macos")]
mod permissions;

#[cfg(target_os = "macos")]
pub use hotkey::{Hotkey, HotkeyError, HotkeyListener};
#[cfg(target_os = "macos")]
pub use inject::{InjectError, InjectionMethod, Injector};
#[cfg(target_os = "macos")]
pub use permissions::{
    check_accessibility, check_microphone, dev_skip_perms, prompt_accessibility, prompt_microphone,
    PermissionStatus,
};

/// System clipboard helpers (last-resort fallback when injection fails).
#[cfg(target_os = "macos")]
pub mod clipboard {
    pub use crate::pasteboard::set_text;
}

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "windows")]
pub use windows::{
    check_accessibility, check_microphone, clipboard, dev_skip_perms, prompt_accessibility,
    prompt_microphone, Hotkey, HotkeyError, HotkeyListener, InjectError, InjectionMethod, Injector,
    PermissionStatus,
};

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
pub use linux::{
    check_accessibility, check_microphone, clipboard, dev_skip_perms, prompt_accessibility,
    prompt_microphone, Hotkey, HotkeyError, HotkeyListener, InjectError, InjectionMethod, Injector,
    PermissionStatus,
};

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
mod stub;
#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub use stub::{
    check_accessibility, check_microphone, clipboard, dev_skip_perms, prompt_accessibility,
    prompt_microphone, Hotkey, HotkeyError, HotkeyListener, InjectError, InjectionMethod, Injector,
    PermissionStatus,
};

pub mod audio_cue;
pub mod overlay;

/// A focused application identified by its localized name and bundle id.
/// Either field may be `None` if the OS doesn't report it (e.g. a macOS
/// process without an `Info.plist`, or a Windows process whose image path
/// can't be read). On Windows `bundle_id` carries the full executable path
/// (there is no bundle-id concept).
#[derive(Debug, Clone)]
pub struct FrontmostApp {
    pub name: Option<String>,
    pub bundle_id: Option<String>,
}

/// The frontmost (focused) application, queried right after injection so it is
/// still the dictation target. `None` if it can't be determined.
#[must_use]
pub fn frontmost_app() -> Option<FrontmostApp> {
    #[cfg(target_os = "macos")]
    {
        frontmost::frontmost_app()
    }
    #[cfg(target_os = "windows")]
    {
        windows::frontmost::frontmost_app()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        None
    }
}
