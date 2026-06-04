//! OS integration for the dictation app: hotkey, text injection,
//! accessibility / microphone permissions, and small native helpers
//! (audio cue, overlay window tweaks).
//!
//! Today only macOS has real implementations; non-macOS targets get a
//! stub module that returns `NotSupported` errors and `Denied`
//! permission status so the workspace compiles everywhere. Adding a new
//! platform means filling in a sibling module — consumer code keeps the
//! same imports.

#![warn(clippy::pedantic)]
#![allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]

#[cfg(target_os = "macos")]
mod frontmost;
#[cfg(target_os = "macos")]
mod permissions;
#[cfg(target_os = "macos")]
mod hotkey;
#[cfg(target_os = "macos")]
mod inject;
#[cfg(target_os = "macos")]
mod pasteboard;

#[cfg(target_os = "macos")]
pub use hotkey::{Hotkey, HotkeyEvent, HotkeyError, HotkeyListener};
#[cfg(target_os = "macos")]
pub use inject::{InjectError, Injector, InjectionMethod};
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

#[cfg(not(target_os = "macos"))]
mod stub;
#[cfg(not(target_os = "macos"))]
pub use stub::{
    check_accessibility, check_microphone, clipboard, dev_skip_perms, prompt_accessibility,
    prompt_microphone, Hotkey, HotkeyError, HotkeyEvent, HotkeyListener, InjectError,
    InjectionMethod, Injector, PermissionStatus,
};

pub mod audio_cue;
pub mod overlay;

/// A focused application identified by its localized name and bundle id.
/// Either field may be `None` if macOS doesn't report it (e.g. a process
/// without an `Info.plist`).
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
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}
