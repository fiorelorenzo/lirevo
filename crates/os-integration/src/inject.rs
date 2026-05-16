//! Injector stub — real impl in T10/T11/T12.

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InjectionMethod {
    Accessibility,
    Pasteboard,
}

#[derive(Debug, Error)]
pub enum InjectError {
    #[error("no focused application")]
    NoFocusedApp,
    #[error("no focused UI element")]
    NoFocusedElement,
    #[error("focused element does not accept text input")]
    NotTextEditable,
    #[error("accessibility permission denied")]
    PermissionDenied,
    #[error("pasteboard write failed: {0}")]
    PasteboardWrite(String),
    #[error("synthetic Cmd+V failed: {0}")]
    SyntheticPaste(String),
    #[error("internal: {0}")]
    Internal(String),
}

#[derive(Default)]
pub struct Injector {
    force_pasteboard: bool,
}

impl Injector {
    #[must_use]
    pub fn new() -> Self {
        Self { force_pasteboard: false }
    }

    #[must_use]
    pub fn with_force_pasteboard(force_pasteboard: bool) -> Self {
        Self { force_pasteboard }
    }

    pub fn inject(&self, _text: &str) -> Result<InjectionMethod, InjectError> {
        Err(InjectError::Internal(format!(
            "Injector lands in T10-T12 (force_pasteboard={})",
            self.force_pasteboard
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn injector_default_is_not_force_pasteboard() {
        let i = Injector::new();
        assert!(!i.force_pasteboard);
    }

    #[test]
    fn injector_with_force_pasteboard_sets_field() {
        let i = Injector::with_force_pasteboard(true);
        assert!(i.force_pasteboard);
    }
}
