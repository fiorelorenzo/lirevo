//! `NSPasteboard` helpers — real impl in T11.

use crate::inject::InjectError;

pub(crate) fn pasteboard_inject(_text: &str) -> Result<(), InjectError> {
    Err(InjectError::Internal("pasteboard_inject lands in T11".into()))
}
