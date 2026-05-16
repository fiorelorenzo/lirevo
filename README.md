# local-dictation-app

[![License: Apache 2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)
[![CI](https://github.com/fiorelorenzo/local-dictation-app/actions/workflows/build-mac.yml/badge.svg)](https://github.com/fiorelorenzo/local-dictation-app/actions/workflows/build-mac.yml)

Fully local, open-source dictation app for macOS (Linux + Windows in v2).
Inspired by FreeFlow, Wispr Flow, Superwhisper. Zero cloud, zero account, zero telemetry.

**Status:** M0 Foundation complete - app builds and runs, but no inference yet. M1 adds Whisper + LLM cleanup.

## Getting started (development)

Requirements:
- macOS on Apple Silicon
- Rust 1.85 (managed automatically via `rust-toolchain.toml`)
- Node 22 (`.nvmrc`)
- `just` (`brew install just`)
- `cargo-nextest` and `cargo-watch` (`brew install cargo-nextest cargo-watch`)

First-time setup:

```bash
just setup
```

Run in dev mode (hot reload on both Rust sidecar and Electron renderer):

```bash
just dev
```

Build a local DMG:

```bash
just dmg
# DMG appears under app/out/make/
```

Other commands: `just test`, `just lint`, `just format`, `just clean`. Run `just` with no args for the full list.

## Documentation

Design documents (architecture spec, milestone specs, implementation plans) are kept as local-only working docs under `docs/`. The public repository tracks only code, configuration, README, CHANGELOG, LICENSE, NOTICE.

## Installing the unsigned M0 DMG

M0 ships unsigned (Apple Developer enrollment is M0.5). To open the app for the first time:

1. Drag `local-dictation-app.app` to `/Applications`.
2. Right-click the app and choose "Open" (only the first time).
3. Or remove the quarantine attribute: `xattr -d com.apple.quarantine /Applications/local-dictation-app.app`.

## Provisioning del modello Whisper (M1a)

M1a esegue speech-to-text via [whisper.cpp](https://github.com/ggerganov/whisper.cpp) (bridge `whisper-rs`, build con features `metal + coreml`). Il modello non è bundlato nel DMG: lo fornisci tu.

1. Scarica un modello ggml dalla [repo HF di whisper.cpp](https://huggingface.co/ggerganov/whisper.cpp/tree/main). Esempio consigliato: `ggml-large-v3-turbo.bin` (~1.5 GB, ottimo trade-off qualità/velocità su M-series).
2. (Opzionale, raccomandato su M-series) Scarica l'encoder CoreML corrispondente, es. `ggml-large-v3-turbo-encoder.mlmodelc.zip`, ed estrailo **nella stessa cartella** del `.bin`. Il sidecar lo rileva automaticamente cercando `<basename>-encoder.mlmodelc/` accanto al `.bin`.
3. Esporta la variabile d'ambiente:
   ```bash
   export SIDECAR_WHISPER_MODEL_PATH=/percorso/assoluto/ggml-large-v3-turbo.bin
   ```
4. Avvia in dev: `just dev`, oppure lancia il sidecar standalone:
   ```bash
   SIDECAR_SOCKET_PATH=/tmp/s.sock cargo run -p inference-core
   ```

Se il modello manca o il path non esiste, il sidecar gira comunque ma `/v1/stt` ritorna `503 stt_unavailable` e `/healthz` riporta `stt_ready: false`.

Per disabilitare l'encoder CoreML (utile su alcuni M1 con bug noti su ANE):
```bash
export SIDECAR_WHISPER_COREML_DISABLE=1
```

## Usare `lda-cli`

Il CLI vive nel crate `crates/lda-cli` e parla col sidecar via UNIX socket. Risoluzione del socket: flag `--socket` > env `SIDECAR_SOCKET_PATH` > default macOS `$HOME/Library/Application Support/app/sidecar.sock` (stesso path della Electron app, così puoi parlare col sidecar mentre la app gira).

Esempi:

```bash
# Salute del sidecar
lda-cli health
# status=ok  version=0.0.1  uptime_ms=12345  stt_ready=true

# Modelli caricati
lda-cli models

# Trascrivi un file WAV
lda-cli stt sample.wav
# stderr: [whisper-rs] ggml-large-v3-turbo (it) 30000ms audio, 4120ms processing (rtf 0.14x)
# stdout: ciao mondo, questo è un test.

# Stesso comando, JSON intero in stdout
lda-cli stt sample.wav --json

# Con segments e una lingua forzata
lda-cli stt sample.wav --language en --segments --json

# Forza MsgPack sulla risposta (debug)
lda-cli --msgpack stt sample.wav
```

Exit codes: `0` success, `2` server unreachable, `3` HTTP 4xx, `4` HTTP 5xx, `5` bad input file.

## Provisioning del modello LLM (M1b)

M1b esegue il cleanup del testo trascritto via [llama.cpp](https://github.com/ggerganov/llama.cpp) (bridge `llama-cpp-2`, build con feature `metal`). Il modello non è bundlato nel DMG: lo fornisci tu.

1. Scarica un modello GGUF instruct dalla [community HuggingFace](https://huggingface.co/lmstudio-community). Esempi consigliati su M-series 16 GB+:
   - `Llama-3.2-3B-Instruct-Q4_K_M.gguf` (~2 GB, default raccomandato)
   - `Qwen2.5-3B-Instruct-Q4_K_M.gguf` (~2 GB, ottimo italiano)
   - `Phi-3.5-mini-instruct-Q4_K_M.gguf` (~2.2 GB)
2. Esporta env vars:
   ```bash
   export SIDECAR_LLM_MODEL_PATH=/percorso/assoluto/Llama-3.2-3B-Instruct-Q4_K_M.gguf
   # Optional: tweak context size (default 4096)
   export SIDECAR_LLM_CTX_SIZE=4096
   ```
3. Avvia il sidecar: `just dev` o `cargo run -p inference-core`.

Se il modello manca o il path non esiste, il sidecar continua a girare ma `/v1/chat` ritorna `503 llm_unavailable` e `/healthz` riporta `llm_ready: false`.

## Usare `lda-cli chat` e `clean`

`lda-cli chat` espone direttamente l'API `/v1/chat`:

```bash
# Singola domanda
lda-cli chat --user "Capitale d'Italia?"
# Roma.

# Con system prompt + temperature
lda-cli chat --user "..." --system "Sei un assistente conciso." --temperature 0.2 --max-tokens 50

# Output JSON completo (utile per scripting)
lda-cli chat --user "..." --json

# Stop sequences (ripetibile)
lda-cli chat --user "..." --stop "END" --stop "</fine>"
```

`lda-cli clean` è il preset di post-processing per dictation. Legge input da arg o stdin e applica un system prompt versionato che richiede solo punctuation/capitalization/paragraphing senza alterare il contenuto:

```bash
# Da argomento
lda-cli clean "and so my fellow americans ask not what your country can do"

# Da stdin (pipe-friendly)
lda-cli stt audio.wav | lda-cli clean

# Con language hint
lda-cli stt audio.wav | lda-cli clean --language it
```

La pipeline completa **STT → cleanup** è quindi un one-liner:

```bash
lda-cli stt ~/sample.wav | lda-cli clean
```

Exit codes invariati (vedi sezione `lda-cli stt`).

## Setting up accessibility permission (M2)

`lda-prototype` needs macOS Accessibility permission to:
- Listen for the push-to-talk hotkey via CGEventTap.
- Inject the cleaned text into the focused application via AXUIElement.

First run will print a message and exit with code 2. To grant:

1. Open **System Settings → Privacy & Security → Accessibility**.
2. Click the `+` button.
3. Add `target/release/lda-prototype` (or `target/debug/lda-prototype` during dev).
4. Toggle the switch to ON.
5. Re-run `lda-prototype`.

Microphone permission is auto-prompted by cpal on the first recording — no manual setup needed.

## Using `lda-prototype` (M2)

Push-to-talk dictation that types into the focused app, end-to-end via the sidecar.

Prerequisites:
- Sidecar running with both models loaded (whisper + llama). See M1a/M1b README sections.
- Accessibility granted (see previous section).

Build and run:

```bash
just build-m2
./target/release/lda-prototype
```

Flags:

```
--hotkey <KEY>         right-option (default) | left-option | right-command | fn | f5
--socket <PATH>        override sidecar UNIX socket
--language <ISO>       cleanup language hint, default "auto"
--force-pasteboard     skip AX inject, always use pasteboard
--paste-delay-ms <N>   pasteboard paste→restore delay (default 120ms)
```

The hotkey is also configurable via env var `SIDECAR_HOTKEY` (same values).

Typical use:

1. Run `lda-prototype` in a terminal.
2. Click into the field where you want text (Notes, Safari URL bar, VS Code editor, etc.).
3. **Hold Right Option** while speaking.
4. **Release** Right Option. Within ~2-3s, cleaned text is typed into the focused field.
5. Repeat. `Ctrl+C` to quit.

### Known limitations of text injection

- **AXUIElement path** works in: Safari, Notes, TextEdit, VS Code, most native Cocoa apps.
- **Pasteboard fallback** is used automatically when AX fails. Known apps where pasteboard is the path: Apple Terminal, some Electron apps with non-standard text input.
- During pasteboard fallback the clipboard is temporarily overwritten and then restored. Non-string clipboard content (images, files) is lost during restore — known limitation, settings UI in M3 will offer to disable pasteboard fallback.
- If `--paste-delay-ms` is too low (default 120), a slow target app may receive the restore before the paste — symptoms: dictation seems to not type anything. Bump to 200-300 if needed.

## Project name

The folder name `local-dictation-app` is a placeholder. The product name is intentionally not chosen yet (see "Open decisions" in the architecture design).

## License

[Apache-2.0](LICENSE). Copyright 2026 Lorenzo Fiore. See [NOTICE](NOTICE) for third-party attributions.
