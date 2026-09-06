# AGENTS.md

Orientation for AI coding agents and human contributors working in this repo.
This file is the source of truth; tool-specific files (e.g. `CLAUDE.md`) point
here.

## What this project is

**Lirevo** — fully local, open-source AI scribe and personal agent for macOS
(Apple Silicon, with Linux and Windows planned for v2). Push-to-talk dictation
→ STT → LLM style-aware cleanup → text injection into the focused app, plus
the foundation for a personal agent that learns your writing style and acts
on your behalf. No cloud, no account, no telemetry.

The folder name `local-dictation-app/` is a legacy placeholder from before the
brand name was chosen. Internal references use `lirevo` / `ai.lirevo.app`.

See `README.md` for end-user setup and `CHANGELOG.md` for milestone status.

## Tech stack

- **Frontend:** Svelte 5 + SvelteKit + Tailwind v4 + shadcn-svelte, running in
  WKWebView via Tauri 2.
- **Backend:** Rust (workspace, edition 2021, toolchain pinned in
  `rust-toolchain.toml`). The Tauri 2 host process loads both inference engines
  directly as libraries, in-process — no separate sidecar process in the shipped
  app. STT is `parakeet-cpp` (our own open-source Rust binding to
  `mudler/parakeet.cpp`, ggml; git-pinned), single model
  `parakeet-tdt-0.6b-v3` GGUF q4_k; the LLM cleanup stage is `llama-cpp-2`
  (GGUF). Both engines use **dynamic ggml backends** (`GGML_BACKEND_DL`): the
  app auto-selects the best compute backend at runtime (Metal on macOS, CPU
  fallback) and surfaces it in **Settings → About**. DL is enabled on
  macOS + Linux; Windows is static-linked (DL deferred there).
- **OS integration:** `cocoa` / `core-foundation` / `core-graphics` for
  CGEventTap (global hotkey); `objc2-app-kit` (`NSPasteboard`) + synthetic
  Cmd+V for text injection.
- **Build tooling:** `just`, `cargo`, `pnpm`, `cargo-nextest`. Node 22 (see
  `.nvmrc`; CI pins the same), pnpm 9.15.9 (pinned via `packageManager`; enable
  with `corepack enable`), Rust 1.88 (see `rust-toolchain.toml`).

## Repository layout

```
.
├── app/                      # Tauri app (frontend + Tauri host)
│   ├── src/                  # Svelte UI (routes, components, stores, i18n)
│   └── src-tauri/            # Tauri host crate (Rust) — the shipped app
├── crates/                   # Rust workspace (8 crates)
│   ├── audio-capture/        # cpal mic capture, device resolution,
│   │                         #   resampling to 16 kHz mono, Smart Mic routing
│   ├── inference-core/       # llama-cpp-2 (LLM) wrapper + cleanup model catalog.
│   │                         #   Its HTTP/axum sidecar layer is dev-only; the
│   │                         #   library surface is used in-process. (STT lives
│   │                         #   in the Tauri host's stt/ module over parakeet-cpp.)
│   ├── lirevo-cli/           # Dev-only CLI talking to inference-core's sidecar
│   ├── lirevo-eval/          # Dev-only cleanup-stage eval harness (corpus,
│   │                         #   scoring, judge reports)
│   ├── lirevo-prompts/       # Versioned LLM cleanup prompts
│   ├── lirevo-prototype/     # Dev-only headless dictation pipeline
│   ├── os-integration/       # macOS hotkey, text injection, TCC checks,
│   │                         #   clipboard + audio cue (stubs on other targets)
│   └── resource-monitor/     # Battery / thermal / memory / CPU signal
│                             #   broadcaster (real sensors on macOS, stub elsewhere)
├── scripts/                  # Build/utility scripts (icons, reset, etc.)
├── Justfile                  # Canonical task entry points
├── Cargo.toml                # Workspace manifest
└── README.md
```

`lirevo-prototype`, `lirevo-cli`, `lirevo-eval`, and `inference-core`'s
HTTP/axum sidecar layer are **dev-only**: they are not bundled in the shipped
DMG. Production code paths run inside the Tauri host (`app/src-tauri/`).

### Tauri host modules (`app/src-tauri/src/`)

This is the only crate shipped in the DMG. STT (`parakeet-cpp`) and LLM
(`llama-cpp-2`) run in-process here — there is no child process, Unix socket,
or HTTP endpoint in the shipped app.

