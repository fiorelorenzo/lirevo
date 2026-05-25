#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Embed Info.plist directly into the Mach-O __TEXT,__info_plist section so
// TCC permission prompts (microphone, accessibility) work when running the
// bare binary via `cargo run` / `tauri dev`. Without this, macOS cannot find
// NSMicrophoneUsageDescription and silently denies the request — the audio
// stream opens but produces zero samples.
//
// In production builds (`tauri build`), the .app bundle's Info.plist takes
// precedence; this embedded copy is harmless redundancy.
#[cfg(target_os = "macos")]
const INFO_PLIST_BYTES: &[u8] = include_bytes!("../Info.plist");

#[cfg(target_os = "macos")]
#[link_section = "__TEXT,__info_plist"]
#[used]
#[allow(dead_code)]
static INFO_PLIST: [u8; INFO_PLIST_BYTES.len()] = *include_bytes!("../Info.plist");

fn main() {
    lirevo_lib::run();
}
