default:
    @just --list

# Tauri dev (HMR + Rust auto-rebuild). Frontend on :1420.
# Bare binary — macOS TCC prompts (mic, accessibility) cannot appear here.
# Use LDA_DEV_SKIP_PERMS=1 to mock permission-granted state when iterating
# UI, or `just dev-bundle` to test real TCC flows.
dev:
    cd app && npm install --no-audit --no-fund
    cd app && npx tauri dev

# Debug .app bundle for testing macOS-permission flows (microphone,
# accessibility). The bare `just dev` binary cannot trigger TCC prompts;
# this builds a proper bundle so macOS recognizes the app and shows the
# permission dialog. Slower than `just dev` (real cargo build + bundling)
# but the only way to exercise the real permission UX.
#
# Output: app/src-tauri/target/aarch64-apple-darwin/debug/bundle/macos/local-dictation-app.app
#
# We `tccutil reset` Accessibility + Microphone before relaunch because
# the bundle is ad-hoc signed (Tauri's default) — the code-signing
# identity hash changes every rebuild, so the previous TCC grants point
# at a stale binary even though the System Settings toggle still reads
# "on". Without the reset the user sees "permission denied" plus a
# stale entry in Privacy & Security and has to clean it up by hand.
# A stable identity needs a Developer ID cert (M0.5).
dev-bundle:
    cd app && npm install --no-audit --no-fund
    cd app && npx tauri build --debug --target aarch64-apple-darwin --bundles app
    -tccutil reset Accessibility app.localdictation
    -tccutil reset Microphone app.localdictation
    open app/src-tauri/target/aarch64-apple-darwin/debug/bundle/macos/local-dictation-app.app

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

# Dev tool: run the refiner-stage model bake-off
eval BACKENDS OUT="$(date +%Y-%m-%d)-bake-off":
    cargo run -p lda-eval --release -- run \
      --corpus crates/lda-eval/data/corpus/v1-seed.jsonl \
      --profiles crates/lda-eval/data/profiles/v1.toml \
      --backends "{{BACKENDS}}" \
      --out crates/lda-eval/data/reports/{{OUT}}.md
