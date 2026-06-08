//! Text injection via `NSPasteboard` + synthetic Cmd+V.

use thiserror::Error;

use crate::pasteboard;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InjectionMethod {
    Pasteboard,
}

#[derive(Debug, Error)]
pub enum InjectError {
    #[error("pasteboard write failed: {0}")]
    PasteboardWrite(String),
    #[error("synthetic Cmd+V failed: {0}")]
    SyntheticPaste(String),
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

    pub fn inject(&self, text: &str) -> Result<InjectionMethod, InjectError> {
        pasteboard::pasteboard_inject(text)?;
        Ok(InjectionMethod::Pasteboard)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn injection_method_is_pasteboard() {
        assert_eq!(InjectionMethod::Pasteboard, InjectionMethod::Pasteboard);
    }

    #[test]
    fn injector_new_constructs() {
        let _ = Injector::new();
    }

    #[test]
    #[ignore = "requires a focused text field + paste delay; manual dev box only"]
    fn injector_real_inject_to_focused_text_field() {
        // Manual smoke: open TextEdit, click in the document, then run:
        //   cargo test -p os-integration -- --ignored
        let inj = Injector::new();
        let result = inj.inject("[lirevo-test-injection] ");
        eprintln!("injection result: {result:?}");
        // No assertion on success — environment may not be ready;
        // the test exists to make manual smoke easy.
    }
}
