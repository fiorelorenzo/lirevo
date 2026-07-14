//! Query the frontmost (focused) macOS application, and — for a small
//! allowlist of apps — a recipient-level context key derived from its
//! focused window's title.
//!
//! `frontmost_app` reads `NSWorkspace.frontmostApplication`, which returns
//! the `NSRunningApplication` that owns the active foreground window.
//! `recipient_context_key` additionally reads the focused window's `AXTitle`
//! via `AXUIElement` (Accessibility API) for allowlisted apps only — see
//! `recipient.rs` for the allowlist and hashing.

use core_foundation::base::{CFType, CFTypeRef, TCFType};
use core_foundation::string::{CFString, CFStringRef};
use objc2_app_kit::NSWorkspace;

use crate::recipient::{self, RecipientContext};
use crate::FrontmostApp;

/// The frontmost application as seen by `NSWorkspace`. `None` if there is no
/// frontmost application (rare — e.g. during fast app switches).
#[must_use]
pub fn frontmost_app() -> Option<FrontmostApp> {
    let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
    Some(FrontmostApp {
        name: app.localizedName().map(|s| s.to_string()),
        bundle_id: app.bundleIdentifier().map(|s| s.to_string()),
    })
}

/// Resolves a recipient-level context key from the frontmost app's focused
/// window title, restricted to [`recipient::is_allowlisted`] bundle ids.
///
/// Returns `None` (silently, not an error) when:
/// - `bundle_id` isn't in the allowlist,
/// - the frontmost app has changed since `bundle_id` was captured (avoids
///   attributing one app's window title to another),
/// - Accessibility isn't granted, or the AX read otherwise fails (no focused
///   window, empty title, etc).
///
/// `human_readable_label` mirrors the user's opt-in setting: when `true`,
/// [`RecipientContext::label`] carries the raw window title; otherwise only
/// the hashed `context_key` is populated.
#[must_use]
pub fn recipient_context_key(
    bundle_id: &str,
    human_readable_label: bool,
) -> Option<RecipientContext> {
    if !recipient::is_allowlisted(bundle_id) {
        return None;
    }

    let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
    if app.bundleIdentifier().map(|s| s.to_string()).as_deref() != Some(bundle_id) {
        // The frontmost app moved on between the caller's snapshot and this
        // call; don't attribute a stale bundle id to a different app's window.
        return None;
    }
    let pid: i32 = app.processIdentifier();

    let title = focused_window_title(pid)?;
    if title.trim().is_empty() {
        return None;
    }
    Some(recipient::build_context(&title, human_readable_label))
}

/// `AXError`. Only `Success` (0) is treated as success; every other value is
/// folded into `None` by the caller.
type AxError = i32;

#[repr(C)]
struct OpaqueAxUiElement {
    _private: [u8; 0],
}
type AxUiElementRef = *const OpaqueAxUiElement;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateApplication(pid: i32) -> AxUiElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AxUiElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> AxError;
    fn CFRelease(cf: CFTypeRef);
}

/// Reads `kAXFocusedWindowAttribute` → `kAXTitleAttribute` off the
/// application identified by `pid` via the Accessibility (AX) API.
///
/// `AXFocusedWindow`/`AXTitle` are `#define`d string macros in Apple's SDK
/// headers rather than linkable symbols (unlike e.g.
/// `kAXTrustedCheckOptionPrompt` in `permissions.rs`), so we construct the
/// `CFString`s from their documented literal values instead of linking
/// against a constant.
fn focused_window_title(pid: i32) -> Option<String> {
    // Safety: `AXUIElementCreateApplication` returns a new (+1) reference or
    // null; we release it via `CFRelease` before returning.
    let app_element = unsafe { AXUIElementCreateApplication(pid) };
    if app_element.is_null() {
        return None;
    }
    let result = (|| {
        let focused_window = copy_attribute(app_element, "AXFocusedWindow")?;
        // `AXFocusedWindow`'s value is itself an AXUIElementRef (a window),
        // wrapped as a CFTypeRef; reinterpret it to read attributes off it.
        let window_element = focused_window
            .as_concrete_TypeRef()
            .cast::<OpaqueAxUiElement>();
        let title = copy_attribute(window_element, "AXTitle")?;
        let cf_string = title.downcast::<CFString>()?;
        Some(cf_string.to_string())
    })();
    unsafe { CFRelease(app_element.cast()) };
    result
}

/// Copies an AX attribute value as an owned `CFType`, or `None` on any
/// `AXError` (attribute unsupported, no value, invalid element, etc).
fn copy_attribute(element: AxUiElementRef, attribute: &str) -> Option<CFType> {
    let attr = CFString::new(attribute);
    let mut value: CFTypeRef = std::ptr::null();
    let err = unsafe {
        AXUIElementCopyAttributeValue(element, attr.as_concrete_TypeRef(), &raw mut value)
    };
    if err != 0 || value.is_null() {
        return None;
    }
    // Safety: `AXUIElementCopyAttributeValue` returns a new (+1) reference on
    // success, matching `wrap_under_create_rule`'s ownership expectation.
    Some(unsafe { CFType::wrap_under_create_rule(value) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recipient_context_key_none_for_unlisted_bundle_id() {
        assert!(recipient_context_key("com.apple.mail", false).is_none());
        assert!(recipient_context_key("com.apple.mail", true).is_none());
    }

    #[test]
    fn recipient_context_key_none_when_allowlisted_app_isnt_frontmost() {
        // In CI/headless test runs the frontmost app is essentially never
        // Messages, so this exercises the "allowlisted but not frontmost"
        // fallback without needing a live AX session.
        assert!(recipient_context_key("com.apple.MobileSMS", false).is_none());
    }
}
