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

# Bump the app version and stub its CHANGELOG entry. Run this, fill in the
# stub, open the PR; tagging `v<version>` on main after the merge is what
# actually publishes (release.yml).
#
# The version has ONE source of truth: `[package] version` in
# app/src-tauri/Cargo.toml. Settings > About reads it via CARGO_PKG_VERSION,
# and the .app/.dmg get it from tauri's Cargo.toml fallback (tauri.conf.json
# intentionally has no `version` key). This recipe keeps the two derived
# copies — Cargo.lock and app/package.json — in step; scripts/check-versions.sh
# fails the lint gate if they ever drift.
release VERSION:
    #!/usr/bin/env bash
    set -euo pipefail
    version="{{VERSION}}"
    version="${version#v}"
    if ! [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
        echo "not a version number: '{{VERSION}}' (expected e.g. 0.9.1)" >&2
        exit 1
    fi
    tmp="$(mktemp)"
    # Scoped to the [package] table so no dependency's version is touched.
    awk -v v="$version" '
        /^\[/ { in_package = ($0 == "[package]") }
        in_package && /^version[[:space:]]*=/ && !done { sub(/"[^"]+"/, "\"" v "\""); done = 1 }
        { print }
    ' app/src-tauri/Cargo.toml > "$tmp" && mv "$tmp" app/src-tauri/Cargo.toml
    # Re-resolve so Cargo.lock carries the new member version. --offline: the
    # dependency graph is unchanged, only our own version moved.
    ( cd app/src-tauri && cargo update --offline --workspace >/dev/null )
    awk -v v="$version" '
        !done && /^  "version":/ { sub(/: *"[^"]*"/, ": \"" v "\""); done = 1 }
        { print }
    ' app/package.json > "$tmp" && mv "$tmp" app/package.json
    if ! grep -q "^## \[$version\]" CHANGELOG.md; then
        awk -v v="$version" -v today="$(date +%F)" '
            !done && /^## \[/ {
                print "## [" v "] - " today " — TODO: one-line release summary"
                print ""
                print "### Fixed"
                print "- TODO"
                print ""
                done = 1
            }
            { print }
        ' CHANGELOG.md > "$tmp" && mv "$tmp" CHANGELOG.md
        echo "CHANGELOG.md: added a stub for $version — fill it in, it becomes the GitHub Release body"
    fi
    rm -f "$tmp"
    scripts/check-versions.sh

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
    # Derive the version from the built .app (matches tauri.conf.json) instead
    # of hardcoding it — otherwise the DMG filename drifts from the real app
    # version on every release bump.
    version="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$app/Contents/Info.plist")"
    dmg="$bundle/dmg/Lirevo_${version}_aarch64.dmg"
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

# Format Rust (both workspaces) + frontend (prettier).
fmt:
    cargo fmt --all
    cd app/src-tauri && cargo fmt --all
    cd app && pnpm exec prettier --write 'src/**/*.{ts,svelte,css,json}'

# Lint gate (same checks CI runs): rustfmt + clippy on both workspaces,
# prettier + eslint on the frontend.
lint:
    scripts/check-versions.sh
    cargo fmt --all --check
    cd app/src-tauri && cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cd app/src-tauri && cargo clippy --all-targets -- -D warnings
    cd app && pnpm exec prettier --check 'src/**/*.{ts,svelte,css,json}'
    cd app && pnpm exec eslint 'src/**/*.{ts,svelte}'

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
