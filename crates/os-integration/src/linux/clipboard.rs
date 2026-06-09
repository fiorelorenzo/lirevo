//! System clipboard write via `arboard` (last-resort fallback when injection
//! fails, plus the snapshot/restore helpers used by injection).
//!
//! `arboard` is built here with the `wayland-data-control` feature, so it talks
//! to X11 (default) and to Wayland compositors that implement the
//! `wlr-data-control` protocol. Mirrors the Windows clipboard module.

/// Last-resort clipboard write: replaces the clipboard's text content with
/// `text`. Returns `true` on success. Mirrors the macOS `pasteboard::set_text`.
#[must_use]
pub fn set_text(text: &str) -> bool {
    match arboard::Clipboard::new() {
        Ok(mut cb) => cb.set_text(text.to_owned()).is_ok(),
        Err(e) => {
            tracing::warn!(error = %e, "clipboard::set_text: failed to open clipboard");
            false
        }
    }
}

/// A best-effort snapshot of the clipboard's current text, used by injection so
/// the user's clipboard can be restored after the synthetic paste.
///
/// LIMITATION vs macOS: macOS snapshots every pasteboard type (images, files,
/// custom UTIs) and restores them all. `arboard` only round-trips text here, so
/// a non-text clipboard payload (image / files) is lost across an injection.
/// `None` means there was no readable text to restore.
#[must_use]
pub(crate) fn snapshot_text() -> Option<String> {
    match arboard::Clipboard::new() {
        Ok(mut cb) => cb.get_text().ok(),
        Err(_) => None,
    }
}

/// Restore a previously snapshotted clipboard text. No-op if the snapshot was
/// empty (nothing readable was on the clipboard before injection).
pub(crate) fn restore_text(snapshot: Option<String>) {
    if let Some(text) = snapshot {
        let _ = set_text(&text);
    }
}
