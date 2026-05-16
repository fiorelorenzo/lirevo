//! Permissions stub — real impl lands in T8.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionStatus {
    Granted,
    Denied,
    NotDetermined,
}

#[must_use]
pub fn check_accessibility() -> PermissionStatus {
    PermissionStatus::NotDetermined
}

#[must_use]
pub fn prompt_accessibility() -> PermissionStatus {
    PermissionStatus::NotDetermined
}

#[must_use]
pub fn check_microphone() -> PermissionStatus {
    PermissionStatus::NotDetermined
}