```
app/src-tauri/src/
├── commands/                 # Tauri IPC handlers invokable from the webview:
│                             #   dictation, inference, history, models,
│                             #   permissions, profile, settings, windows,
│                             #   dialog, updater
├── db/                       # Local SQLite history (rusqlite + migrations):
│                             #   history.rs queries, migrations/ append-only SQL
├── engine/                   # Resource-aware Engine lifecycle: lazy load/unload
│                             #   of LLM + STT, auto-recover, energy-profile
│                             #   integration (decision.rs, slot.rs, streak.rs)
├── stt/                      # Host STT layer over parakeet-cpp (catalog.rs,
│                             #   types.rs, mock.rs); GGUF download into models/
├── error.rs                  # AppError enum serialized over IPC
├── hotkey.rs                 # Push-to-talk coordinator: bridges os-integration's
│                             #   CFRunLoop HotkeyListener into tokio, drives the
│                             #   Recorder, runs the STT → cleanup → inject pipeline
├── logging.rs                # tracing-subscriber init (rolling daily log file)
├── models.rs                 # Frontend-facing catalog wire type + load helpers
├── paths.rs                  # Data + log dir resolution ("Lirevo" / "Lirevo (Dev)")
├── settings.rs               # Persisted Settings with versioned migration
├── state.rs                  # Shared AppState (Recorder, Injector, Settings,
│                             #   ModelState watch channel, Db, Engine)
└── tray.rs                   # Menu-bar tray: template icons, loading animation,
                              #   permission attention badge, state transitions
```

## Cross-platform discipline

The app targets **macOS first**, then **Linux and Windows in v2**. Today only
macOS works end-to-end, but the codebase must stay portable. Concretely:

- **Always introduce a platform-neutral abstraction first; put OS-specific
  code behind it.** Never call platform APIs directly from `app/src-tauri/`
  or from the frontend.
  - Hotkey, text injection, accessibility checks → behind `os-integration`.
  - Audio capture / device enumeration → behind `audio-capture`.
  - File-system paths, app-data dirs, autostart → use Tauri's cross-platform
    APIs (`tauri::api::path`, `tauri-plugin-autostart`, etc.), not raw
    `~/Library/...` paths.
- **macOS-specific implementations live in `#[cfg(target_os = "macos")]`
  modules** inside the abstraction crate (e.g. `os-integration/src/macos/`).
  When adding a new capability, define the trait / public function first and
  add a `unimplemented!()` or `Err(NotSupportedOnThisPlatform)` stub for
  `#[cfg(not(target_os = "macos"))]` so the workspace still compiles on
  other targets.
- **Frontend code is platform-agnostic.** UI components must not assume
  macOS-only concepts (TCC, Accessibility, AppleScript, ⌘ key). Where the
  UX is genuinely OS-shaped (e.g. permission wizard copy, modifier-key
  labels), branch on a value exposed by the Rust side (`platform.os`,
  `platform.modifier_label`), not on `navigator.platform` or hard-coded
  strings.
- **Dependencies:** avoid macOS-only crates (`cocoa`, `core-foundation`,
  `objc2`, `accessibility-sys`, etc.) anywhere outside `os-integration`,
  `audio-capture`, and other abstraction crates. If a new dep is
  macOS-only, gate it with `[target.'cfg(target_os = "macos")'.dependencies]`
  in `Cargo.toml`.
- **CI is macOS-only today**, which makes it easy to land code that breaks
  on Linux/Windows. Before merging anything that touches `os-integration`,
  `audio-capture`, or path/permission handling, run
  `cargo check --workspace --target x86_64-unknown-linux-gnu` locally if you
  can — and call out the gap in the PR description if you can't.

If the only way to implement something is platform-specific, that's fine —
just make the abstraction explicit so the v2 Linux/Windows port is a matter
of adding a sibling implementation, not rewriting consumers.

## Common commands

Use `just` recipes — they are the contract that CI runs.

