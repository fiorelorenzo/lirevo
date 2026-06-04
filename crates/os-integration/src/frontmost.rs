//! Query the frontmost (focused) macOS application.
//!
//! Used right after text injection to record which app the dictation went
//! into. Reads `NSWorkspace.frontmostApplication`, which returns the
//! `NSRunningApplication` that owns the active foreground window.

use objc2_app_kit::NSWorkspace;

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
