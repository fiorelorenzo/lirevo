//! Cross-platform "start/stop recording" audio cues.
//!
//! Plays a short non-blocking sound at the moment we begin and end a
//! dictation capture. The cue is non-essential UX — failures are
//! intentionally swallowed.
//!
//! macOS uses the built-in `Tink.aiff` / `Pop.aiff` system sounds played
//! through `afplay`, which is always present and avoids pulling a full
//! audio playback crate into the host. Other platforms are no-ops today;
//! when porting to Linux/Windows, add a sibling impl (e.g. via `rodio`
//! with bundled sample assets) — consumers don't need to change.

#[derive(Clone, Copy, Debug)]
pub enum CueKind {
    Start,
    Stop,
}

/// Fire-and-forget. Returns immediately; the cue plays asynchronously and
/// any failure is logged at trace level and dropped.
pub fn play(kind: CueKind) {
    play_impl(kind);
}

#[cfg(target_os = "macos")]
fn play_impl(kind: CueKind) {
    let path = match kind {
        // Tink = soft confirmation click — the cleaner "ready to listen" cue.
        CueKind::Start => "/System/Library/Sounds/Tink.aiff",
        // Pop = thumpier, signals the act of release / commit.
        CueKind::Stop => "/System/Library/Sounds/Pop.aiff",
    };
    let _ = std::process::Command::new("/usr/bin/afplay").arg(path).spawn();
}

#[cfg(not(target_os = "macos"))]
fn play_impl(_kind: CueKind) {
    tracing::trace!("audio_cue::play: no implementation on this platform yet");
}