| Goal                                    | Command                       |
| --------------------------------------- | ----------------------------- |
| First-time setup                        | `cd app && pnpm install`      |
| Dev (HMR, no real TCC prompts)          | `just dev`                    |
| Dev with mocked permissions             | `LIREVO_DEV_SKIP_PERMS=1 just dev` |
| Dev with real TCC prompts (debug `.app`)| `just dev-bundle`             |
| Bump the version + stub the CHANGELOG    | `just release <version>`      |
| Release `.app` + `.dmg`                 | `just dmg`                    |
| All tests (Rust nextest + Vitest)       | `just test`                   |
| Type check (Rust + Svelte)              | `just check`                  |
| Format                                  | `just fmt`                    |
| Lint (clippy `-D warnings` + eslint)    | `just lint`                   |
| Wipe build caches                       | `just clean`                  |
| Reset runtime state (keeps models)      | `just reset`                  |
| Reset runtime state + delete models     | `just reset-all`              |

Both `just dev` and `just dev-bundle` inject the distinct **debug bundle id**
`ai.lirevo.app.dev` via `--config '{"identifier":"ai.lirevo.app.dev"}'`. The
production identifier `ai.lirevo.app` lives in `tauri.conf.json` and is used
only by `just dmg`.

`just dev-bundle` re-signs the bundle for **stable TCC grants**: set
`APPLE_SIGNING_IDENTITY` (a "Developer ID Application" cert) in an untracked
`.env` (auto-loaded by the Justfile). It re-signs with that identity but
**without** hardened runtime — Tauri's identity-sign would enable hardened
runtime, which blocks the bundled inference libs (ggml/Metal dynamic backends,
llama, parakeet-cpp) from loading. A stable identity keeps the code-signing hash constant so TCC
grants persist across rebuilds. Without an identity it falls back to ad-hoc and
resets the stale grants on each build.

`just reset` / `just reset-all` delegate to `scripts/reset.sh`: they clear TCC
grants, wipe `settings.json`, and remove logs. `reset` keeps downloaded models;
`reset-all` deletes them too (with a confirmation prompt). Both refuse to run
while the app is alive. `just eval` is a dev-only refiner-stage model bake-off.

### Local verification: run the minimal covering subset

CI now has three tiers (2026-09-06). `check-macos` (`just lint`/`just check`/
`just test`) and `check-windows` (a Windows compile guard) moved to `push:
main` only — nobody waits on them on a PR. The PR path runs one job,
`check-linux`, a scoped `cargo check` compile guard for the shipped app, and
skips it entirely for a diff that can't touch `app/src-tauri` or the crates
it depends on. `preflight` (`.github/preflight.json`) is the local
replacement for the coverage that moved off the PR path — see "CI and the
release pipeline" below. Locally you only need enough signal to catch an
obviously broken PR, so **scope commands to the diff, not the whole
suite**. Scope by *amount* (narrow to the crate/package/files you touched),
never by *category* — don't run lint but skip typecheck, or clippy on the
root workspace but skip `app/src-tauri`'s own fmt/clippy pass (it's a
separate Cargo workspace, see Common commands above). Run the full suite
(`just check && just lint && just test`) before release-critical changes
(release pipeline, signing/notarization scripts, migrations).
`preflight`'s `local-build-test` check runs `just check && just test` (no
lint - see "CI and the release pipeline" below) before every push.

- **Rust tests, scoped:** `cargo nextest run -p <crate> <filter>`, e.g.
  `cargo nextest run -p os-integration hotkey::`. For the Tauri host:
  `cd app/src-tauri && cargo nextest run <filter>`.
- **Frontend tests, scoped:** `cd app && pnpm exec vitest run <path>`, e.g.
  `pnpm exec vitest run src/lib/__tests__/settings.test.ts`.
- **Rust lint, scoped:** `cargo clippy -p <crate> --all-targets -- -D
  warnings` (root workspace) or `cd app/src-tauri && cargo clippy
  --all-targets -- -D warnings`. `cargo fmt --all --check` is whole-workspace
  by nature but is fast — no need to scope it.
- **Frontend lint, scoped:** `cd app && pnpm exec eslint <files>` and
  `pnpm exec prettier --check <files>` directly — the `lint` recipe hardcodes
  the full `src/**/*.{ts,svelte}` glob, so pass specific paths to the tools
  yourself rather than editing the recipe.
- **Type check:** `svelte-check` (`pnpm exec svelte-check --threshold
  error`) and `cargo check` are whole-project/whole-crate by nature — there's
  no useful narrower form, just run them as-is.

### Notarization (release `.dmg`)

`just dmg` notarizes the release `.app` and `.dmg` via
`scripts/notarize-macos.sh` — but **only** when Apple credentials are in the
env. Without them it prints a warning and exits 0, so a credential-less `just dmg`
still produces an **un-notarized** build: it runs on the build machine but is
Gatekeeper-rejected ("developer cannot be verified") on any other Mac. To
notarize locally, set ONE of these credential sets in the untracked `.env`:

