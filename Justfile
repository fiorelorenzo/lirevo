# Auto-load an untracked `.env` (gitignored) for local-only vars such as
# APPLE_SIGNING_IDENTITY. No-op when the file is absent (CI, fresh clones).
set dotenv-load := true

default:
    @just --list

# Tauri dev (HMR + Rust auto-rebuild). Frontend on :1420.
# Bare binary — macOS TCC prompts (mic, accessibility) cannot appear here.
# Use LIREVO_DEV_SKIP_PERMS=1 to mock permission-granted state when iterating
# UI, or `just dev-bundle` to test real TCC flows.
dev:
    cd app && npm install --no-audit --no-fund
    cd app && npx tauri dev

# Debug .app bundle for testing macOS-permission flows (microphone,
# accessibility). The bare `just dev` binary cannot trigger TCC prompts;
# this builds a proper bundle so macOS recognizes the app and shows the
# permission dialog.
#
# Output: app/src-tauri/target/aarch64-apple-darwin/debug/bundle/macos/Lirevo.app
#
# Stable signing for persistent permissions: set APPLE_SIGNING_IDENTITY (in an
# untracked `.env`, auto-loaded above) to a "Developer ID Application" cert —
# `security find-identity -v -p codesigning` lists yours. A stable identity
# keeps the code-signing hash constant across rebuilds, so macOS TCC grants
# PERSIST: grant mic + Accessibility once and they stick on every rebuild.
#
# We build ad-hoc, then re-sign with the identity but WITHOUT hardened runtime:
# Tauri's identity-sign turns on hardened runtime, which blocks the bundled
# inference libs (ggml/Metal, whisper, llama, audiopipe/MLX) from loading and
# the app fails to launch ("Launchd job spawn failed"). Re-signing without it
# keeps the identity stable (TCC persists) while letting the app run. Without an
# identity (ad-hoc), the hash changes each build so we wipe the stale grants.
dev-bundle:
    #!/usr/bin/env bash
    set -euo pipefail
    app="app/src-tauri/target/aarch64-apple-darwin/debug/bundle/macos/Lirevo.app"
    ( cd app && npm install --no-audit --no-fund && env -u APPLE_SIGNING_IDENTITY npx tauri build --debug --target aarch64-apple-darwin --bundles app )
    if [ -n "${APPLE_SIGNING_IDENTITY:-}" ]; then
        codesign --force --deep -s "$APPLE_SIGNING_IDENTITY" "$app"
    else
        tccutil reset Accessibility ai.lirevo.app || true
        tccutil reset Microphone ai.lirevo.app || true
    fi
    open "$app"

# Release build → .app + .dmg under app/src-tauri/target/aarch64-apple-darwin/release/bundle/
dmg:
    cd app && npm install --no-audit --no-fund
    cd app && npx tauri build --target aarch64-apple-darwin

# Run all tests (Rust nextest + frontend vitest).
test:
    cargo nextest run --workspace
    cd app && npm test -- --run

# Type check across the workspace + frontend.
check:
    cargo check --workspace --all-targets
    cd app && npx svelte-kit sync
    cd app && npx svelte-check --threshold error
    cd app/src-tauri && cargo check --all-targets

# Format Rust + frontend (prettier).
fmt:
    cargo fmt --all
    cd app && npx prettier --write 'src/**/*.{ts,svelte,css,json}' 2>/dev/null || true

# Lint Rust + frontend (eslint if configured).
lint:
    cargo clippy --workspace --all-targets -- -D warnings
    cd app && npx eslint 'src/**/*.{ts,svelte}' 2>/dev/null || true

# Wipe caches.
clean:
    cargo clean
    rm -rf app/node_modules app/src-tauri/target app/.svelte-kit app/build

# Reset runtime state so the next `just dev-bundle` starts fresh: TCC
# grants cleared, settings.json wiped, logs removed. Downloaded model
# files are KEPT — they're multi-GB and rarely the thing you want to
# re-pull. Use `just reset-all` to nuke those too. Refuses to run while
# the app is alive.
reset:
    scripts/reset.sh

# Same as `reset`, but also deletes the downloaded model files. Prompts
# for confirmation before wiping so a fat-finger doesn't cost you a
# multi-gigabyte re-download.
reset-all:
    scripts/reset.sh --models

# Dev tool: run the refiner-stage model bake-off
eval BACKENDS OUT="$(date +%Y-%m-%d)-bake-off":
    cargo run -p lirevo-eval --release -- run \
      --corpus crates/lirevo-eval/data/corpus/v1-seed.jsonl \
      --profiles crates/lirevo-eval/data/profiles/v1.toml \
      --backends "{{BACKENDS}}" \
      --out crates/lirevo-eval/data/reports/{{OUT}}.md
