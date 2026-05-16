//! macOS OS integration for the dictation app: hotkey, text injection,
//! accessibility permission helpers.

#![warn(clippy::pedantic)]
#![allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]

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
    check_accessibility, check_microphone, prompt_accessibility, PermissionStatus,
};

#[cfg(not(target_os = "macos"))]
compile_error!("os-integration currently supports macOS only");
