//! Windows permission helpers.
//!
//! Windows has no per-app TCC-style gate for the capabilities Lirevo needs:
//! a low-level keyboard hook (`SetWindowsHookEx`) and synthetic input
//! (`SendInput`) do not require a consent prompt the way macOS Accessibility /
//! Input Monitoring do. The Windows 10/11 microphone privacy toggle only
//! affects apps that capture audio through the modern media APIs and is not
//! something this crate can usefully query per-process here, so we report
//! `Granted` and let the actual capture path surface any failure.
//!
//! There are real-world caveats this does NOT model (UNVALIDATED): synthetic
//! input from a normal-integrity process cannot reach an elevated window (UIPI),
//! and a hook installed by a 64-bit process won't see 32-bit-only message
//! pumps. Neither is a user-grantable permission, so they are out of scope for
//! a `PermissionStatus`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionStatus {
    Granted,
    Denied,
    NotDetermined,
}

/// Debug-build escape hatch matching the macOS implementation: when
/// `LIREVO_DEV_SKIP_PERMS` is set to a non-empty, non-"0" value all permission
/// checks/prompts short-circuit to `Granted`. Compiled out of release builds.
#[must_use]
pub fn dev_skip_perms() -> bool {
    #[cfg(debug_assertions)]
    {
        std::env::var("LIREVO_DEV_SKIP_PERMS")
            .ok()
            .is_some_and(|v| !v.is_empty() && v != "0")
    }
    #[cfg(not(debug_assertions))]
    {
        false
    }
}

#[must_use]
pub fn check_accessibility() -> PermissionStatus {
    PermissionStatus::Granted
}

#[must_use]
pub fn prompt_accessibility() -> PermissionStatus {
    PermissionStatus::Granted
}

#[must_use]
pub fn check_microphone() -> PermissionStatus {
    PermissionStatus::Granted
}

#[must_use]
pub fn prompt_microphone() -> PermissionStatus {
    PermissionStatus::Granted
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
    fn checks_report_granted() {
        assert_eq!(check_accessibility(), PermissionStatus::Granted);
        assert_eq!(check_microphone(), PermissionStatus::Granted);
    }
}
