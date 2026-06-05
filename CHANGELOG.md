# Changelog

All notable changes to this project are documented in this file. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project adheres
to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.6.0] - Unreleased — v0.6: energy profiles, resource-aware Engine, history, native menu-bar posture

### Added
- **Resource-aware Engine lifecycle.** A single `Engine` (`app/src-tauri/src/engine/`)
  now owns both inference backends. It lazy-loads STT/LLM on first dictation,
  auto-recovers from a failed load, and unloads idle models to free memory. A
  `lifecycle_loop` ticks every 5 s against `resource_monitor::Signals` and runs
  a pure `lifecycle_decision` resolver (`engine/decision.rs`) that returns
  unload / reload / preload actions. Triggers include critical memory pressure,
  low free RAM (< 2048 MB), a heavy foreground app, on-battery below the profile
  threshold, and per-profile idle timeouts. STT is never unloaded mid-dictation
  (the streaming worker holds the slot lock). Idle unloads emit an informational
  `engine:llm_state_changed` Tauri event and never regress the user-facing
  `model:state` — the next dictation transparently reloads.
- **`resource-monitor` crate.** Cross-platform system-resource broadcaster: emits
  `Signals` snapshots (battery %, thermal state, memory pressure, free RAM,
  system CPU %, foreground-app CPU %) over a tokio broadcast channel. Real
  sensors under `crates/resource-monitor/src/macos/`; a no-op stub on every other
  target so the workspace compiles everywhere.
- **Energy profiles** (`power_saver` / `balanced` / `performance`, plus an `auto`
  resolver). `inference_core::profile::ProfileSelector` scores `ResourceMonitor`
  signals with 30 s hysteresis, an emergency bypass (Low Power Mode, thermal
  Critical, memory Critical, battery < 5%), and a user-pinnable mode; on change
  it pushes the matching `ProfilePolicy` (idle-unload timeouts, LLM thread count,
  STT precision, battery threshold) into the Engine. Each profile's knobs:
  Power Saver 10 s LLM / 60 s STT idle-unload, eco-only cores, unload below 50%
  battery; Balanced 120 s / 300 s, mixed cores, unload below 20%; Performance
  600 s / 900 s, all P-cores, never unloads on battery. Configurable from
  **Settings → General → App → Energy Profile** or the tray's Energy Profile
  submenu via the `apply_profile_mode` command. Balanced and Performance keep
  models warm; Power Saver skips warm-up.
- **Smart Microphone.** When a Bluetooth output is actively playing and the
  configured mic is also Bluetooth, dictation routes capture to a backup mic
  (built-in by default) so the Bluetooth link stays in A2DP stereo instead of
  dropping to mono HFP. `audio_capture::choose_input_device` runs on every
  hotkey-down; a rerouted capture is recorded in the history row as
  `smart_routing_applied`. New settings: `smart_mic_routing` (default on) and an
  optional `backup_input_device` (null = built-in auto). Configurable in the
  wizard's final step and in **Settings → General → Dictation**.
- **Dictation history.** Successful dictations are saved to a local SQLite
  database (`app/src-tauri/src/db/`, `rusqlite` + `rusqlite-migration`) with an
  append-only migration system (`001_dictations`, `002_smart_routing`). The home
  screen gained an infinite-scroll history list with per-row previews, model
  badges, target app, timings, an expandable detail view, single-row delete, and
  a "Clear" action. `history_*` Tauri commands back the UI; a `dictation:saved`
  event refreshes the list live. New `record_history` setting (default on); the
  history write is best-effort and never blocks the pipeline.
- **Redesigned menu-bar tray.** Monochrome waveform template icons whose
  amplitude encodes the active energy profile (low = Power Saver, medium =
  Balanced, tall = Performance), an animated loading pulse, recording / loading /
  error states, and a permission-attention badge shown when Accessibility or
  Microphone is missing (polled independently of any open window). The menu adds
  an Energy Profile submenu (Auto / Power Saver / Balanced / Performance with the
  resolved profile) and a "Check for updates" item.
