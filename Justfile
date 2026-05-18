default:
    @just --list

# Tauri dev (HMR + Rust auto-rebuild). Frontend on :1420.
dev:
    cd app && npm install --no-audit --no-fund
    cd app && npx tauri dev

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