- **App Store Connect API key (preferred):** `APPLE_API_KEY` (path to the `.p8`
  key), `APPLE_API_KEY_ID` (10-char Key ID), `APPLE_API_ISSUER` (issuer UUID).
- **Apple ID:** `APPLE_ID` (email), `APPLE_PASSWORD` (app-specific password, not
  the account password), `APPLE_TEAM_ID`.

These are on top of `APPLE_SIGNING_IDENTITY`, which is still required to sign.

Ordering is load-bearing: `tauri build` (sign only) → `bundle-macos-install.sh`
(relocate dylibs + re-sign) → notarize + staple the `.app` → roll the `.dmg`
from the stapled `.app` → notarize + staple the `.dmg`. Notarizing before the
re-sign would be invalidated by it. To keep this order, the `dmg` recipe scopes
the notarization vars OUT of `tauri build`'s env (`env -u APPLE_ID …`) so
**Tauri does not auto-notarize the pre-bundling `.app`** (it would otherwise,
since `APPLE_SIGNING_IDENTITY` + a notarization cred set triggers it). All
notarization happens in the explicit post-bundle step.

### CI and the release pipeline

`ci.yml` is checks-only, three tiers (2026-09-06):

- **`check-linux`** (`ubuntu-latest`) is the only job on the PR path: a
  scoped `cargo check` on the shipped app (`app/src-tauri`), skipped when the
  diff can't touch it. It only proves the workspace **compiles** on Linux —
  it does not mean Lirevo runs there (see "Platform support status" below).
- **`check-macos`** (`macos-15`) runs `just lint` + `just check` + `just
  test` on the primary platform. No artifacts. A change that breaks any of
  them breaks CI.
- **`check-windows`** (`windows-latest`) is the same compile-only guard as
  `check-linux`.

`check-macos` and `check-windows` moved to `push: main` only — they still
run on every merge, just not on every PR push. The one required status check
on `main` is `ci`, an aggregate job that needs `changes`, `check-linux`,
`check-macos` and `check-windows`, and treats a `skipped` dependency as
passing — that's what lets a docs/frontend-only PR go green on `check-linux`
alone. `preflight` (`.github/preflight.json`) is the PR-path stand-in for the
coverage that's reproducible on this machine: its `local-build-test` check
runs `just check && just test` (the Linux-reachable subset of what
`check-macos` runs) before every push, via the installed `pre-push` hook.
The macOS-only surface and the Windows compile guard aren't locally
reproducible and stay covered by `push: main` and `release.yml`.

The distributable, signed + notarized `.dmg` is built by a separate workflow:

- **`release.yml`** (`macos-15`, on a `v*` tag) imports the Developer ID cert +
  the App Store Connect API key into a temporary keychain, runs `just dmg` (which
  signs, notarizes, and staples), and uploads the `.dmg` to the GitHub Release.
  Signing/notarization secrets live in the repo's GitHub Actions secrets;
  base CI has no Apple creds. Before the build it asserts the tag matches the
  crate version and extracts the release notes from `CHANGELOG.md` — both fail
  fast rather than after ~15 minutes of signing.
- **`publish-backends.yml`** is a **skeleton** for publishing fetchable GPU
  backend module bundles (Linux/Windows Vulkan/CUDA) consumed by
  `engine/fetch.rs`. The actual build steps are TODO and the workflow is not
  enabled as a required check.

### Versioning

The app version has **one source of truth**: `[package] version` in
`app/src-tauri/Cargo.toml`. Everything else derives from it:

- `env!("CARGO_PKG_VERSION")` → `settings.app_version` → **Settings → About**.
- Tauri's Cargo.toml fallback → `CFBundleShortVersionString` → `Info.plist` →
  the `.dmg` filename (`just dmg` reads it back with PlistBuddy).
  **`tauri.conf.json` deliberately has no `version` key** — that absence is what
  makes the bundle and the in-app version the same value by construction.
  Re-adding it fails `scripts/check-versions.sh`.
- `app/package.json`'s version is inert (nothing reads it) but kept as an exact
  mirror, so it can't rot the way it did at `0.1.0` while the app shipped
  `0.9.0`.