- **Native menu-bar app posture.** The app runs with `ActivationPolicy::Accessory`
  (no Dock icon); the tray is the only persistent presence. Closing the home or
  settings window hides it (`api.prevent_close()` + `hide()`) instead of quitting.
  Launch-at-login (via `tauri-plugin-autostart`, `MacosLauncher::LaunchAgent`)
  passes a `--minimized` argument so an autostarted launch opens no window and
  stays silently in the tray.
- **`lirevo-eval` crate.** Dev-only evaluation harness for the LLM cleanup stage:
  loads JSONL corpora, runs them against configurable backends, scores output
  (chrF, length-ratio, embedding-cosine, assertion checks), and produces judge
  reports. Subcommands `run`, `gen-corpus`, `judge`, `bless`, `bake-cell`; driven
  by the `just eval` recipe. Not bundled in the DMG.
- **Quit-safety `atexit` handler** (`register_quit_safety_atexit` in `lib.rs`)
  that flips `LIREVO_EXIT_REQUESTED` before `ggml_metal`'s C++ destructor runs,
  preventing a spurious `SIGABRT` on app quit.

### Changed
- **In-process inference, no sidecar in the shipped app.** Both `LlamaBackend`
  (`llama-cpp-2`) and `audiopipe::Model` are loaded directly into the Tauri host
  process. There is no child process, Unix socket, or HTTP endpoint in the
  shipped DMG. `inference-core`'s axum sidecar, `lirevo-cli`, `lirevo-prototype`,
  and `lirevo-eval` are all dev-only.
- **Cleanup prompt** (`lirevo-prompts::build_clean_system_prompt`) now edits out
  speech disfluencies and adds punctuation while preserving the dictation
  language — it never translates. When no LLM is configured or cleanup fails, the
  raw transcript is injected as-is.
- **Dictation overlay** is notch-safe and persists through a `processing` phase
  (driven by `overlay:phase` events: `recording` → `processing` → `done`) until
  the final text is injected, instead of dismissing after recording stops. An
  RAII `OverlayPhaseGuard` guarantees the overlay dismisses on every exit path.
- **Dev/prod data + log directories are named after the app** (`paths.rs`):
  `Lirevo` for release, `Lirevo (Dev)` for debug, replacing the bundle-id leaf.
  Debug builds get a one-time migration of the old `ai.lirevo.app` directory to
  `Lirevo (Dev)`, preserving existing dev models and history.
- **Distinct debug bundle id `ai.lirevo.app.dev`** (injected via
  `--config '{"identifier":"ai.lirevo.app.dev"}'` by `just dev` and
  `just dev-bundle`) so debug builds never share macOS system state
  (Caches / WebKit / Preferences / TCC) with the release app.
- **Stable dev code-signing.** `just dev-bundle` re-signs the bundle with a
  Developer ID identity (from `APPLE_SIGNING_IDENTITY` in `.env`) **without**
  hardened runtime, so the bundled native inference libraries (ggml/Metal,
  whisper, llama, audiopipe) still load and TCC grants persist across rebuilds.
  Without an identity it falls back to ad-hoc and resets TCC for
  `ai.lirevo.app.dev` each build.
- **Tray menu** dropped the "Re-run setup wizard" and "View logs" items;
  "Energy" was renamed to "Energy Profile". Re-running the wizard now lives in
  **Settings → About**.

### Removed
- **`keep_models_warm` setting and toggle.** Warm-up is now derived from the
  active energy profile (Balanced/Performance warm; Power Saver does not).
- M3-era tray "Re-run setup wizard" / "View logs" menu items (see Changed).

## [Unreleased]

### Roadmap (next milestones)

The project is evolving into a **personal agent that learns how you write and helps you write more like yourself, everywhere**. The roadmap reflects this in two staged ships:

