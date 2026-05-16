# Changelog

All notable changes to this project are documented in this file. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project adheres
to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
