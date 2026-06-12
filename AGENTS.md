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
  CGEventTap (global hotkey) and AXUIElement (text injection).
- **Build tooling:** `just`, `cargo`, `pnpm`, `cargo-nextest`. Node 22 (see
  `.nvmrc`; CI pins the same), pnpm 9.15.0 (pinned via `packageManager`; enable
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

Base CI is **checks-only**:

- **`build-mac.yml`** (`macos-15`, on push/PR) runs `just check` + `just test`.
  No artifacts. A change that breaks either breaks CI.
- **`cross-platform-check.yml`** (Ubuntu + Windows, on push/PR) `cargo check`s
  the shipped app (`app/src-tauri`) to guard cross-platform compilation. It
  proves the workspace **compiles** on those targets — it does **not** mean
  Lirevo runs there (see "Platform support status" below).

The distributable, signed + notarized `.dmg` is built by a separate workflow:

- **`release.yml`** (`macos-15`, on a `v*` tag) imports the Developer ID cert +
  the App Store Connect API key into a temporary keychain, runs `just dmg` (which
  signs, notarizes, and staples), and uploads the `.dmg` to the GitHub Release.
  Signing/notarization secrets live in the repo's GitHub Actions secrets;
  base CI has no Apple creds.
- **`publish-backends.yml`** is a **skeleton** for publishing fetchable GPU
  backend module bundles (Linux/Windows Vulkan/CUDA) consumed by
  `engine/fetch.rs`. The actual build steps are TODO and the workflow is not
  enabled as a required check.

### Platform support status

macOS (Apple Silicon) is the only platform Lirevo is functional on end-to-end
today. `os-integration` has real Linux (evdev hotkey, enigo paste, arboard
clipboard) and Windows (Win32 hotkey, inject, overlay) backends behind the
platform-neutral abstractions, plus a stub fallback, and `cross-platform-check`
keeps them compiling — but they are **compile-validated only, not runtime-tested**.
Do not describe Linux/Windows as usable; they are in progress / planned for v2.

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
7. **Text injection has two paths**: AXUIElement (preferred) with a pasteboard
   fallback. Pasteboard fallback overwrites the user's clipboard, restores it
   after the paste, and loses non-string clipboard content (images/files) in
   the process. Don't add new code paths that change this without a settings
   toggle.

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
- **Verification before claiming done:** Run `just check` and `just test`
  (and `just lint` if you touched Rust) before saying a task is complete.
  CI failure on `main` blocks everyone.
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