- **v0.5 (~month 3-4): free dictation public** — clean, fast, local-first dictation app with style learning, released as the OSS foundation. Free forever under AGPL-3.0-or-later. Serves as both the audience-building loss leader and the runtime base for the agent.
- **v1.0 (~month 10-12): paid agent launch** — full personal agent built on top of the dictation base. Observes (opt-in), learns, acts. Paid €129 one-time perpetual license. Free dictation users get conversion path; agent buyers get the loss-leader features included.

The inference stack runs on Rust-native, multi-vendor foundations to support both v0.5 and v1.0. M4 (STT swap to audiopipe) shipped in 0.5.0. M5 attempted an LLM-runtime swap to mistral.rs but was abandoned — the app stays on `llama-cpp-2` (GGUF); two salvage upstream PRs to mistral.rs came out of it (build fix #2176, tokenizer #2177). The v0.6 line that replaced it (single llama.cpp backend + resource-aware Engine lifecycle + energy profiles, all in this changelog's unreleased section) is the current dictation work. Next the agent stack is built out (M6-M7), then polish + license + launch (M8-M10).

- **M5 — LLM runtime (resolved on `llama-cpp-2`).** The planned migration to `mistral.rs` + a Gemma 4 default was attempted and rolled back; Lirevo keeps `llama-cpp-2` and the in-app GGUF catalog (recommended default: `gemma-3-1b-it-Q4_K_M.gguf`, blessed by `lirevo-eval`). The energy this would have spent on a backend swap went into the v0.6 resource-aware lifecycle and energy-profile work instead. **The v0.5 free dictation public release sits at the end of this dictation track.**
- **M6 — Agent core.** Builds on the generic local SQLite layer + migration runner already shipped in v0.6 (`app/src-tauri/src/db/`): adds screen capture infrastructure (custom module on cidre, AGENTS.md-compliant cross-platform abstraction) + vision-based style learning + retrieval foundations (vector DB local choice TBD via M6 brainstorm) + hierarchical context (per-app + per-window-title + per-recipient where detectable). Capture cadence configurable, screenshots discarded after feature extraction (storage as structured data only).
- **M7 — Agent UX.** Agent Console as full-screen overlay summoned by `Cmd+Shift+Space` (Spotlight/Raycast-style). Search/retrieve over learned activity. "What I've learned" inspector for transparency. Manual teach hotkey ("this is how I write"). Privacy UX polish.
- **M8 — Polish & Reliability + Mac optimizations.** VAD silence-detection auto-stop, custom hotkey combos wired to settings UI, per-app force-pasteboard overrides, model download resume on cancel/network failure, QoS user-interactive, diagnostic panel. (The memory-pressure / thermal / Low Power Mode handlers, polished waveform tray icons, and live-level wizard "Test mic" originally scoped here shipped early in the v0.6 line.)
- **M9 — License & Payment infrastructure.** OAuth flow (Google/GitHub/Apple) via custom URL scheme callback + offline JWT 365-day opt-in. License backend Rust+axum on Hetzner Cloud (~€15/mo all-in). Lemonsqueezy as Merchant of Record. Paywall UX with three tiers (Free / Cloud Sync €4/mo / Agent €129 one-time). Privacy Inspector UI showing real-time network activity for auditability.
- **M10 — Beta + v1.0 paid agent launch.** Code signing + notarization + auto-update endpoints live + minisign keys + docs (getting-started, troubleshooting, CONTRIBUTING, privacy commitment) + landing site + beta program (waitlist signups from v0.5 launch) + release notes automation + HN/Reddit/Product Hunt launch + v1.0.0 release.

After the dictation track lands (and again after M7), the project rewrites `architecture-design.md` as a **v2** consolidated source of truth reflecting the current stack (Tauri + audiopipe + llama-cpp-2 + agent core). The original v1 doc becomes archeology.

**Pricing model** (for v1.0 launch):

| Tier | Price | Includes |
|---|---|---|
| Free dictation | €0 | Local dictation + basic style learning forever |
| Cloud Sync | €4/mo | Free + E2E encrypted sync of dictation history, settings, style profiles across devices |
| Agent | €129 one-time | Free + agent features (continuous capture, Agent Console, advanced consolidator, multi-context profiles) |
| Agent + Cloud | €129 + €4/mo | Everything |

One-time pricing (not subscription) for agent respects "the app is yours when you buy it" OSS values. Cloud Sync is recurring because there's actual ongoing infrastructure cost. v2.0 paid upgrade (€49) at year 2-3 funds continued development.

**Privacy commitment** (verifiable via open source code): user content (audio, transcripts, screenshots, style profiles, indexed activity) NEVER leaves the device. Only license validation tokens and (opt-in) E2E encrypted cloud sync blobs transit the network. No telemetry. No analytics. No crash reports without explicit opt-in. AGPL-3.0-or-later license prevents commercial forks from closing this stance.

**Cross-platform discipline** (AGENTS.md): v1 ships macOS-only (binary signed/notarized). Code stays portable. v2 adds Linux + Windows via existing abstraction layers (`os-integration` traits, audiopipe multi-vendor GPU support, future Win32/X11/Wayland screen capture implementations). Active upstream contribution work in `rust-ml-contrib/` targets Metal perf parity with llama.cpp, cross-vendor GPU expansion for LLMs (Vulkan/DirectML), MLX-style optimization, and future NPU support.

Detailed plans and specs are tracked in the local `docs/` working directory (gitignored, including a strategic decision memo). The CHANGELOG entry for each milestone will be added under [Unreleased] when work begins.

## [0.5.0] - 2026-05-26 — M4: STT migration to audiopipe

### Added
- **`audiopipe` as the STT inference layer.** Three models available behind a unified `Model::from_pretrained()` API:
  - **Parakeet TDT 0.6B v3** (default, CC-BY-4.0) — 25 European languages incl. Italian; lowest-latency transducer.
  - **Qwen3-ASR 0.6B** (opt-in, Apache-2.0) — 30 languages + 22 Chinese dialects; adds JA/ZH/AR/HI/...
  - **Whisper large-v3-turbo** (fallback, MIT) — ~99 languages; broadest coverage.
- **Wizard model picker** (3-card radio) on a new wizard step; Parakeet pre-selected as recommended.
- **Wizard language step** filters its dropdown by the chosen model's language list. Auto-detect is always the default; switching the model after picking a language resets to auto-detect with an inline notice.
- **Settings → Models tab** lists all three STT models with Active / Installed / Not-downloaded status and a Use button for hot-swap.
- **`get_stt_catalog` Tauri command** — exposes the backend STT catalog to the frontend so the static TypeScript mirror can assert parity in debug builds.
- **`app/src-tauri/src/stt/` module** with `catalog.rs` (model metadata), `mod.rs` (loader + `SttModelHandle` enum), and `mock.rs` (`MockModel` gated on `test-stt` feature + `LIREVO_DEV_USE_MOCK_STT=1` env var) so unit tests + UI iteration don't need real ONNX weights.
- **`audiopipe` mirror at `https://github.com/fiorelorenzo/audiopipe`**, Cargo dep pinned to commit `f00281ce`. The fork is the integration anchor — streaming on the fork (Phase 3) is **deferred** beyond this release; the live overlay UI ships in a follow-up when fork-side streaming lands.
- **Multi-vendor GPU lanes via audiopipe Cargo features.** This DMG enables `metal`, `coreml`, `parakeet-mlx`, `qwen3-asr-ggml`, `whisper`. Future v2 Linux/Windows builds add `directml` and `vulkan-ggml` via target-specific deps.
- NOTICE updated with audiopipe + Parakeet + Qwen3-ASR + Whisper attributions.

### Changed
- **STT engine: `whisper-rs` → `audiopipe`.** The HTTP sidecar (dev-only, `inference-core` binary) and the shipped Tauri app both now load `audiopipe::Model`. The wire protocol of the sidecar's `/v1/stt` endpoint is unchanged (WAV in, text out) — `lirevo-cli stt` and `lirevo-prototype` keep working end-to-end.
- Settings schema bumped to **v3** (additive): adds `stt_model_id: Option<String>` keyed by the catalog id (e.g. `"parakeet-tdt-0.6b-v3"`).
- Sidecar env vars: `SIDECAR_WHISPER_MODEL_PATH` / `SIDECAR_WHISPER_COREML_DISABLE` replaced with `SIDECAR_STT_MODEL_NAME`. The `SIDECAR_STT_BACKEND=stub` test escape hatch is preserved.
- `ModelState` events: the `whisper: bool` field is renamed to `stt: bool` for semantic accuracy now that the engine is not whisper-specific. **Breaking** for any downstream consumer of the event payload (none in tree).
- `AppError::WhisperNotLoaded` renamed `SttNotLoaded`. Internal rename only.
- README "Whisper model provisioning" section replaced with "STT model provisioning (M4+: audiopipe)" — three-model table + headless sidecar instructions.

### Removed
- **`whisper-rs` dependency** (and `whisper-rs-sys`, `whisper.cpp` transitives).
- `crates/inference-core/src/whisper.rs`, `crates/inference-core/src/stub.rs`, and the `SttBackend` / `StubBackend` traits in `backend.rs`. The audiopipe surface is the abstraction; we don't re-introduce a project-side wrapper trait.
- CoreML encoder special-casing (audiopipe handles Apple Silicon acceleration internally via its `parakeet-mlx` engine).
- M3-era frontend `coreml_url` field on STT catalog entries.

### Available but not yet wired
- **Streaming `transcribe_stream` on the fork** (plan T14, landed 2026-05-26 as `feat/parakeet-streaming` on `fiorelorenzo/audiopipe`, rev `d63cf3a`, two commits): re-decode-growing-buffer architecture for the Parakeet ONNX engine, `PartialTranscript { text, delta, segments, is_final }` shape, cumulative `text` byte-identical to one-shot. Wired through to Lirevo's Cargo dep (pin bumped from `f00281ce` to `d63cf3a`). The dictation state machine still calls one-shot `transcribe()` — wiring live-overlay UX (plan T15) into the hotkey state machine + adding a chunk-peek API to `audio-capture` are tracked as a follow-up.

### Deferred (not in this release)
- **Live-overlay UI consuming `transcribe_stream`** (plan T15). Needs `audio-capture` to expose a chunk-peek API and the dictation state machine to feed audio incrementally during hold. Tracked as follow-up.
- **Upstream PR to screenpipe/audiopipe** (plan T16). Will be filed once the streaming impl has logged real-utterance smoke runs from Lirevo.

## [0.4.0] - 2026-05-18 — M3: Tauri app shell + model manager

### Added
- **Tauri 2 app shell** replacing the M0 Electron scaffold. Single-process Rust + WKWebView. Bundle size ~30 MB (vs ~80 MB Electron).
- `inference-core` folded as an in-process Rust library. No more sidecar process, UNIX socket, or napi wrapper.
- Settings persistence via `tauri-plugin-store` with Rust-side validation. Env-affecting changes trigger an in-process model reload with a "Reloading models..." toast.
- Setup wizard: 5 screens (Welcome, Accessibility, Microphone, Models, Hotkey) with pill stepper + horizontal slide transitions + alert-dialog skip confirm.
- Settings window: 4 tabs (General, Models, Hotkey, About) using shadcn-svelte primitives.
- Model manager: curated catalog (3 STT — Whisper large-v3-turbo / distil-large-v3 / small.en; 2 LLM — Qwen3-4B-Instruct-2507 / Llama-3.2-3B-Instruct) + file picker fallback + streaming downloads with cancel + CoreML encoder auto-extract for compatible Whisper models.
- Tray with 4 state-driven icons (loading / ready / recording-pulse / error) + live menu reflecting model and recording state.
- Custom titlebar with macOS Overlay traffic lights.
- Recording overlay: floating glassmorphic indicator with live audio waveform (24 bars, ~33 Hz updates from new `audio-capture` RMS emitter).
- Design system: Inter Variable + JetBrains Mono via @fontsource-variable; OKLCH design tokens; multi-layer shadows; generous radii (12-32px); motion durations + easings.
- 11 custom Svelte components: Titlebar, Logo, KeyChip, StepIndicator, PermissionStatus, SuccessCheck, SkeletonRow, EmptyState, ModelCard, FilePicker, RecordingIndicator.
- shadcn-svelte primitives: Button, Input, Label, Select, RadioGroup, Switch, Slider, Dialog, AlertDialog, Progress, Sonner, Separator, Tooltip.
- i18n via i18next with `en.json` baseline.
- Logging via `tracing` + `tracing-appender` with daily rotation (`~/Library/Logs/ai.lirevo.app/`).
- Auto-update plumbing (`tauri-plugin-updater`) installed and wired through Settings → About → "Check for updates". Endpoints empty until code signing in M0.5/pre-v1.
- `inference-core` convenience methods: `WhisperBackend::transcribe(&[u8], &str)` and `LlamaBackend::chat_sync(ChatRequest)`.
- `audio-capture` emits RMS audio levels via a `watch::Sender<f32>` during recording.
- `os-integration::clipboard::set_text` for last-resort clipboard fallback during inject failures.
- `just dev` / `just dmg` / `just test` / `just check` / `just fmt` / `just lint` / `just clean`.

### Changed
- CI workflow rewritten for the Tauri toolchain (no more Forge / napi build steps).
- `app/` directory replaced with a fresh Tauri scaffold (`app/src/` SvelteKit frontend, `app/src-tauri/` Rust backend).

### Removed
- `crates/os-bindings-napi/` — no longer consumed by anything.
- Old Electron M0 `app/src/main.ts` and `app/src/sidecar.ts` (the entire Electron scaffold).

### Stale
- `docs/specs/2026-05-17-m3-app-shell-design.md` (Electron-based, superseded by `2026-05-17-m3-tauri-app-shell-design.md`).
- `docs/plans/2026-05-17-m3-app-shell-plan.md` (gitignored).

### Post-signing tasks (deferred to M0.5/pre-v1)
- Acquire Apple Developer ID; sign + notarize + staple the .app.
- Generate minisign keypair for updater signatures (`tauri signer generate`).
- Populate `tauri.conf.json` updater section: `pubkey`, `endpoints`.
- Host latest.json + signed updates on GitHub Releases.

### Known limitations
- Tray icons are programmatically-generated placeholders; polished icons land in M8 beta.
- Wizard "Test mic" button is a 2-second UI placebo (real live mic level test deferred — real TCC prompt happens at first dictation).
- Model download resume after cancel is not implemented.
- English-only UI; other locales deferred (i18next scaffold ready).
- Uses SvelteKit with adapter-static + in-page hash routing per window. Each Tauri WebviewWindow loads a different SvelteKit route (`/`, `/wizard`, `/settings`, `/model-manager`).

## [0.3.0] - <set release date at merge> - M2 prototype dictation

### Added
- New crate `audio-capture`: cpal-based microphone capture with stereo→mono mixdown + rubato resample to 16 kHz mono f32. Public `Recorder::start/stop` API + `samples_to_wav` helper.
- New crate `os-integration` (macOS-only): `HotkeyListener` (CGEventTap push-to-talk, default Right Option), `Injector` (AXUIElement primary + NSPasteboard fallback), permission helpers (`check_accessibility`, `prompt_accessibility`, `check_microphone`).
- New crate `os-bindings-napi`: napi-rs Node addon wrapping Recorder/HotkeyListener/Injector for M3 Electron consumption. Produces `libos_bindings_napi.dylib`; loading in Electron is M3's job.
- New crate `lirevo-prompts`: shared system prompts extracted from lirevo-cli, now consumed by both `lirevo-cli clean` and `lirevo-prototype`.
- New binary `lirevo-prototype`: end-to-end push-to-talk dictation. Preflight checks accessibility + sidecar reachability + healthz both ready. Hotkey loop spawns parallel pipeline: record → POST /v1/stt → POST /v1/chat (with cleanup prompt) → AX/Pasteboard inject. Graceful degrade: if LLM fails, raw STT is injected.
- README sections: "Setting up accessibility permission", "Using lirevo-prototype", "Known limitations of text injection".
- Justfile recipes: `build-m2`, `prototype`.
- CI builds all M2 crates in release, verifies napi dylib presence.

### Changed
- `crates/lirevo-cli/src/clean_prompt.rs` removed; `lirevo-cli` now depends on `lirevo-prompts`. Behavior of `lirevo-cli clean` unchanged.
- CI build step renamed: "Build sidecar + CLI (release)" → "Build sidecar + CLI + M2 (release)".

### Notes
- M2 is macOS-only. The `os-integration` crate has `compile_error!` on non-macOS targets; v2 will add Linux + Windows backends.
- napi-rs dylib is built in CI but not loaded — M3 wires it into the Electron renderer.
- VAD / silence-detection auto-stop deferred to M4.
- Modifier+key combo hotkeys (e.g., Cmd+Shift+D) deferred to M3 settings UI.
- Per-app force-pasteboard overrides deferred to M3.
- Upstream API churn handled inline (cpal 0.16→0.17, rubato 0.16→2.x, objc2-* feature renames, napi 2.x ThreadsafeFunction signature change). Same pattern as whisper-rs / llama-cpp-2 in M1a/M1b. Deviations documented in commits.

## [0.2.0] - <set release date at merge> - M1b LLM cleanup

### Added
- `inference-core` exposes `POST /v1/chat` (lean custom shape, JSON+MsgPack): `{system?, user, history?, temperature?, max_tokens?, stop?}` → `{text, model, stopped_by, tokens}`.
- `LlamaBackend` on top of `llama-cpp-2` with the `metal` feature. Chat template read from the GGUF metadata (ChatML fallback). Provisioned via the `SIDECAR_LLM_MODEL_PATH` env var.
- `StubLlmBackend` selectable via `SIDECAR_LLM_BACKEND=stub` for CI/testing without a real model (extra envs `SIDECAR_LLM_STUB_SLEEP_MS` and `SIDECAR_LLM_STUB_CTX_SIZE` for concurrency and overflow tests).
- `SIDECAR_LLM_CTX_SIZE` env var (default 4096) controls model context size, exposed via `/v1/models`.
- Concurrent LLM requests serialized via `std::sync::Mutex::try_lock()` → immediate 503 `busy` (the `llama-cpp-2` library does not make `LlamaContext` `Send`, so `tokio::sync::Mutex` is unusable; fail-fast is the practical equivalent for the single-user use case and a documented deviation from the original plan, which called for a 30s wait).
- `LlmBackend` trait parallel to `SttBackend`; `AppState` now has two independent slots for STT and LLM.
- `lirevo-cli chat` (raw subcommand, params via flags `--user --system --temperature --max-tokens --stop --json`) and `lirevo-cli clean` (cleanup preset with a versioned system prompt in `crates/lirevo-cli/src/clean_prompt.rs` + stdin support).
- End-to-end pipeline `lirevo-cli stt audio.wav | lirevo-cli clean` working with stub backends in CI.
- README: sections "LLM model provisioning" and `lirevo-cli chat` / `clean` usage.

### Changed
- `/healthz` adds `llm_ready: bool`. **Breaking** (field consumers must accept the new flag, but the existing `stt_ready` check is unchanged).
- `/version` `backend` field changes from `"whisper-rs"` to `"inference-core"`. **Breaking** by design: the binary now hosts two backends, the process identity is more honest.
- `/v1/models` can list 0, 1, or 2 entries (LLM entry has an extra `ctx_size`; STT entry unchanged).
- Sidecar release binary grows from ~15 MB to an estimated ~25 MB (static whisper.cpp + llama.cpp).

### Notes
- No SSE streaming: deferred to M4.
- No `ctx_size` hot-reload via API: M3 (settings UI) handles this via sidecar respawn.
- No tool/function calling: out of scope for the dictation use case.
- No simultaneously-loaded multiple LLM models: M3 model-manager performs single-swap.
- Top-k / top-p / repetition penalty hardcoded to sensible defaults: additive surface to be added later if needed.
- `llama-cpp-2` pinned to `0.1.x` at M1b time; minor API deviations (notably: `LlamaContext` is `!Send`, requiring `unsafe impl Send + Sync` on `LlamaBackend` with Mutex serialization as the invariant).

## [0.1.0] - <set release date at merge> - M1a STT

### Added
- `inference-core` now exposes `POST /v1/stt` (WAV in body → transcribed text) and `GET /v1/models`.
- `WhisperBackend` on `whisper-rs` with the `metal + coreml` features. Auto-detects the CoreML encoder next to the `.bin`. Provisioned via the `SIDECAR_WHISPER_MODEL_PATH` env var.
- `StubBackend` selectable via `SIDECAR_STT_BACKEND=stub` for CI/testing without a real model.
- Internal audio pipeline: hound + rubato to resample any WAV (8–96 kHz, 1–2 channels) to 16 kHz mono f32.
- Content negotiation JSON ↔ MsgPack on all endpoints via the `Accept` header.
- Concurrent requests serialized via `tokio::sync::Mutex` with a 30s timeout → 503 `busy`.
- New crate `crates/lirevo-cli`: subcommands `health`, `version`, `models`, `stt`.
- `just test-real` for integration tests against a real model (marker `#[ignore]`).
- README: sections "Whisper model provisioning" and `lirevo-cli` usage.

### Changed
- `/healthz` includes `stt_ready: bool`. `/version` reports `backend: "whisper-rs"` (previously `"hello-world"`).
- Sidecar release binary grows from ~3 MB to ~15 MB (static whisper.cpp + CoreML bridge).
- Rust toolchain bumped 1.85 → 1.88 (required by `whisper-rs-sys 0.15`).

### Notes
- No streaming, no model download manager, no LLM cleanup: M4, M3, M1b respectively.
- Models and `.mlmodelc` are not bundled in the DMG: the user downloads them and points to them via env.
- The `accelerate` feature of `whisper-rs` was removed upstream in 0.16 (macOS arm64 acceleration is now handled internally by whisper.cpp).

## [0.0.1] - 2026-05-15 - M0 Foundation

### Added
- Monorepo with `Cargo workspace` (crate `inference-core`) and Electron Forge app (`app/`) using Vite + Svelte 5 + TypeScript strict.
- `inference-core` Rust sidecar with `/healthz` and `/version` endpoints over a UNIX socket (axum). Graceful SIGTERM / SIGINT shutdown with socket file cleanup.
- Electron main process spawns and supervises the sidecar, polls `/healthz`, and exposes its state to the renderer via a contextBridge.
- macOS menu bar tray with sidecar status indicator.
- Svelte renderer that displays the live sidecar status.
- `just` orchestrator with commands: `dev`, `build`, `dmg`, `test`, `lint`, `format`, `clean`, `setup`.
- GitHub Actions CI workflow that builds an unsigned arm64 DMG on every push and PR.
- Apache-2.0 LICENSE and NOTICE at the repo root (per-file headers intentionally omitted).
- ESLint + Prettier configs for TypeScript and Svelte. Clippy pedantic on Rust.

### Notes
- DMG is unsigned in M0. Signing and notarization land in M0.5 after Apple Developer enrollment.
- x86_64 (Intel Mac) build is deferred to M0.5 or M1 (driven by user demand).
- Rust toolchain pinned to 1.85 (bumped from the original 1.84 target because transitive deps require `edition2024`, stabilized in 1.85).