Never hand-edit those files: run **`just release <version>`**, which bumps
`Cargo.toml`, re-resolves `Cargo.lock`, mirrors `package.json`, and inserts a
dated `CHANGELOG.md` stub. Fill the stub in — it becomes the GitHub Release
body verbatim. `scripts/check-versions.sh` runs inside `just lint` (so, in CI)
and fails on any drift.

Releasing is: `just release <version>` → fill the CHANGELOG → PR → merge →
tag `v<version>` on `main` → `release.yml` publishes.

### Platform support status

macOS (Apple Silicon) is the only platform Lirevo is functional on end-to-end
today. `os-integration` has real Linux (evdev hotkey, enigo paste, arboard
clipboard) and Windows (Win32 hotkey, inject, overlay) backends behind the
platform-neutral abstractions, plus a stub fallback, and the `ci` workflow's
`check-linux` / `check-windows` jobs keep them compiling — but they are
**compile-validated only, not runtime-tested**.
Do not describe Linux/Windows as usable; they are in progress / planned for v2.

## Working in a worktree, next to other agents

**Share the `target` dir, or a second worktree cold-builds it.** There are two
Cargo workspaces here: the root workspace (8 crates, see Common commands above)
and `app/src-tauri` (the shipped host, its own workspace, per the CI
`rust-cache` config in `.github/workflows/ci.yml`). Neither `.cargo/config.toml`
nor either workspace sets `target-dir`, so each checkout gets its own multi-GB
`target/` by default. Point a second worktree at the main checkout's cache
before building: from the repo root, `export CARGO_TARGET_DIR=$(git -C
<main-checkout> rev-parse --show-toplevel)/target`; from `app/src-tauri`, the
same idea points at `<main-checkout>/app/src-tauri/target`. Cargo's own target
lock then serializes the heavy parts, so two worktrees building at once do not
both peak RAM.

**Only one worktree can run the app at a time.** `just dev` and
`just dev-bundle` fix Vite on `:1420` with `strictPort: true`
(`app/vite.config.js`), with no per-checkout override, so a second worktree's
`just dev` fails loudly with "Port 1420 is already in use" instead of quietly
moving to the next port. That is the failure you want; it just means you
cannot run two dev sessions side by side.

**The app-data dir is shared by app name, not by checkout path.**
`paths::data_dir` (`app/src-tauri/src/paths.rs`) resolves to `~/Library/
Application Support/Lirevo` (release) or the `Lirevo (Dev)` sibling (debug),
the same path no matter which worktree launched the process. Two worktrees
running `just dev-bundle` at once share the same `data.db` (dictation
history), the same `settings.json`, and the same downloaded `models/`, which
is actually useful for the multi-GB GGUF files since nothing needs a copy per
worktree. It also means one worktree's settings change or history write shows
up in the other's live session, and both hold the same stable-signing TCC
identity. There is no per-worktree isolation for any of this today, so
sequence UI verification across worktrees rather than running it at the same
time.

**The board is Project #1, and its conventions are at the end of this file.** An earlier
read of this repo concluded there was no board; `gh project view 1 --owner fiorelorenzo`
resolves it, so plan against it and read its field, label and milestone schema from the API
before filing rather than guessing the shape.

**Pushing to `main` is blocked, not just discouraged.** The
`require-pull-request` ruleset enforces it: squash is the only allowed merge
method, no approving review is required, and `protect-default-branch`
requires the single `ci` context to pass, branch-up-to-date not enforced
(`strict_required_status_checks_policy: false` - `ci` aggregates
`check-linux`, `check-macos` and `check-windows` with `skipped` counting as
passing, see "CI and the release pipeline" above).
`delete_branch_on_merge` is on, so a merged branch disappears from the remote
on its own; no manual `git push -d` needed.

## Code conventions

- **License headers:** **Do not** add per-file SPDX/Copyright headers. The
  Apache-2.0 `LICENSE` and `NOTICE` at the repo root cover the whole codebase.
- **Rust:**
  - `cargo fmt` is mandatory; `cargo clippy --workspace --all-targets -- -D warnings` is enforced by CI.
  - `clippy::pedantic` is enabled per-crate via `#![warn(clippy::pedantic)]` —
    new warnings should be fixed, not silenced, unless there's a clear reason
    (then `#[allow(...)]` with a one-line justification).
  - MSRV is 1.85 (`clippy.toml`); the toolchain pins 1.88.
