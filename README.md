# Lirevo

[![ci](https://github.com/fiorelorenzo/lirevo/actions/workflows/ci.yml/badge.svg)](https://github.com/fiorelorenzo/lirevo/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/fiorelorenzo/lirevo?sort=semver)](https://github.com/fiorelorenzo/lirevo/releases/latest)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-macOS%20(Apple%20Silicon)-black)](#platform-support)

Fully local, open-source AI scribe for macOS (Apple Silicon). Push-to-talk
dictation that transcribes your speech, cleans up disfluencies in your own
language, and types the result into whatever app you are focused on. Everything
runs on-device. Zero cloud, zero account, zero telemetry.

**Pronunciation:** Lirevo — _lee-REH-voh_.

Inspired by Wispr Flow and Superwhisper, but built to learn your writing style
and grow into a personal agent. The roadmap is staged: a free, local-first
dictation app first (v0.5/v0.6), then a paid personal agent built on top of it
(v1.0). See the [CHANGELOG](CHANGELOG.md) for milestone status.

## Principles

- **Local.** Speech-to-text and the LLM cleanup both run on-device. Your audio
  and transcripts never leave your machine.
- **Private.** No account, no telemetry, no analytics, no crash reporting.
- **Yours.** Open source under [Apache-2.0](LICENSE). Models are downloaded from
  their upstream sources at first run, never bundled or proxied.

## Download

Lirevo ships as a signed, notarized arm64 `.dmg` for Apple Silicon Macs
(macOS 14 Sonoma or later).

1. **[Download the latest `.dmg` from GitHub Releases](https://github.com/fiorelorenzo/lirevo/releases/latest).**
2. Open the `.dmg` and drag `Lirevo.app` to `/Applications`.
3. Launch it. The notarized build opens without a Gatekeeper warning; if you
   built an un-notarized `.dmg` yourself, right-click the app and choose **Open**
   the first time (or run `xattr -d com.apple.quarantine /Applications/Lirevo.app`).
4. The in-app **setup wizard** then downloads the speech and cleanup models and
   walks you through the two required macOS permissions (Microphone and
   Accessibility).

> Releases are produced by a signed + notarized pipeline that runs on a `v*`
> tag. If no published release is listed yet, the link above resolves as soon as
> the first one is tagged, or you can build from source (below).

## Features

- **Push-to-talk dictation.** Hold a global hotkey (default: Right Option)
  anywhere on the system, speak, and release. Lirevo transcribes, cleans up, and
  types the result into the focused app.
- **Local speech-to-text.** Parakeet TDT v3 (25 European languages) runs fully
  on-device through our own [`parakeet-cpp`](https://github.com/fiorelorenzo/parakeet-cpp)
  Rust binding to [`parakeet.cpp`](https://github.com/mudler/parakeet.cpp) (ggml).
- **Style-aware cleanup.** A small local LLM (GGUF via llama.cpp) removes speech
  disfluencies and adds punctuation **without translating** — the output stays in
  the language you spoke. If cleanup is unconfigured or fails, the raw transcript
  is typed as-is.
- **Live overlay.** A transparent, notch-safe overlay shows a live waveform plus
  a streaming partial transcript while you speak, then a processing animation
  until the final text is injected.
- **Text injection.** Types at your cursor via the macOS Accessibility API, with
  a clipboard-paste fallback for apps that do not expose a standard text element.
- **Resource-aware Engine.** Models lazy-load on first use and unload when idle
  to free memory; the next dictation reloads them transparently.
- **Energy profiles.** Power Saver / Balanced / Performance (plus an Auto mode
  that watches battery, thermal, and memory pressure) control how long models
  stay resident, the LLM thread count, and when models unload on battery.
- **Smart Microphone.** Avoids forcing Bluetooth audio out of stereo (A2DP) into
  mono (HFP) by routing capture to a backup mic when your Bluetooth output is
  actively playing.
- **Dictation history.** Optional local SQLite history of every dictation, shown
  on the home screen. It never leaves your machine and can be cleared any time.
- **Native menu-bar app.** Lives in the menu-bar tray; a Dock icon appears only
  while a window is open and disappears when you close it back to the tray.
  Can optionally start minimized (tray-only) at login.

## Platform support

macOS (Apple Silicon) is the only platform where Lirevo is functional
end-to-end today. The codebase is kept portable so the v2 Linux + Windows ports
are a matter of adding sibling backends, not rewriting consumers.

| Platform | Status | Notes |
| --- | --- | --- |
| **macOS (Apple Silicon)** | **Available** | Fully functional. Signed + notarized `.dmg`. Metal GPU acceleration via dynamic ggml backends. |
| Linux | In progress (compile-only) | The workspace compiles and `os-integration` has real backends (evdev hotkey, enigo paste, arboard clipboard), but they are **compile-validated only, not runtime-tested**. Not usable yet. |
| Windows | In progress (compile-only) | The workspace compiles and `os-integration` has a Win32 backend (hotkey, inject, overlay), but it is **compile-validated only, not runtime-tested**. STT is static-linked there (dynamic backends deferred). Not usable yet. |

The CI cross-platform jobs build the shipped app on Ubuntu and Windows on every
push, which catches portability regressions. A passing check means the code
**compiles** there, not that Lirevo runs.

## Using the app

Lirevo lives in the menu-bar tray: hold your hotkey (default Right Option),
speak, and release, and the cleaned-up text is typed into the focused app.

For the full guide (the setup wizard, energy profiles, Smart Microphone,
dictation history, permissions, text-injection limitations, and which models get
downloaded), see **[docs/usage.md](docs/usage.md)**.

## Build from source

Requires macOS 14+ on Apple Silicon, plus Rust (pinned via `rust-toolchain.toml`),
Node 22, pnpm, `just`, `cargo-nextest`, and the Xcode command-line tools.

```bash
cd app && pnpm install
just dev      # run in development
just dmg      # signed + notarized .dmg (needs signing credentials)
```

See **[AGENTS.md](AGENTS.md)** for the full command list, the architecture, the
signing/notarization flow, and the cross-platform discipline.

## Documentation

- **[AGENTS.md](AGENTS.md)** — the source of truth for contributors and AI
  agents: tech stack, architecture, repository layout, the canonical `just`
  commands, cross-platform discipline, and the macOS signing/permission gotchas.
- **[docs/usage.md](docs/usage.md)** — the full user guide.
- **[docs/testing.md](docs/testing.md)** — test suites and the CI lint/test gate.
- **[CHANGELOG.md](CHANGELOG.md)** — milestone and roadmap status.

## Contributing

Contributions are welcome. Read **[AGENTS.md](AGENTS.md)** first. Never commit
directly to `main`: branch, open a PR, and run `just check`, `just lint`, and
`just test` before claiming a task done.

## About the name

**Lirevo** is a coined name in the Vercel/Stripe/Anthropic tradition:
pronounceable but with no pre-existing semantic baggage in any language, so it
can carry the brand entirely on its own meaning. Pronounced _lee-REH-voh_.

## License

[Apache-2.0](LICENSE). Copyright 2026 Lorenzo Fiore. See [NOTICE](NOTICE) for
third-party attributions.

A relicense to AGPL-3.0-or-later is planned for the public dictation release
(see the [CHANGELOG](CHANGELOG.md) roadmap); the current code is Apache-2.0.
