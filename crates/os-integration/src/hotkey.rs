//! Hotkey stub — real impl lands in T9.

use thiserror::Error;
use tokio::sync::mpsc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hotkey {
    RightOption,
    LeftOption,
    RightCommand,
    Fn,
    F5,
}

impl Hotkey {
    pub fn from_env() -> Self {
        match std::env::var("SIDECAR_HOTKEY").ok().as_deref() {
            Some("right-option" | "RightOption") | None => Self::RightOption,
            Some("left-option" | "LeftOption") => Self::LeftOption,
            Some("right-command" | "RightCommand") => Self::RightCommand,
            Some("fn" | "Fn") => Self::Fn,
            Some("f5" | "F5") => Self::F5,
            Some(other) => {
                tracing::warn!(value = %other, "unknown SIDECAR_HOTKEY value, falling back to RightOption");
                Self::RightOption
            }
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
        "accessibility permission denied; grant via System Settings → Privacy → Accessibility, then restart"
    )]
    PermissionDenied,
    #[error("failed to create CGEventTap")]
    TapCreationFailed,
    #[error("internal: {0}")]
    Internal(String),
}

pub struct HotkeyListener {
    _private: (),
}

impl HotkeyListener {
    pub fn install(_hotkey: Hotkey) -> Result<(Self, mpsc::Receiver<HotkeyEvent>), HotkeyError> {
        Err(HotkeyError::Internal("HotkeyListener lands in T9".into()))
    }

    pub fn shutdown(self) {}
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
}
