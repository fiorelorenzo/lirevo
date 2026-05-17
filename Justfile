# Justfile - orchestrator for local-dictation-app
# Run `just` (no args) for the command list.

default:
    @just --list

# ---- dev ----

# Tauri dev (filled in T37)
dev:
    @echo "Not yet wired — see T37"

# Watch the Rust sidecar and re-run on change.
sidecar-dev:
    cd crates/inference-core && cargo watch -x run

# Run electron in dev mode.
app-dev:
    cd app && npm start

# ---- build ----

# Release build of sidecar + electron production bundle (.app, not DMG).
build:
    cargo build --release --target aarch64-apple-darwin -p inference-core
    mkdir -p app/resources
    cp target/aarch64-apple-darwin/release/inference-core app/resources/inference-core
    cd app && npm run package

# Release build of M2 prototype binary + napi addon.
build-m2:
    cargo build --release --target aarch64-apple-darwin -p lda-prototype -p os-bindings-napi

# Run the dictation prototype in dev mode (requires sidecar already running).
prototype:
    #!/usr/bin/env bash
    set -euo pipefail
    if [ -z "${SIDECAR_SOCKET_PATH:-}" ]; then
      echo "warning: SIDECAR_SOCKET_PATH not set; using default ~/Library/Application Support/app/sidecar.sock" >&2
    fi
    ./target/debug/lda-prototype

# ---- quality gates ----

# Run unit and integration tests across all crates and the app.
test:
    cargo nextest run -p inference-core
    cargo nextest run -p lda-cli
    cargo nextest run -p lda-prompts
    cargo nextest run -p audio-capture
    cargo nextest run -p os-integration
    cd app && npm test

# Run the ignored "real model" tests.
# Requires:
#   SIDECAR_WHISPER_MODEL_PATH=/path/to/ggml-*.bin (for STT test)
#   SIDECAR_LLM_MODEL_PATH=/path/to/*.gguf (for LLM test)
# Tests that lack their required env are reported but skipped via panic-on-missing-env.
test-real:
    cargo test -p inference-core -- --ignored --nocapture

# Lint everything (clippy + eslint).
lint:
    cargo clippy --workspace --all-targets -- -D warnings
    cd app && npm run lint

# Format everything.
format:
    cargo fmt
    cd app && npm run format

# Remove build artifacts.
clean:
    cargo clean
    cd app && rm -rf out node_modules/.vite

# One-time setup for a fresh clone.
setup:
    cd app && npm install
    cargo fetch
