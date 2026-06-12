//! Non-macOS stubs for the OS integration surface.
//!
//! Today the app ships macOS-only. This module keeps `os-integration`
//! compiling on Linux/Windows targets so that:
//!   - The workspace can be checked against other targets (e.g.
//!     `cargo check --target x86_64-unknown-linux-gnu`) to catch
//!     abstraction leaks the moment they land.
//!   - Future platform ports replace individual stub functions without
//!     rewriting consumers.
//!
//! Stubs return `NotSupported`-flavoured errors or `Denied` permission
//! status — never silently "granted" — so a consumer that forgets to
//! handle the gap is forced to deal with it.
//!
//! Stays public-symbol-compatible with the macOS module surface re-exported
//! from `lib.rs`.

use thiserror::Error;
use tokio::sync::mpsc;

use crate::hotkey_spec::HotkeyEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionStatus {
    Granted,
    Denied,
    NotDetermined,
}

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
        Self::RightOption
    }
}

#[derive(Debug, Error)]
pub enum HotkeyError {
    #[error("global hotkey listener not implemented on this platform")]
    NotSupported,
    #[error("internal: {0}")]
    Internal(String),
}

pub struct HotkeyListener;

impl HotkeyListener {
    pub fn install(_hotkey: Hotkey) -> Result<(Self, mpsc::Receiver<HotkeyEvent>), HotkeyError> {
        Err(HotkeyError::NotSupported)
    }

    pub fn shutdown(self) {}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InjectionMethod {
    Pasteboard,
}

#[derive(Debug, Error)]
pub enum InjectError {
    #[error("text injection not implemented on this platform")]
    NotSupported,
    #[error("internal: {0}")]
    Internal(String),
}

#[derive(Default)]
pub struct Injector;

impl Injector {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    pub fn inject(&self, _text: &str) -> Result<InjectionMethod, InjectError> {
        Err(InjectError::NotSupported)
    }
}

#[must_use]
pub fn dev_skip_perms() -> bool {
    false
}

#[must_use]
pub fn check_accessibility() -> PermissionStatus {
    PermissionStatus::Denied
}

#[must_use]
pub fn prompt_accessibility() -> PermissionStatus {
    PermissionStatus::Denied
}

#[must_use]
pub fn check_microphone() -> PermissionStatus {
    PermissionStatus::Denied
}

#[must_use]
pub fn prompt_microphone() -> PermissionStatus {
    PermissionStatus::Denied
}

pub mod clipboard {
    /// System clipboard write. Stub returns `false` so consumers know the
    /// fallback path is unavailable on this platform.
    pub fn set_text(_text: &str) -> bool {
        false
    }
}
