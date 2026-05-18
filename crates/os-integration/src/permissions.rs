//! macOS Accessibility + microphone permission helpers.
//!
//! Uses raw FFI against the `ApplicationServices` framework (linked by build.rs)
//! to avoid binding-crate API churn. The exposed Rust API stays stable across
//! upstream changes; only the extern declarations would need updates if Apple
//! ever renamed the C symbols (extremely unlikely for these stable APIs).

use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::{CFString, CFStringRef};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionStatus {
    Granted,
    Denied,
    NotDetermined,
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: core_foundation::base::CFTypeRef) -> bool;

    /// Defined in `ApplicationServices` as a `CFStringRef` constant; the symbol is
    /// `kAXTrustedCheckOptionPrompt`. We declare it as a `CFStringRef` because
    /// the Rust linker resolves it to the `CFString` global.
    static kAXTrustedCheckOptionPrompt: CFStringRef;
}

/// Returns Granted if this process is trusted for Accessibility
/// (i.e., listed and toggled on in System Settings → Privacy & Security → Accessibility).
/// Used by `CGEventTap` installation + AX text injection.
#[must_use]
pub fn check_accessibility() -> PermissionStatus {
    if unsafe { AXIsProcessTrusted() } {
        PermissionStatus::Granted
    } else {
        PermissionStatus::Denied
    }
}

/// Shows the macOS Accessibility prompt sheet, asking the user to grant access.
/// Note: the user must manually toggle the switch and (likely) restart the binary.
/// Returns the current status — usually still `Denied`/`NotDetermined` immediately
/// after the prompt because the user hasn't responded yet.
#[must_use]
pub fn prompt_accessibility() -> PermissionStatus {
    unsafe {
        // Build dict { kAXTrustedCheckOptionPrompt: true }
        let key = CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt);
        let val = CFBoolean::true_value();
        let dict = CFDictionary::from_CFType_pairs(&[(key, val)]);
        let trusted = AXIsProcessTrustedWithOptions(dict.as_concrete_TypeRef().cast());
        if trusted {
            PermissionStatus::Granted
        } else {
            PermissionStatus::NotDetermined
        }
    }
}

/// Microphone permission via `AVCaptureDevice.authorizationStatusForMediaType:`.
/// Returns the current TCC state without prompting the user (cpal will trigger
/// the actual TCC prompt on first capture).
#[must_use]
pub fn check_microphone() -> PermissionStatus {
    use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};

    // Safety: `AVMediaTypeAudio` is a process-lifetime NSString constant.
    let media_type = unsafe { AVMediaTypeAudio.unwrap() };
    let status = unsafe { AVCaptureDevice::authorizationStatusForMediaType(media_type) };
    match status {
        AVAuthorizationStatus::Authorized => PermissionStatus::Granted,
        AVAuthorizationStatus::Denied | AVAuthorizationStatus::Restricted => {
            PermissionStatus::Denied
        }
        AVAuthorizationStatus::NotDetermined => PermissionStatus::NotDetermined,
        _ => PermissionStatus::NotDetermined,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_status_eq_and_debug() {
        assert_eq!(PermissionStatus::Granted, PermissionStatus::Granted);
        assert_ne!(PermissionStatus::Granted, PermissionStatus::Denied);
        let _ = format!("{:?}", PermissionStatus::Denied);
    }

    #[test]
    fn check_accessibility_returns_valid_status() {
        let s = check_accessibility();
        assert!(matches!(
            s,
            PermissionStatus::Granted | PermissionStatus::Denied | PermissionStatus::NotDetermined
        ));
    }

    #[test]
    fn check_microphone_returns_valid_status() {
        let s = check_microphone();
        assert!(matches!(
            s,
            PermissionStatus::Granted | PermissionStatus::Denied | PermissionStatus::NotDetermined
        ));
    }
}
