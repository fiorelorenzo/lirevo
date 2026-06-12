# Auto-load an untracked `.env` (gitignored) for local-only vars such as
# APPLE_SIGNING_IDENTITY. No-op when the file is absent (CI, fresh clones).
set dotenv-load := true

# Debug builds use a distinct bundle id so dev and prod never share macOS
# system state — Caches, WebKit storage, Preferences, and TCC permissions are
# all keyed on the bundle id. (Data + log dirs are split separately, by app
# name, in the Rust path resolver.) Release (`dmg`) keeps the real identifier
# from tauri.conf.json.
dev_identifier := "ai.lirevo.app.dev"

default:
    @just --list

# Tauri dev (HMR + Rust auto-rebuild). Frontend on :1420.
# Bare binary — macOS TCC prompts (mic, accessibility) cannot appear here.
# Use LIREVO_DEV_SKIP_PERMS=1 to mock permission-granted state when iterating
# UI, or `just dev-bundle` to test real TCC flows.
dev:
    cd app && pnpm install --frozen-lockfile
    cd app && pnpm exec tauri dev --config '{"identifier":"{{dev_identifier}}"}'

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
    ( cd app && pnpm install --frozen-lockfile && env -u APPLE_SIGNING_IDENTITY pnpm exec tauri build --debug --config '{"identifier":"{{dev_identifier}}"}' --target aarch64-apple-darwin --bundles app )
    # Relocate the two inference engines' dylibs + ggml backend modules into the
    # .app (preserving the dual-ggml `lirevo_pk_` disambiguation), rewrite the
    # binary rpath to @loader_path/../Frameworks, and re-sign. Uses
    # APPLE_SIGNING_IDENTITY if set (stable TCC), else ad-hoc.
    scripts/bundle-macos-install.sh "$app" debug aarch64-apple-darwin
    if [ -z "${APPLE_SIGNING_IDENTITY:-}" ]; then
        tccutil reset Accessibility {{dev_identifier}} || true
        tccutil reset Microphone {{dev_identifier}} || true
    fi
    open "$app"

# Release build → self-contained .app + .dmg under
# app/src-tauri/target/aarch64-apple-darwin/release/bundle/
#
# Tauri packages the .dmg from the .app it builds in the SAME pass, so we can't
# inject the engine-relocation between them via the CLI. Instead: build the .app
# only, relocate+re-sign the inference engines into it (preserving the dual-ggml
# `lirevo_pk_` disambiguation, rpath -> @loader_path/../Frameworks), then roll the
# .dmg from the fixed-up .app ourselves. Uses APPLE_SIGNING_IDENTITY if set.
#
# Notarization (gated on Apple creds):
#   The order MUST be build (sign only) -> bundle+re-sign -> notarize+staple .app
#   -> roll .dmg from the stapled .app -> notarize+staple the .dmg. Notarizing
#   before the bundle/re-sign would be invalidated by it; rolling the .dmg before
#   stapling the .app would ship an un-stapled app inside.
#
#   We must NOT let `tauri build` auto-notarize: it would notarize the
#   PRE-bundling .app, which bundle-macos-install.sh then invalidates. Tauri
#   auto-notarizes when APPLE_SIGNING_IDENTITY is set AND a full notarization
#   cred set is also in the env, so we run the build with the notarization vars
#   scoped OUT (`env -u ...`) — keeping APPLE_SIGNING_IDENTITY so it still signs.
#   All notarization happens in our explicit scripts/notarize-macos.sh step.
#
#   scripts/notarize-macos.sh exits 0 with a warning when no APPLE_* creds are
#   present, so this recipe (and CI's build-mac, which runs it credential-less)
#   still produces an un-notarized build. See the script header for the env vars.
dmg:
    #!/usr/bin/env bash
    set -euo pipefail
    bundle="app/src-tauri/target/aarch64-apple-darwin/release/bundle"
    app="$bundle/macos/Lirevo.app"
    # Sign-only build: strip the notarization creds from tauri's env so it does
    # not auto-notarize the pre-bundling .app (APPLE_SIGNING_IDENTITY is kept).
    ( cd app && env -u APPLE_ID -u APPLE_PASSWORD -u APPLE_TEAM_ID \
        -u APPLE_API_KEY -u APPLE_API_KEY_ID -u APPLE_API_ISSUER -u APPLE_API_KEY_PATH \
        pnpm install --frozen-lockfile && \
      env -u APPLE_ID -u APPLE_PASSWORD -u APPLE_TEAM_ID \
        -u APPLE_API_KEY -u APPLE_API_KEY_ID -u APPLE_API_ISSUER -u APPLE_API_KEY_PATH \
        pnpm exec tauri build --target aarch64-apple-darwin --bundles app )
    scripts/bundle-macos-install.sh "$app" release aarch64-apple-darwin
    # Notarize + staple the bundled, re-signed .app BEFORE rolling the .dmg, so
    # the app inside the .dmg carries its stapled ticket. No-op (exit 0) without
    # Apple creds.
    scripts/notarize-macos.sh app "$app"
    mkdir -p "$bundle/dmg"
    dmg="$bundle/dmg/Lirevo_0.6.0_aarch64.dmg"
    rm -f "$dmg"
    staging="$(mktemp -d)"
    cp -R "$app" "$staging/"
    ln -s /Applications "$staging/Applications"
    hdiutil create -volname "Lirevo" -srcfolder "$staging" -ov -format UDZO "$dmg"
    rm -rf "$staging"
    # Notarize + staple the .dmg itself so the download passes Gatekeeper before
    # it is mounted. No-op (exit 0) without Apple creds.
    scripts/notarize-macos.sh dmg "$dmg"
    echo "dmg: $dmg"

# Run all tests (Rust nextest + frontend vitest).
test:
    cargo nextest run --workspace
    cd app && pnpm test -- --run

# Type check across the workspace + frontend.
check:
    cargo check --workspace --all-targets
    cd app && pnpm exec svelte-kit sync
    cd app && pnpm exec svelte-check --threshold error
    cd app/src-tauri && cargo check --all-targets

# Format Rust + frontend (prettier).
fmt:
    cargo fmt --all
    cd app && pnpm exec prettier --write 'src/**/*.{ts,svelte,css,json}' 2>/dev/null || true

# Lint Rust + frontend (eslint if configured).
lint:
    cargo clippy --workspace --all-targets -- -D warnings
    cd app && pnpm exec eslint 'src/**/*.{ts,svelte}' 2>/dev/null || true

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