- **Frontend:**
  - Svelte 5 runes syntax (`$state`, `$derived`, `$effect`). No legacy
    `$:`/`writable` patterns in new code.
  - Tailwind v4 + shadcn-svelte primitives. Prefer composing existing
    components over hand-rolled markup.
  - Vitest for unit tests (`app/src/lib/__tests__/`). One file per module.
- **Comments:** Default to none. Add a comment only when the *why* is
  non-obvious (hidden constraint, subtle invariant, workaround for a specific
  bug). Don't restate what well-named code already says.
- **Commits:** Conventional Commits (`feat(scope):`, `fix(scope):`,
  `chore:`, `docs:`, `ci:`, `test:`, `refactor:`). Keep the subject under
  ~72 chars; put detail in the body. PR titles follow the same convention.

## macOS-specific gotchas

These will bite any agent doing UI or permission work — read before touching
hotkey, microphone, or injection code paths.

1. **TCC is bound to the code-signing identity hash, not the bundle ID.**
   A permission granted to one build of the app does *not* transfer to another
   build with a different signing hash, even if the bundle ID matches. Every
   ad-hoc-signed debug bundle is a fresh TCC entity. `just dev-bundle` works
   around this by re-signing with a stable Developer ID identity (see "stable
   TCC grants" under Common commands) so grants persist across rebuilds.
2. **Dev and prod use distinct bundle IDs.** Debug builds (`just dev`,
   `just dev-bundle`) run as `ai.lirevo.app.dev`; release (`just dmg`) is
   `ai.lirevo.app`. macOS keys Caches, WebKit storage, Preferences, and TCC on
   the bundle ID, so the debug app never inherits or pollutes the release app's
   system state. Data and log directories are split separately, by app name,
   in `paths.rs`: release uses `~/Library/Application Support/Lirevo` and
   `~/Library/Logs/Lirevo`; debug uses the same paths with a `Lirevo (Dev)` leaf.
3. **`just dev` cannot trigger TCC prompts.** The bare binary launched by
   `cargo tauri dev` is not a proper `.app` bundle; macOS silently denies the
   prompt. To exercise real permission UX use `just dev-bundle` (debug `.app`)
   or `just dmg` (release `.app`).
4. **Hardened runtime is intentionally omitted from `dev-bundle`.** The bundled
   inference libraries (ggml/Metal dynamic backends, llama-cpp-2, parakeet-cpp)
   are not all signed
   by the same Team ID and JIT Metal shaders at runtime, so hardened runtime
   stops the app from launching. `dev-bundle` re-signs without `--options
   runtime`; `entitlements.plist` (`cs.disable-library-validation`,
   `cs.allow-jit`, `cs.allow-unsigned-executable-memory`) covers the release DMG.
5. **Reset stale grants before re-testing permissions.** Use the bundle ID that
   matches the build — `ai.lirevo.app.dev` for dev bundles, `ai.lirevo.app` for
   the release DMG:
   ```bash
   # debug (dev-bundle)
   tccutil reset Microphone     ai.lirevo.app.dev
   tccutil reset Accessibility  ai.lirevo.app.dev
   ```
   `just reset` / `just reset-all` clear these automatically.
6. **`LIREVO_DEV_SKIP_PERMS=1`** short-circuits the `check_*` / `prompt_*`
   commands to "granted" and makes `test_mic` return a synthetic envelope.
   Debug builds only. Use it for UI iteration when you don't need real audio
   or real prompts.
7. **Text injection always goes through `NSPasteboard`** — there is no
   AXUIElement path and no `force_pasteboard` toggle. Injection snapshots the
   *entire* current pasteboard (every item, every concrete type via
   `dataForType:`, not just the string content), writes our text, synthesizes
   a Cmd+V key event, waits for the target app to consume the paste, then
   restores the full pre-injection snapshot via `setData:forType:`. Types
   backed by promised/lazy data (`dataForType:` returns `nil`) can't be
   snapshotted and are silently dropped — a known limitation, not a bug. Don't
   add new code paths that change this without a settings toggle.

## What NOT to commit

The repo is **public and open source** under Apache-2.0. Treat anything you
add to git as world-readable forever.

- **Model weights** (`*.bin`, `*.gguf`, `*.mlmodelc/`, `models/`). Multi-GB
  binaries; users download their own in-app via the setup wizard — see the
  README provisioning sections.
- **Build artifacts** (`target/`, `dist/`, `build/`, `out/`, `*.dmg`, `node_modules/`).
- **Test audio fixtures** (`crates/inference-core/tests/fixtures/*.wav`).
- **Local working docs** (`docs/plans/`, `docs/specs/`). The `docs/` directory
  is intentionally not tracked beyond the gitignore entries — design docs are
  local-only.
- **Tool-local state** (`.claude/`, `.vscode/`, `.idea/`).
- **Secrets of any kind.** This codebase does not call any cloud API; there
  should be no API keys, tokens, or credentials anywhere. If you find one,
  treat it as a leak — rotate immediately and rewrite history.

If you're unsure whether something is safe to commit, ask first or open a
draft PR — don't push to `main`.

## Working safely as an agent

- **Branch:** Never commit directly to `main`. Create a feature branch
  (`feat/...`, `fix/...`) and open a PR.
- **Scope:** Do what the task asks. Don't refactor unrelated code, don't
  rename files "while you're there", don't introduce new dependencies without
  flagging it in the PR description.
- **Verification before claiming done:** Run the minimal covering subset of
  tests/lint/typecheck for what you changed (see "Local verification" under
  Common commands) before saying a task is complete — full `just check` /
  `just lint` / `just test` only for release-critical changes. CI runs the
  full matrix on every PR and is the actual gate; a CI failure on `main`
  blocks everyone.
- **Destructive ops** (force push, `git reset --hard`, deleting branches,
  rewriting history): get explicit confirmation from a human first.
- **macOS permissions:** If a flow needs the real TCC prompt, say so in the
  PR — reviewers will need to test with `just dev-bundle` or a fresh `.dmg`.

## Where to look first

- **What changed recently?** `CHANGELOG.md` and `git log --oneline -20`.
- **How does X currently work?** Start in `app/src-tauri/src/` for backend
  flows, `app/src/routes/` for UI flows, or the relevant crate under
  `crates/` for inference / OS plumbing.
- **What's the user-visible behaviour?** `README.md` ("Using the app" and the
  model-provisioning sections).

