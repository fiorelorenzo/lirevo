//! Recipient-level context key resolution.
//!
//! For a small allowlist of apps where each window represents a distinct
//! conversation partner (Messages first), style learning can scope by
//! *recipient* rather than just by app. This module holds the
//! platform-neutral half of that: the allowlist gate and the hashing that
//! turns a raw window title into a privacy-preserving `context_key`. It is
//! compiled unconditionally (no `#[cfg(target_os = ...)]`) so it stays
//! unit-testable on every target, even though only macOS can currently
//! produce a window title to feed it (see `frontmost.rs`'s
//! `recipient_context_key`, and the stub in `lib.rs` for other platforms).
//! On non-macOS targets nothing calls these yet — allow the resulting
//! `dead_code` there rather than gating the module itself, so the unit
//! tests below keep compiling (and could run) everywhere.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use sha2::{Digest, Sha256};

/// Bundle ids allowed to resolve a recipient-level context key from their
/// focused window's title. Deliberately small and explicit: reading window
/// titles is more privacy-sensitive than the existing app-level lookup
/// (`frontmost_app`), so this only runs for apps where the title reliably
/// names a conversation partner rather than document content, a URL, a file
/// path, etc. Messages first; extend deliberately, one app at a time.
const ALLOWLIST: &[&str] = &["com.apple.MobileSMS"];

/// A recipient-level context key derived from an allowlisted app's focused
/// window title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipientContext {
    /// Stable, non-reversible identifier for the window title (SHA-256,
    /// truncated). Safe to persist unconditionally — this is what callers
    /// should use to scope style-learning context by recipient.
    pub context_key: String,
    /// The raw window title, populated only when the caller opts into a
    /// human-readable label (off by default). `None` otherwise: never store
    /// raw window titles unhashed without that explicit opt-in.
    pub label: Option<String>,
}

/// Whether `bundle_id` is allowed to resolve a recipient-level context key.
/// Callers outside [`ALLOWLIST`] must fall back to app-level scoping.
#[must_use]
pub fn is_allowlisted(bundle_id: &str) -> bool {
    ALLOWLIST.contains(&bundle_id)
}

/// Hashes a window title into a stable, non-reversible context key.
///
/// Truncated to 16 hex chars (64 bits) of SHA-256 — collision resistance is
/// plenty for scoping a handful of recipients per app, while keeping the
/// persisted key short.
#[must_use]
pub fn hash_title(title: &str) -> String {
    let digest = Sha256::digest(title.as_bytes());
    format!("{digest:x}")[..16].to_string()
}

/// Builds a [`RecipientContext`] from a raw window title read off an
/// allowlisted app's focused window, honoring the human-readable-label
/// opt-in. `title` is never stored as-is unless `human_readable_label` is
/// `true`.
#[must_use]
pub fn build_context(title: &str, human_readable_label: bool) -> RecipientContext {
    RecipientContext {
        context_key: hash_title(title),
        label: human_readable_label.then(|| title.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_gates_messages_only() {
        assert!(is_allowlisted("com.apple.MobileSMS"));
        assert!(!is_allowlisted("com.apple.mail"));
        assert!(!is_allowlisted("com.tinyspeck.slackmacgap"));
        assert!(!is_allowlisted(""));
    }

    #[test]
    fn hash_is_stable_and_not_the_raw_title() {
        let a = hash_title("Jane Doe");
        let b = hash_title("Jane Doe");
        assert_eq!(a, b);
        assert_ne!(a, "Jane Doe");
        assert_eq!(a.len(), 16);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn hash_differs_for_different_titles() {
        assert_ne!(hash_title("Jane Doe"), hash_title("John Smith"));
    }

    #[test]
    fn build_context_hides_label_by_default() {
        let ctx = build_context("Jane Doe", false);
        assert_eq!(ctx.label, None);
        assert_eq!(ctx.context_key, hash_title("Jane Doe"));
    }

    #[test]
    fn build_context_reveals_label_only_when_opted_in() {
        let ctx = build_context("Jane Doe", true);
        assert_eq!(ctx.label.as_deref(), Some("Jane Doe"));
        assert_eq!(ctx.context_key, hash_title("Jane Doe"));
    }
}
