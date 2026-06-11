# Lirevo

[![build-mac](https://github.com/fiorelorenzo/lirevo/actions/workflows/build-mac.yml/badge.svg)](https://github.com/fiorelorenzo/lirevo/actions/workflows/build-mac.yml)
[![cross-platform-check](https://github.com/fiorelorenzo/lirevo/actions/workflows/cross-platform-check.yml/badge.svg)](https://github.com/fiorelorenzo/lirevo/actions/workflows/cross-platform-check.yml)
[![Latest release](https://img.shields.io/github/v/release/fiorelorenzo/lirevo?sort=semver)](https://github.com/fiorelorenzo/lirevo/releases/latest)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-macOS%20(Apple%20Silicon)-black)](#platform-support)

Fully local, open-source AI scribe for macOS (Apple Silicon). Push-to-talk
dictation that transcribes your speech, cleans up disfluencies in your own
language, and types the result into whatever app you are focused on. Everything
runs on-device. Zero cloud, zero account, zero telemetry.

**Pronunciation:** Lirevo — *lee-REH-voh*.

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
> the first one is tagged — or you can [build from source](#build-from-source).

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

A dedicated `cross-platform-check` CI job builds the shipped app on Ubuntu and
Windows on every push, which catches portability regressions early. A passing
check means the code **compiles** on those targets — it does **not** mean Lirevo
runs there.

## Using the app

Lirevo is a **menu-bar app**. A Dock icon appears while a window is open;
closing the home or settings window hides it back to the tray (the Dock icon
disappears) instead of quitting. Reopen it from the tray's **Show Lirevo** item.

### First run: the setup wizard

On first launch a short wizard gets you ready to dictate:

1. **Pick your dictation language.** The bundled speech model (Parakeet TDT v3)
   covers 25 European languages; auto-detect is the default.
2. **Download your models.** Two progress cards — the dictation (STT) model and
   the cleanup (LLM) model — with retry on error. You continue once both finish.
3. **Grant permissions.** Microphone (to capture audio) and Accessibility (for
   the global hotkey and to type into other apps). Accessibility must be toggled
   on manually in System Settings; macOS has no programmatic grant.
4. **Finish setup.** Choose your push-to-talk hotkey (default: Right Option), and
   toggle **Launch at login** and **Smart Microphone**.

### Push-to-talk dictation

Hold the hotkey anywhere on the system and speak. Release to transcribe, clean
up, and inject into the focused app. The pipeline runs in three stages:

1. **Speech-to-text** transcribes your audio, streaming a live partial transcript
   into the overlay while you talk.
2. **Cleanup** runs a small local LLM that removes disfluencies and adds
   punctuation **without translating**. If no cleanup model is configured, the
   raw transcript is typed as-is; if cleanup fails, Lirevo falls back to the raw
   transcript.
3. **Injection** types the text at your cursor via the macOS Accessibility API,
   with a clipboard-paste fallback for apps that do not expose a standard text
   element.

A transparent, notch-safe **overlay** appears centred on screen the moment you
start recording — a live waveform plus the streaming transcript while you speak,
then a processing animation that persists through transcription and cleanup
until the final text is injected.

### The menu-bar tray

The tray icon is a monochrome waveform whose amplitude encodes the active energy
profile (low = Power Saver, medium = Balanced, tall = Performance). It also
reflects state: an animated pulse while models load, a recording indicator while
you dictate, an error icon if models fail, and an attention badge when a required
permission is missing. The tray menu has a status line, the hotkey hint, an
**Energy Profile** submenu, **Show Lirevo**, **Settings…**, **Check for
updates**, and **Quit**.

### Energy profiles

Lirevo is resource-aware. An **Energy Profile** controls how long models stay
resident in memory, how many CPU threads the LLM uses, and when models are
unloaded on battery. Set it from the tray's Energy Profile submenu or in
**Settings → General → App**:

| Profile | LLM idle-unload | STT idle-unload | Models kept warm |
| --- | --- | --- | --- |
| Power Saver | 10 s | 60 s | no |
| Balanced (default) | 2 min | 5 min | yes |
| Performance | 10 min | 15 min | yes |

In **Auto** mode (the default), Lirevo watches battery level, AC state, thermal
and memory pressure, and foreground-app CPU, and picks the profile for you,
switching to Power Saver under pressure (a toast explains why). Models that get
idle-unloaded reload transparently on your next dictation, so they always appear
"ready".

### Smart Microphone

When your primary mic is a Bluetooth device (AirPods, say), opening it for
capture forces the Bluetooth link out of stereo (A2DP) into mono handsfree
(HFP), killing stereo playback for the duration. **Smart Microphone** (on by
default; **Settings → General → Dictation**) avoids this: if a Bluetooth output
is actively playing and your mic is also Bluetooth, dictation routes to a backup
mic (built-in by default, configurable) so your audio keeps playing in stereo.

### Dictation history

If **Record dictation history** is enabled (**Settings → General → App**), every
dictation is saved to a local SQLite database on your device and shown on the
home screen. Each entry expands to show the raw transcript, the cleaned text,
which models ran, the target app, the input device used, timings, and language.
History never leaves your machine; clear it any time from the home screen.

### Permissions

Lirevo needs two macOS permissions:

- **Microphone** — to capture your speech.
- **Accessibility** — to register the global hotkey and to type into other apps.

If either is missing, the home screen shows a warning banner and the tray icon
shows an attention badge, with buttons to grant or open the relevant System
Settings pane.

### Text injection: known limitations

- The **Accessibility (AXUIElement) path** is preferred and works in most native
  Cocoa apps (Safari, Notes, TextEdit, and similar). It inserts at the cursor or
  replaces the current selection.
- The **pasteboard fallback** kicks in automatically when the Accessibility path
  cannot reach an app's text element — typical for Electron apps (VS Code,
  Cursor, Slack, Discord) — or when you enable **Always use pasteboard** in
  Settings.
- During the pasteboard fallback the clipboard is temporarily overwritten with
  your text and then restored. **Non-string clipboard content (images, files) is
  not preserved** and is lost. Disable the fallback with **Always use
  pasteboard** only if you accept clipboard-paste for every injection.
- If injection lands before the target app is ready, raise the **Paste delay**
  slider (**Settings → General → Text injection**, default 120 ms).

## Architecture

Lirevo is a single Tauri 2 process. The frontend is Svelte 5 (runes) +
Tailwind v4 + shadcn-svelte running in WKWebView. The backend is a Rust
workspace; the Tauri host loads both inference engines **directly as in-process
libraries** — there is no sidecar process, Unix socket, or HTTP endpoint in the
shipped app.

- **Speech-to-text:** [`parakeet-cpp`](https://github.com/fiorelorenzo/parakeet-cpp),
  our own open-source Rust binding to [`parakeet.cpp`](https://github.com/mudler/parakeet.cpp)
  (ggml). The shipped model is `parakeet-tdt-0.6b-v3` (GGUF q4_k).
- **LLM cleanup:** llama.cpp via [`llama-cpp-2`](https://crates.io/crates/llama-cpp-2)
  (GGUF). The recommended default is a small Gemma model.
- **Dynamic GPU backends.** Both engines build their ggml backends as loadable
  modules (`GGML_BACKEND_DL`) and the app **auto-selects the best backend at
  runtime** — Metal on macOS, with a CPU fallback (Vulkan/CUDA on Linux/Windows
  when those targets land). The resolved backend for each engine is shown in
  **Settings → About**.
- **Resource-aware Engine.** A single `Engine` lazily loads and unloads each
  model on demand, driven by signals from the `resource-monitor` crate and the
  active energy profile, and auto-recovers on error.
- **OS integration.** Hotkey events flow from a CGEventTap thread (in
  `os-integration`) through a channel into a tokio task that owns the dictation
  state machine. Settings persist via `tauri-plugin-store`; history persists in
  local SQLite with a migration runner.

Cross-platform discipline (see [AGENTS.md](AGENTS.md)): platform-specific code
is gated behind abstractions in `os-integration` / `audio-capture` /
`resource-monitor` so the v2 Linux + Windows ports add sibling implementations
rather than rewriting consumers.

### Repository layout

```
.
├── app/                  # Tauri app (Svelte frontend + Tauri host — the shipped app)
│   ├── src/              # Svelte 5 UI (routes, components, stores, i18n)
│   └── src-tauri/        # Tauri host crate (Rust); STT + LLM run in-process here
├── crates/               # Rust workspace (8 crates)
│   ├── audio-capture/    # cpal mic capture, resampling to 16 kHz mono, Smart Mic
│   ├── inference-core/   # LLM (llama-cpp-2) wrapper + cleanup model catalog
│   ├── lirevo-cli/       # Dev-only CLI (inference-core sidecar client)
│   ├── lirevo-eval/      # Dev-only cleanup-stage eval harness
│   ├── lirevo-prompts/   # Versioned LLM cleanup prompts
│   ├── lirevo-prototype/ # Dev-only headless dictation pipeline
│   ├── os-integration/   # Hotkey, text injection, permissions, overlay (per-OS)
│   └── resource-monitor/ # Battery / thermal / memory / CPU signal broadcaster
├── scripts/              # Build / utility scripts
├── Justfile              # Canonical task entry points
└── Cargo.toml            # Workspace manifest
```

`lirevo-prototype`, `lirevo-cli`, `lirevo-eval`, and `inference-core`'s
HTTP/axum sidecar layer are **dev-only** — they are not bundled in the shipped
`.dmg`. Production code paths run inside the Tauri host (`app/src-tauri/`).

## Build from source

### Requirements

- macOS 14 (Sonoma) or later, on Apple Silicon
- Rust 1.88 (managed automatically via `rust-toolchain.toml`)
- Node 22 (managed via `.nvmrc`; CI pins the same)
- `just` (`brew install just`)
- `cargo-nextest` (`brew install cargo-nextest`)
- A C/C++ toolchain with `cmake` (for the vendored ggml builds), provided by the
  Xcode command-line tools

### First-time setup

```bash
cd app && npm install
```

### Common commands

Use `just` recipes — they are the contract CI runs. Run `just` with no args for
the full list.

| Goal                                        | Command                            |
| ------------------------------------------- | ---------------------------------- |
| Dev (HMR, no real TCC prompts)              | `just dev`                         |
| Dev with mocked permissions                 | `LIREVO_DEV_SKIP_PERMS=1 just dev` |
| Dev with real TCC prompts (debug `.app`)    | `just dev-bundle`                  |
| Release `.app` + signed/notarized `.dmg`    | `just dmg`                         |
| All tests (Rust nextest + Vitest)           | `just test`                        |
| Type check (Rust + Svelte)                  | `just check`                       |
| Format                                      | `just fmt`                         |
| Lint (clippy `-D warnings` + eslint)        | `just lint`                        |
| Wipe build caches                           | `just clean`                       |
| Reset runtime state (keeps models)          | `just reset`                       |
| Reset runtime state + delete models         | `just reset-all`                   |

`just reset` clears TCC grants, `settings.json`, and logs but keeps your
downloaded models; `just reset-all` also deletes the model files. Both refuse to
run while the app is alive.

### Signing and notarization

`just dmg` produces a release `.app` + `.dmg`. It **signs** the bundle when
`APPLE_SIGNING_IDENTITY` (a "Developer ID Application" cert) is set in an
untracked `.env`, and **notarizes + staples** it when Apple notarization
credentials are also present (App Store Connect API key, or Apple ID +
app-specific password). Without those it still exits 0 and produces an
un-notarized build (which runs on the build machine but is Gatekeeper-rejected
elsewhere). The signed + notarized distributable `.dmg` is produced by the
`release` workflow on a `v*` tag and uploaded to the GitHub Release. See
[AGENTS.md](AGENTS.md) for the full notarization ordering and the macOS
permission / dev-signing workflows.

## Model provisioning

The shipped app downloads everything for you from the setup wizard — there are
no files to place manually.

### Speech-to-text (STT)

A single model ships today (authoritative catalog:
`app/src-tauri/src/stt/catalog.rs`):

| Model | Size | Languages | License |
| --- | --- | --- | --- |
| **Parakeet TDT v3** (default) | ~644 MB | 25 European languages | CC-BY-4.0 |

The GGUF weights (`tdt-0.6b-v3-q4_k.gguf` from `mudler/parakeet-cpp-gguf`) are
downloaded into the app data directory's `models/` folder on first use. There
are no separate CoreML encoders to fetch — on Apple Silicon `parakeet-cpp`
accelerates via the dynamic ggml Metal backend.

### Language model (LLM cleanup)

LLM cleanup models are GGUF files downloaded in-app from an embedded catalog
(`crates/inference-core/data/model_catalog.json`):

| Model | Filename | Size | Recommended |
| --- | --- | --- | --- |
| Qwen3 4B | `Qwen3-4B-Instruct-2507-Q4_K_M.gguf` | ~2.5 GB | no |
| Llama 3.2 3B | `Llama-3.2-3B-Instruct-Q4_K_M.gguf` | ~2.0 GB | no |
| Qwen3 1.7B | `Qwen3-1.7B-Q4_K_M.gguf` | ~1.1 GB | no |
| **Gemma 3 1B** | `gemma-3-1b-it-Q4_K_M.gguf` | ~800 MB | **yes** |
| Gemma 3 270M | `gemma-3-270m-it-Q4_K_M.gguf` | ~250 MB | no |

The wizard downloads the recommended model (currently **Gemma 3 1B**) by
default. LLM files are stored under the app data directory's `models/` folder
(for example `~/Library/Application Support/Lirevo/models/`). You can also point
the **Models** tab at an existing `.gguf` file with the file picker. The LLM
context size is configurable in **Settings → Models → Advanced** (default 4096
tokens).

### Dev vs prod data and log directories

Debug builds use a distinct bundle id (`ai.lirevo.app.dev`) and app-name-suffixed
directories so they never touch the release app's models, history, settings, or
macOS system state:

| Build type | Data directory | Log directory |
| --- | --- | --- |
| Release (`just dmg`) | `~/Library/Application Support/Lirevo` | `~/Library/Logs/Lirevo` |
| Debug (`just dev`, `just dev-bundle`) | `~/Library/Application Support/Lirevo (Dev)` | `~/Library/Logs/Lirevo (Dev)` |

## Contributing

Contributions are welcome. **[AGENTS.md](AGENTS.md)** is the source of truth for
contributors (and AI coding agents): tech stack, repository layout,
cross-platform discipline, code conventions, the canonical `just` commands, and
the macOS permission / signing gotchas. Read it before opening a PR. Never commit
directly to `main` — branch and open a PR; run `just check` and `just test`
before claiming a task done.

Design documents (architecture spec, milestone specs, implementation plans) are
kept as local-only working docs under `docs/`. The public repository tracks only
code, configuration, README, CHANGELOG, LICENSE, and NOTICE.

## About the name

**Lirevo** is a coined name in the Vercel/Stripe/Anthropic tradition —
pronounceable but with no pre-existing semantic baggage in any language, so it
can carry the brand entirely on its own meaning. Pronounced *lee-REH-voh*.

The folder name `local-dictation-app/` is a legacy placeholder from before the
brand was chosen. It will be renamed when convenient; meanwhile every internal
reference uses `lirevo` / `ai.lirevo.app`.

## License

[Apache-2.0](LICENSE). Copyright 2026 Lorenzo Fiore. See [NOTICE](NOTICE) for
third-party attributions.

A relicense to AGPL-3.0-or-later is planned for the public dictation release
(see the [CHANGELOG](CHANGELOG.md) roadmap); the current code is Apache-2.0.
</content>
</invoke>
