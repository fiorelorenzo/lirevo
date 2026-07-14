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
    spec_from_env, ActivationMode, CaptureEvent, EdgeDetector, HotkeyEvent, HotkeySpec, LiveState,
    ModOnly, Modifier, ModifierFlags, Side, Trigger,
};

/// Recipient-level context key allowlist + hashing (platform-neutral, pure
/// logic — see module docs). Compiled on every target so it stays
/// unit-testable off macOS.
mod recipient;
pub use recipient::RecipientContext;

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
pub use hotkey::{HotkeyError, HotkeyListener};
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
    prompt_microphone, HotkeyError, HotkeyListener, InjectError, InjectionMethod, Injector,
    PermissionStatus,
};

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
pub use linux::{
    check_accessibility, check_microphone, clipboard, dev_skip_perms, prompt_accessibility,
    prompt_microphone, HotkeyError, HotkeyListener, InjectError, InjectionMethod, Injector,
    PermissionStatus,
};

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
mod stub;
#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub use stub::{
    check_accessibility, check_microphone, clipboard, dev_skip_perms, prompt_accessibility,
    prompt_microphone, HotkeyError, HotkeyListener, InjectError, InjectionMethod, Injector,
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

/// Resolves a recipient-level context key from the frontmost window's title,
/// for a small hard-coded bundle-id allowlist (Messages first — see
/// `recipient::is_allowlisted`). `bundle_id` should be the value just
/// obtained from [`frontmost_app`]'s `FrontmostApp::bundle_id`.
///
/// Returns `None` — silently, never an error — for any bundle id outside the
/// allowlist, or on any platform without Accessibility (AX) support; callers
/// must fall back to app-level (`target_bundle`) scoping in that case. Only
/// implemented on macOS today: Wayland/Linux and Windows have no equivalent
/// AX window-title API wired up, so they always return `None`.
///
/// `human_readable_label` mirrors the user's opt-in setting (off by
/// default): when `true`, the returned context's `label` carries the raw
/// window title; otherwise only the hashed `context_key` is populated.
#[must_use]
pub fn recipient_context_key(
    bundle_id: &str,
    human_readable_label: bool,
) -> Option<RecipientContext> {
    #[cfg(target_os = "macos")]
    {
        frontmost::recipient_context_key(bundle_id, human_readable_label)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (bundle_id, human_readable_label);
        None
    }
}
