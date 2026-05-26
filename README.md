# Lirevo

[![License: Apache 2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)
[![CI](https://github.com/fiorelorenzo/lirevo/actions/workflows/build-mac.yml/badge.svg)](https://github.com/fiorelorenzo/lirevo/actions/workflows/build-mac.yml)

Fully local, open-source AI scribe and agent for macOS (Linux + Windows in v2).
Inspired by FreeFlow, Wispr Flow, Superwhisper — but learns your writing style and grows into a personal agent. Zero cloud, zero account, zero telemetry.

**Pronunciation:** Lirevo — *lee-REH-voh*.

**Status:** **M4 shipped** — Tauri app with full setup wizard, push-to-talk dictation, and STT now powered by `audiopipe` (Parakeet TDT v3 / Qwen3-ASR / Whisper, multi-vendor GPU). Next: M5 swaps the LLM cleanup runtime to `mistral.rs` + Gemma 4. M6–M10 build the agent core, then ship v0.5 (free dictation) and v1.0 (paid agent). See [CHANGELOG](CHANGELOG.md) for the full roadmap.

## Installing

Releases ship as an arm64 `.dmg`. Until M10 the app is **unsigned** (Apple Developer enrollment is part of M10), so on the first launch:

1. Drag `Lirevo.app` to `/Applications`.
2. Right-click the app and choose **Open** (only the first time).
3. Or remove the quarantine attribute manually:
   ```bash
   xattr -d com.apple.quarantine /Applications/Lirevo.app
   ```

## Using the app

After launching the app for the first time, the **setup wizard** guides you through eight steps:

1. **Welcome.**
2. **Accessibility** — grant via the System Settings deep link (needed for the global hotkey and text injection).
3. **Microphone** — confirm the mic test envelope is non-zero.
4. **STT model** — pick one of three speech-to-text engines: Parakeet TDT v3 (default, 25 European languages, lowest latency), Qwen3-ASR (30 languages with broad Asian, Arabic, and European coverage), or Whisper large-v3-turbo (99-language fallback). Weights download from Hugging Face into `~/.cache/huggingface/hub/`.
5. **Language** — pick "Auto-detect" (default) or force a specific language. The dropdown is filtered by the model chosen in the previous step.
6. **Cleanup model (optional)** — pick a small LLM to add punctuation and tidy sentence flow, or skip to inject raw STT output. Can be added or changed later from Settings.
7. **Background mode** — decide how present the app should be (launch at login, start hidden, stay in the menu bar).
8. **Hotkey** — pick a key (default: Right Option).

Once the wizard is done, hold the hotkey anywhere on the system and speak. Release to transcribe → clean → inject into the focused app.

The menu bar icon shows model state (loading / ready / recording / error). **Settings**, **Model Manager**, and **Re-run Wizard** are all accessible from the tray menu.

### Text injection: known limitations

- **AXUIElement path** works in Safari, Notes, TextEdit, VS Code, and most native Cocoa apps.
- **Pasteboard fallback** is used automatically when AX fails. It currently kicks in for Apple Terminal and some Electron apps with non-standard text input.
- During pasteboard fallback the clipboard is temporarily overwritten and then restored. **Non-string clipboard content (images, files) is lost during restore** — known limitation; a settings toggle to disable pasteboard fallback is planned for M8.
- If the paste delay is too low for a slow target app, the restore may land before the paste (symptom: nothing types). Bump `--paste-delay-ms` to 200–300 if you see this in dev.

## Architecture (one paragraph)

Single Tauri 2 process. The frontend is Svelte 5 + Tailwind v4 + shadcn-svelte running in WKWebView. The backend is Rust, calling `audiopipe::Model` (Parakeet / Qwen3-ASR / Whisper) for STT and `llama-cpp-2` for LLM cleanup — M5 will swap the latter for `mistral.rs`. Hotkey events flow from a CGEventTap thread (in `os-integration`) through an mpsc channel into a tokio task that owns the dictation state machine. Settings persist via `tauri-plugin-store`. Auto-update plumbing is wired but inactive until code signing lands in M10.

Cross-platform discipline (see [AGENTS.md](AGENTS.md)): macOS-only today, but platform-specific code is gated behind abstractions in `os-integration` / `audio-capture` so the v2 Linux + Windows ports are a matter of adding sibling implementations, not rewriting consumers.

## Development

### Requirements

- macOS on Apple Silicon
- Rust 1.85 (managed automatically via `rust-toolchain.toml`)
- Node 22 (`.nvmrc`)
- `just` (`brew install just`)
- `cargo-nextest` and `cargo-watch` (`brew install cargo-nextest cargo-watch`)

### First-time setup

```bash
just setup
```

Or manually:

```bash
cd app && npm install
```

### Common commands

| Goal                                        | Command                          |
| ------------------------------------------- | -------------------------------- |
| Dev (HMR, no real TCC prompts)              | `just dev`                       |
| Dev with mocked permissions                 | `LIREVO_DEV_SKIP_PERMS=1 just dev` |
| Dev with real TCC prompts (debug `.app`)    | `just dev-bundle`                |
| Release `.app` + `.dmg`                     | `just dmg`                       |
| All tests (Rust nextest + Vitest)           | `just test`                      |
| Type check (Rust + Svelte)                  | `just check`                     |
| Format                                      | `just fmt`                       |
| Lint (clippy `-D warnings` + eslint)        | `just lint`                      |
| Wipe build caches                           | `just clean`                     |

Run `just` with no args for the full list.

### macOS permission workflows (mic / Accessibility)

macOS TCC binds permissions to a binary's code-signing identity hash, not its bundle ID. Three consequences:

- The bare `just dev` binary **cannot** trigger TCC prompts — macOS auto-denies the request silently. Use one of the workarounds below.
- A permission granted to the release `.app` (`just dmg`) does **not** transfer to `just dev` / `just dev-bundle` outputs, even though they share `ai.lirevo.app` as bundle ID.
- Every fresh debug bundle is a fresh TCC entity. If macOS misbehaves after rebuilds, reset:
  ```bash
  tccutil reset Microphone     ai.lirevo.app
  tccutil reset Accessibility  ai.lirevo.app
  ```
  This clears the cached grant/deny + the entry in System Settings → Privacy. The next launch starts from scratch and macOS shows the prompt again.

Pick the right workflow:

| Goal                                                  | Command                                       |
| ----------------------------------------------------- | --------------------------------------------- |
| Iterate on wizard UI without real audio / TCC         | `LIREVO_DEV_SKIP_PERMS=1 just dev` — short-circuits `check_*` / `prompt_*` to Granted; `test_mic` returns a synthetic envelope. Debug builds only. |
| Test real TCC prompt + real audio capture             | `just dev-bundle` — builds a debug `.app` and opens it. |
| Final smoke test before release                       | `just dmg` — release `.app` + `.dmg`.         |

### Dev-only crates

These crates are **never** bundled in the shipped `.app`:

- **`lirevo-prototype`** (`crates/lirevo-prototype`) — headless end-to-end dictation pipeline. Useful for testing the STT → cleanup → inject chain without launching the Tauri UI. Run with `cargo run -p lirevo-prototype`. Needs an `inference-core` HTTP sidecar running.
- **`lirevo-cli`** (`crates/lirevo-cli`) — thin client over the `inference-core` HTTP sidecar (`/v1/stt`, `/v1/chat`, `/healthz`). See [Dev tools: lirevo-cli](#dev-tools-lirevo-cli) below.
- **`lirevo-eval`** (`crates/lirevo-eval`) — refiner-model bake-off harness. Benchmarks LLM candidates against a multilingual corpus on chrF, semantic cosine, deterministic assertions, latency, and (optionally) LLM-as-judge fidelity scores. See `crates/lirevo-eval/README.md`.

### Dev tools: `lirevo-cli`

`lirevo-cli` lives in `crates/lirevo-cli` and talks to the `inference-core` HTTP sidecar over a UNIX socket. Socket resolution order: `--socket` flag > `SIDECAR_SOCKET_PATH` env > default `$HOME/Library/Application Support/ai.lirevo.app/sidecar.sock`.

Examples:

```bash
# Sidecar health
lirevo-cli health
# status=ok  version=0.0.1  uptime_ms=12345  stt_ready=true  llm_ready=true

# Loaded models
lirevo-cli models

# Transcribe a WAV
lirevo-cli stt sample.wav
# stderr: [whisper-rs] ggml-large-v3-turbo (en) 30000ms audio, 4120ms processing (rtf 0.14x)
# stdout: hello world, this is a test.

# Full JSON response on stdout
lirevo-cli stt sample.wav --json

# Force a language + return per-segment timings
lirevo-cli stt sample.wav --language en --segments --json

# MsgPack response (debug)
lirevo-cli --msgpack stt sample.wav

# Raw chat call
lirevo-cli chat --user "Capital of Italy?"
# Rome.

# Chat with a system prompt
lirevo-cli chat --user "..." --system "Be concise." --temperature 0.2 --max-tokens 50

# Dictation cleanup preset (versioned system prompt — only punctuation /
# capitalization / paragraphing, never alters meaning)
lirevo-cli clean "and so my fellow americans ask not what your country can do"

# Pipe-friendly: stdin → clean
lirevo-cli stt audio.wav | lirevo-cli clean

# Language hint
lirevo-cli stt audio.wav | lirevo-cli clean --language en

# End-to-end one-liner
lirevo-cli stt ~/sample.wav | lirevo-cli clean
```

Exit codes: `0` success, `2` server unreachable, `3` HTTP 4xx, `4` HTTP 5xx, `5` bad input file.

### STT model provisioning (M4+: audiopipe)

The shipped app's wizard handles STT model downloads automatically. Three models are offered:

| Model | Size | Languages | License |
| --- | --- | --- | --- |
| **Parakeet TDT 0.6B v3** (default, recommended) | ~600 MB | 25 European languages | CC-BY-4.0 |
| **Qwen3-ASR 0.6B** (broad languages) | ~700 MB | 30 languages, broad Asian / Arabic / European coverage | Apache-2.0 |
| **Whisper large-v3-turbo** (fallback) | ~1.5 GB | ~99 languages | MIT |

Weights are downloaded to the Hugging Face cache (`~/.cache/huggingface/hub/`) on first use of each model — `audiopipe` handles this transparently. There are no `.bin` / `.gguf` paths to provide manually and no CoreML encoder to download separately (audiopipe's Parakeet MLX engine handles Apple Silicon acceleration internally).

For headless / sidecar workflows (`lirevo-prototype`, `lirevo-cli` against a standalone `inference-core`), point the sidecar at a model by name:

```bash
# Select model (defaults to parakeet-tdt-0.6b-v3 on Apple Silicon, mlx variant)
export SIDECAR_STT_MODEL_NAME=parakeet-tdt-0.6b-v3
# Or, for testing without weights:
export SIDECAR_STT_BACKEND=stub
SIDECAR_SOCKET_PATH=/tmp/s.sock cargo run -p inference-core
```

Multi-vendor GPU acceleration is configured via Cargo features on the `audiopipe` dep (`metal`, `coreml`, and on v2 builds `directml` / `vulkan-ggml`). The shipped macOS DMG ships with Metal + CoreML + MLX enabled.

### LLM model provisioning (headless / sidecar use)

Recommended GGUF instruct models on 16 GB+ M-series:

- `Llama-3.2-3B-Instruct-Q4_K_M.gguf` (~2 GB, current recommended default)
- `Qwen2.5-3B-Instruct-Q4_K_M.gguf` (~2 GB, strong on Italian)
- `Phi-3.5-mini-instruct-Q4_K_M.gguf` (~2.2 GB)

```bash
export SIDECAR_LLM_MODEL_PATH=/absolute/path/to/Llama-3.2-3B-Instruct-Q4_K_M.gguf
# Optional: tweak context size (default 4096)
export SIDECAR_LLM_CTX_SIZE=4096
```

Start the sidecar with `cargo run -p inference-core` or `just dev`. If the model is missing, `/v1/chat` returns `503 llm_unavailable` and `/healthz` reports `llm_ready: false`.

The env-var-based LLM path will be **superseded by M5** when `mistral.rs` takes over and the model catalog moves into the app catalog with `benchmark_score` from `lirevo-eval`.

## Documentation

Design documents (architecture spec, milestone specs, implementation plans) are kept as local-only working docs under `docs/`. The public repository tracks only code, configuration, README, CHANGELOG, LICENSE, NOTICE.

## About the name

**Lirevo** is a coined name in the Vercel/Stripe/Anthropic tradition — pronounceable but with no pre-existing semantic baggage in any language, so it can carry the brand entirely on its own meaning. Pronounced *lee-REH-voh*.

The folder name `local-dictation-app/` is a legacy placeholder from before the brand was chosen. It will be renamed when convenient; meanwhile every internal reference uses `lirevo`.

## License

[Apache-2.0](LICENSE). Copyright 2026 Lorenzo Fiore. See [NOTICE](NOTICE) for third-party attributions.
