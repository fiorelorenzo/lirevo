# Changelog

All notable changes to this project are documented in this file. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project adheres
to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Roadmap (next milestones)

The inference stack is being rewritten end-to-end on Rust-native, multi-vendor foundations. Two engine swaps land back-to-back (M4 for STT, M5 for LLM), followed by polish, feature expansion, and v1.0.

- **M4 — STT migration to audiopipe.** Replaces `whisper-rs` with `audiopipe::Model` (Rust, MIT, screenpipe team) consumed directly (no project-side wrapper). Audiopipe ships **Parakeet TDT 0.6B v3** (CC-BY-4.0, 25 European languages incl. Italian, lowest-latency transducer), **Qwen3-ASR 0.6B** (Apache-2.0, 30 languages + 22 Chinese dialects), and **Whisper** variants behind a unified `Model::from_pretrained()` API, with **multi-vendor GPU acceleration**: Apple Metal/MLX/CoreML, NVIDIA CUDA, AMD/Intel via DirectML on Windows and Vulkan-GGML on Linux. The setup wizard adds a 3-option model picker with the language step coordinated to the chosen model. We mirror audiopipe in our org, pin Cargo to our fork's commit, and **implement a streaming API on the fork** so the live-partial-transcript overlay UX ships with M4 regardless of upstream review timing. The upstream PR to `screenpipe/audiopipe` is filed in parallel as good-citizenship work — not a shipping gate. The 2-week timebox is on the **technical implementation** of streaming on top of Parakeet ONNX; if that engineering can't be made to work, M4 ships with one-shot `transcribe()` only and live partials are deferred. No pre-launch WER benchmark gate — since there are no production users yet, the wizard's 3-model choice is the de facto gate; a manual smoke check on Italian (~10 phrases per model) lives in M4 manual QA as a sanity catch.
- **M5 — LLM runtime migration to mistral.rs + Gemma 4 default.** Replaces `llama-cpp-2` with `mistralrs::*` consumed directly (no wrapper). **The model catalog also refreshes**: Qwen3-4B-Instruct-2507 and Llama-3.2-3B-Instruct (M3 lineup) are dropped as too big; new lineup is **Gemma 4 E2B-it + assistant draft pair** (Apache-2.0, ~1.55GB Q4, multimodal image+audio+video, 140+ languages incl. Italian, 128K context, speculative decoding for 3x decode speedup) as default, and **Qwen3-VL-2B-Instruct** (Apache-2.0, ~1.3GB Q4, 256K native context, 32 OCR languages, 102M downloads = very mature) as the stable alternative. Catalog entries include `benchmark_score` from `lda-eval`. Audit + cross-model perf bench on Task 1 as kill switch. **Conditional Task 1.5**: Gemma 4 has 3 known open issues in mistral.rs (#2098 GGUF panic, #2058 inference hang on complex prompts, #2051 NaN logits) — if they affect our path, we fix them upstream via `~/Progetti/Personale/rust-ml-contrib/` and submit PRs to `EricLBuehler/mistral.rs` (budget 2-3 weeks). If unrescuable, Qwen3-VL-2B becomes default and Gemma 4 stays opt-in "experimental." This pattern — production milestone driving upstream contribution work — is how M5+ extracts value from the contributor relationship the maintainer is building at `rust-ml-contrib/` (targeting Metal perf parity, cross-vendor GPU expansion for LLMs, MLX-style optimization). `lda-eval` extension to multimodal task scoring is deferred to post-M5 (likely M6 alongside Style Learning, which needs to measure vision-context profile quality).
- **M6 — Polish & Reliability + Mac optimizations.** VAD silence-detection auto-stop, custom hotkey combos (Cmd+Shift+D etc.) wired through to the M3 settings UI, per-app force-pasteboard overrides, real wizard "Test mic" with live audio level, model download resume on cancel/network failure, polished tray icons; plus the original Mac optim sweep: memory pressure handler, thermal state monitor, QoS user-interactive, Low Power Mode awareness, diagnostic panel.
- **M7 — Streaming everywhere.** Extend `transcribe_stream` on our audiopipe fork to Qwen3-ASR and Whisper (Parakeet already done in M4). Live overlay UX uniform across all three models.
- **M8 — Edit Mode.** Voice-driven transform of selected text (`focused_selection` detection, modifier-extension hotkey, dedicated system prompt, `replace_focused_selection`). First "agentic" feature built on the new mistral.rs foundation.
- **M9 — Beta + polish + v1.0.** Code signing + notarization + auto-update endpoints live + minisign keys + docs (getting-started, troubleshooting, CONTRIBUTING) + beta program (20-50 testers) + release notes automation + v1.0.0 release.

After M5, the project also rewrites `architecture-design.md` as a **v2** consolidated source of truth reflecting the post-pivot stack (Tauri + audiopipe + mistral.rs). The original v1 doc becomes archeology.

MLX is no longer a standalone milestone — it ships as an audiopipe feature flag (`parakeet-mlx`) enabled by default on macOS builds. The old "M4 Streaming pipeline" and "M7 MLX integration (Python sidecar)" milestones are dropped: streaming is now part of M4 (conditional on the fork-side technical implementation); MLX is part of M4 (Cargo feature).

Scope decisions for v1: the user picks an STT model in the wizard. Default user reaches Parakeet's 25 European languages; users who need Japanese/Chinese/Arabic/Hindi/Persian/etc. pick Qwen3-ASR; users who need the long tail of 99 languages pick Whisper. The earlier "drop non-EU languages from v1" decision is reversed by Qwen3-ASR availability through audiopipe.

Cross-platform note (AGENTS.md compliance): adopting audiopipe makes AMD GPU support a first-class target for STT — Windows AMD users get DirectML, Linux AMD users get Vulkan via the GGML model variants. The known gap is Parakeet ONNX on Linux AMD (CPU-only); the wizard will route those users to Qwen3-ASR GGML. For LLM cross-vendor GPU (Vulkan/DirectML), both mistral.rs and candle currently lack it — the upstream contribution roadmap in `rust-ml-contrib` aims to close this gap.

Detailed plans and specs are tracked in the local `docs/` working directory (gitignored). The CHANGELOG entry for each milestone will be added under [Unreleased] when work begins.

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
- Logging via `tracing` + `tracing-appender` with daily rotation (`~/Library/Logs/local-dictation-app/`).
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
- New crate `lda-prompts`: shared system prompts extracted from lda-cli, now consumed by both `lda-cli clean` and `lda-prototype`.
- New binary `lda-prototype`: end-to-end push-to-talk dictation. Preflight checks accessibility + sidecar reachability + healthz both ready. Hotkey loop spawns parallel pipeline: record → POST /v1/stt → POST /v1/chat (with cleanup prompt) → AX/Pasteboard inject. Graceful degrade: if LLM fails, raw STT is injected.
- README sections: "Setting up accessibility permission", "Using lda-prototype", "Known limitations of text injection".
- Justfile recipes: `build-m2`, `prototype`.
- CI builds all M2 crates in release, verifies napi dylib presence.

### Changed
- `crates/lda-cli/src/clean_prompt.rs` removed; `lda-cli` now depends on `lda-prompts`. Behavior of `lda-cli clean` unchanged.
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
- `inference-core` espone `POST /v1/chat` (shape custom lean, JSON+MsgPack): `{system?, user, history?, temperature?, max_tokens?, stop?}` → `{text, model, stopped_by, tokens}`.
- `LlamaBackend` su `llama-cpp-2` con feature `metal`. Chat template letto dal metadata GGUF (fallback ChatML). Provisioning via env `SIDECAR_LLM_MODEL_PATH`.
- `StubLlmBackend` selezionabile via `SIDECAR_LLM_BACKEND=stub` per CI/testing senza modello reale (env aggiuntivi `SIDECAR_LLM_STUB_SLEEP_MS` e `SIDECAR_LLM_STUB_CTX_SIZE` per i test di concorrenza e overflow).
- `SIDECAR_LLM_CTX_SIZE` env var (default 4096) controlla context size del modello, esposta via `/v1/models`.
- Serializzazione delle richieste LLM concorrenti via `std::sync::Mutex::try_lock()` → 503 `busy` immediato (la libreria `llama-cpp-2` non rende `LlamaContext` `Send`, quindi `tokio::sync::Mutex` non è utilizzabile; il fail-fast è una scelta equivalente in pratica per il use case single-user e deviazione documentata dal piano originale che parlava di 30s wait).
- Trait `LlmBackend` parallelo a `SttBackend`; `AppState` ora ha due slot indipendenti per STT e LLM.
- `lda-cli chat` (subcommand raw, params via flags `--user --system --temperature --max-tokens --stop --json`) e `lda-cli clean` (preset cleanup con system prompt versionato in `crates/lda-cli/src/clean_prompt.rs` + stdin support).
- Pipeline end-to-end `lda-cli stt audio.wav | lda-cli clean` funzionante con stub backends in CI.
- README: sezioni "Provisioning del modello LLM" e "Usare `lda-cli chat` e `clean`".

### Changed
- `/healthz` aggiunge `llm_ready: bool`. **Breaking** (consumer del campo devono accettare il nuovo flag, ma il check esistente su `stt_ready` non cambia).
- `/version` `backend` field passa da `"whisper-rs"` a `"inference-core"`. **Breaking** documentato: il binary ora ospita due backend, l'identità di processo è più onesta.
- `/v1/models` può listare 0, 1 o 2 entries (entry LLM ha `ctx_size` aggiuntivo, entry STT invariata).
- Sidecar binary release passa da ~15 MB a ~25 MB stimati (whisper.cpp + llama.cpp statici).

### Notes
- Niente streaming SSE: rimandato a M4.
- Niente hot-reload di `ctx_size` via API: M3 (settings UI) gestirà via respawn del sidecar.
- Niente tool/function calling: fuori scope per il dictation use case.
- Niente multipli modelli LLM caricati simultaneamente: M3 model-manager farà swap singolo.
- Top-k/top-p/repetition penalty hardcoded a default sensati: aggiunta additive in futuro se servirà.
- `llama-cpp-2` pinned to `0.1.x` at M1b time; API deviazioni minor (notabile: `LlamaContext` !Send richiede `unsafe impl Send + Sync` su `LlamaBackend` con Mutex-serialization come invariante).

## [0.1.0] - <set release date at merge> - M1a STT

### Added
- `inference-core` ora espone `POST /v1/stt` (WAV in body → testo trascritto) e `GET /v1/models`.
- `WhisperBackend` su `whisper-rs` con features `metal + coreml`. Auto-detect dell'encoder CoreML accanto al `.bin`. Provisioning via env `SIDECAR_WHISPER_MODEL_PATH`.
- `StubBackend` selezionabile via `SIDECAR_STT_BACKEND=stub` per CI/testing senza modello reale.
- Pipeline audio interna: hound + rubato per resamplare ogni WAV (8-96 kHz, 1-2 canali) a 16 kHz mono f32.
- Content negotiation JSON ↔ MsgPack su tutti gli endpoint via header `Accept`.
- Serializzazione delle richieste concorrenti via `tokio::sync::Mutex` con timeout 30s → 503 `busy`.
- Crate nuovo `crates/lda-cli`: subcommands `health`, `version`, `models`, `stt`.
- `just test-real` per i test integration con modello reale (marker `#[ignore]`).
- README: sezioni "Provisioning del modello Whisper" e "Usare lda-cli".

### Changed
- `/healthz` include `stt_ready: bool`. `/version` riporta `backend: "whisper-rs"` (era `"hello-world"`).
- Sidecar binary release passa da ~3 MB a ~15 MB (whisper.cpp statico + bridge CoreML).
- Rust toolchain bumped 1.85 → 1.88 (richiesto da `whisper-rs-sys 0.15`).

### Notes
- Niente streaming, niente download manager modelli, niente LLM cleanup: rispettivamente M4, M3, M1b.
- Modelli e `.mlmodelc` non sono bundlati nel DMG: l'utente li scarica e li punta via env.
- La feature `accelerate` di whisper-rs è stata rimossa upstream nella 0.16 (l'accelerazione macOS arm64 è ora gestita internamente da whisper.cpp).

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