## The GitHub Project is the source of truth

Current state and future roadmap live on **Project #1 "Lirevo roadmap"** (owner
`fiorelorenzo`), not in this file and not in a chat transcript. Keeping it
current is part of doing the work, not paperwork at the end: the board is how
Lorenzo sees where the project stands without reading session logs, so a board
that lags reality is worse than no board.

**Status is a claim about reality, keep it true.**

- Before you write code for an issue, move it to `In Progress`. If what you are
  about to do has no issue, create one first (see below), then start.
- Move it to `Done` only when the change is merged and verified, not when the
  code is written. Merged but something is still open? Say so in a comment and
  leave it `In Progress`.
- Board fields, the same four on every one of Lorenzo's roadmap boards on
  purpose: `Status` (`Todo` / `In Progress` / `Done`), `Priority` (P0-P3),
  `Effort` (S/M/L/XL) and `Parallel` (Yes/No, whether a parallel agent can take
  the issue without colliding with other work). Set all four on anything you
  file. Never write a value that is not already an option, read the schema
  instead of guessing, and never add, rename or drop a field on this board
  alone: the convention is shared across the projects.

**Comment when a reader would want to know.** A decision taken, an approach
tried and abandoned, a blocker hit, a surprise in the code, a scope change, a
finding that invalidates the issue as written. One comment per meaningful turn
in the work, not one per commit, and no routine progress narration.

**File the work you discover.** When something real surfaces mid-task or in a
conversation with Lorenzo (a bug you noticed on the way, a follow-up the fix
implies, an idea worth doing later), open an issue for it instead of silently
widening the current change or letting it evaporate. Then say in the current
issue that you split it out, with a link.

**Conventions for a new issue.** Match what the board already shows, do not
invent a parallel style:

- Title in conventional-commit form, lowercase after the colon, e.g.
  `fix(style): few-shot examples can make cleanup emit an example's text`.
- Labels follow one taxonomy, identical in every repo: exactly one `type:*`
  (`feature`, `fix`, `refactor`, `test`, `chore`, `ci`, `docs`, `design`,
  `security`, `spike`), exactly one of `priority:P0`-`priority:P3`, and one or
  more `area:*` naming the surfaces the change touches. `epic` and `flagship`
  (an epic, and headline work) are the only unprefixed labels. Priority is
  deliberately in two places, the `Priority` board field and the `priority:*`
  label, so set both.
