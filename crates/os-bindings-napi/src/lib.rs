//! Node addon wrapping audio-capture + os-integration for M3 Electron consumption.

#![deny(clippy::all)]

#[macro_use]
extern crate napi_derive;

use os_integration::PermissionStatus;

fn map_perm(s: PermissionStatus) -> String {
    match s {
        PermissionStatus::Granted => "granted",
        PermissionStatus::Denied => "denied",
        PermissionStatus::NotDetermined => "not_determined",
    }
    .to_string()
}

#[napi]
pub fn check_accessibility() -> String {
    map_perm(os_integration::check_accessibility())
}

#[napi]
pub fn prompt_accessibility() -> String {
    map_perm(os_integration::prompt_accessibility())
}

#[napi]
pub fn check_microphone() -> String {
    map_perm(os_integration::check_microphone())
}
