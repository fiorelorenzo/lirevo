# Testing

Lirevo's checks run through `just` recipes, which are the same contract CI uses.

## Test suites

- **Rust** — unit and integration tests across the workspace crates, run with
  `cargo nextest`. Coverage spans audio capture and resampling, the inference
  core (catalog, wire, llama), the eval harness (scoring, probes, corpus),
  resource-monitor, and os-integration, plus a `lirevo-cli` smoke test.
- **Frontend** — Vitest suites under `app/src/lib/__tests__/` (settings, hotkey,
  i18n, backend) and `app/src/lib/components/home/__tests__/` (HistoryList).

## Running locally

| Goal | Command |
| --- | --- |
| All tests (Rust nextest + Vitest) | `just test` |
| Type check (Rust + svelte-check) | `just check` |
| Lint gate (rustfmt + clippy + prettier + eslint) | `just lint` |
| Auto-format | `just fmt` |

- `just test` runs `cargo nextest run --workspace`, then `pnpm test` in `app/`.
- `just check` runs `cargo check --all-targets`, `svelte-kit sync`,
  `svelte-check --threshold error`, and a `cargo check` of the Tauri host.
- `just lint` is the strict gate: `cargo fmt --check` and
  `cargo clippy -- -D warnings` on both the root workspace and the Tauri host,
  plus `prettier --check` and `eslint` on the frontend.
- `just fmt` applies `cargo fmt` and `prettier --write`.

## CI

The `ci` workflow runs on every push to `main` and every pull request:

- **check-macos** (macos-15) — the primary gate: `just check`, then `just lint`,
  then `just test`.
- **check-linux / check-windows** — compile-only guards (`cargo check` of the
  shipped app) that catch cross-platform regressions. A green cross-platform
  check means the code **compiles** on that target, not that Lirevo runs there
  (see the platform-support note in the README).

The signed, notarized `.dmg` is built separately by `release.yml` on a `v*` tag.