- `area:*` values here: `cleanup`, `cross-platform`, `docs`, `eval`, `frontend`,
  `models`, `observability`, `os-integration`, `persistence`, `pipeline`,
  `release`, `stt`, `style`. Add one only when the surface really is new, and
  never reintroduce an unprefixed or differently shaped label.
- Milestone: one of the milestones still open (`v0.10`, `v1.0`, `v1.1`; `v0.8`
  and `v0.9` have shipped). Read the exact strings from the API, they contain em
  dashes and are easy to mistype:
  `gh api repos/fiorelorenzo/lirevo/milestones --jq '.[].title'`.
- **Every issue hangs off an epic, with no exceptions, and that includes an
  issue filed in the middle of an agent run.** Epics are titled `[Epic] Name`
  and carry the `epic` label; read the current ones rather than trusting a list
  in this file: `gh issue list -R fiorelorenzo/lirevo --label epic --state all`.
  If none of them fits, create a new epic (same title format, `epic` label, one
  per coherent area) and parent the issue to it. An issue with no parent is a
  defect in the board, and it is a defect that accumulates in exactly one way:
  an agent files a real finding mid-run, sets its labels and its four fields,
  and forgets the one step that is a separate GraphQL mutation. **So parent it
  in the same turn you create it**, and when a subagent files something on your
  behalf, parenting it is yours rather than theirs. This board was clean on
  2026-08-23, all 75 issues parented, which is the state to keep it in.

  The audit, worth running at the end of any run that filed issues. It pages 100
  at a time, so re-run it with `-f c=<endCursor>` until `hasNextPage` is false;
  empty output on every page is the passing state.

  ```bash
  gh api graphql -f query='query($c:String){repository(owner:"fiorelorenzo",name:"lirevo"){
    issues(first:100,after:$c,states:[OPEN,CLOSED]){pageInfo{hasNextPage endCursor}
    nodes{number parent{number} labels(first:20){nodes{name}}}}}}' \
    --jq '.data.repository.issues.nodes[] | select(.parent==null)
          | select([.labels.nodes[].name] | index("epic") | not) | .number'
  ```

```bash
# Read the schema, never guess an option value
gh project field-list 1 --owner fiorelorenzo --format json
gh api repos/fiorelorenzo/lirevo/milestones --jq '.[].title'

# Fill these three in; everything below runs as written, no placeholders to edit
ISSUE=123                 # the issue you are working on
EPIC=456                  # its parent epic
STATUS="In Progress"      # Todo | In Progress | Done

PROJECT_ID=$(gh project view 1 --owner fiorelorenzo --format json --jq '.id')
STATUS_FIELD=$(gh project field-list 1 --owner fiorelorenzo --format json \
  --jq '.fields[] | select(.name=="Status") | .id')
OPTION_ID=$(gh project field-list 1 --owner fiorelorenzo --format json \
  --jq ".fields[] | select(.name==\"Status\") | .options[] | select(.name==\"$STATUS\") | .id")
ITEM_ID=$(gh project item-list 1 --owner fiorelorenzo --format json --limit 300 \
  --jq ".items[] | select(.content.number==$ISSUE) | .id")
gh project item-edit --id "$ITEM_ID" --project-id "$PROJECT_ID" \
  --field-id "$STATUS_FIELD" --single-select-option-id "$OPTION_ID"

# New issue: create, put it on the board, hang it off its epic.
# `gh issue create` prints the new issue's URL, so capture it and reuse it.
ISSUE_URL=$(gh issue create -R fiorelorenzo/lirevo --title "fix(area): ..." --body "..." \
  --label "area:style,type:fix,priority:P1" --milestone "v1.0 — Polish, Reliability & Release Confidence")
gh project item-add 1 --owner fiorelorenzo --url "$ISSUE_URL"
gh api graphql -f query='mutation($p:ID!,$c:ID!){addSubIssue(input:{issueId:$p,subIssueId:$c}){subIssue{number}}}' \
  -f p="$(gh issue view $EPIC -R fiorelorenzo/lirevo --json id --jq '.id')" \
  -f c="$(gh issue view "$ISSUE_URL" --json id --jq '.id')"
```

`item-edit` is idempotent, so re-setting a value that is already correct is a
fine way to make sure the board is right. An issue can have only one parent: to
move it to a different epic, pass `replaceParent: true` in the same mutation.
Everything you write on an issue or a card is repo-facing text, so the writing
conventions above apply to it.
