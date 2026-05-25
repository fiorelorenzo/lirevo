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
  `rust-toolchain.toml`). Tauri 2 host process loads `whisper-rs` and
  `llama-cpp-2` directly as libraries — no separate sidecar process in the
  shipped app.
- **OS integration:** `cocoa` / `core-foundation` / `core-graphics` for
  CGEventTap (global hotkey) and AXUIElement (text injection).
- **Build tooling:** `just`, `cargo`, `npm`, `cargo-nextest`. Node 22, Rust 1.88
  (see `rust-toolchain.toml`).

## Repository layout

```
.
├── app/                      # Tauri app (frontend + Tauri host)
│   ├── src/                  # Svelte UI (routes, components, stores, i18n)
│   └── src-tauri/            # Tauri host crate (Rust): commands, tray,
│                             #   hotkey wiring, settings, state machine
├── crates/                   # Rust workspace
│   ├── audio-capture/        # cpal-based mic capture + resampling
│   ├── inference-core/       # whisper-rs + llama-cpp-2 wrappers, HTTP/axum
│   │                         #   layer used only by dev CLIs
│   ├── lirevo-cli/              # Dev CLI (`lirevo-cli`) talking to inference-core
│   ├── lirevo-prompts/          # Versioned LLM prompts (cleanup, etc.)
│   ├── lirevo-prototype/        # Dev-only headless dictation pipeline
│   └── os-integration/       # macOS hotkey + injection bindings
├── scripts/                  # Build/utility scripts (icons, etc.)
├── Justfile                  # Canonical task entry points
├── Cargo.toml                # Workspace manifest
└── README.md
```

`lirevo-prototype`, `lirevo-cli`, and `inference-core`'s HTTP layer are **dev-only**:
they are not bundled in the shipped DMG. Production code paths run inside the
Tauri host (`app/src-tauri/`).

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
| First-time setup                        | `just setup` (if defined) or `cd app && npm install` |
| Dev (HMR, no real TCC prompts)          | `just dev`                    |
| Dev with mocked permissions             | `LIREVO_DEV_SKIP_PERMS=1 just dev` |
| Dev with real TCC prompts (debug `.app`)| `just dev-bundle`             |
| Release `.app` + `.dmg`                 | `just dmg`                    |
| All tests (Rust nextest + Vitest)       | `just test`                   |
| Type check (Rust + Svelte)              | `just check`                  |
| Format                                  | `just fmt`                    |
| Lint (clippy `-D warnings` + eslint)    | `just lint`                   |
| Wipe build caches                       | `just clean`                  |

CI (`.github/workflows/build-mac.yml`) runs `just check`, `just test`, and
`just dmg` on `macos-15`. A change that breaks any of those breaks CI.

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
   ad-hoc-signed debug bundle is a fresh TCC entity.
2. **`just dev` cannot trigger TCC prompts.** The bare binary launched by
   `cargo tauri dev` is not a proper `.app` bundle; macOS silently denies the
   prompt. To exercise real permission UX use `just dev-bundle` (debug `.app`)
   or `just dmg` (release `.app`).
3. **Reset stale grants before re-testing permissions:**
   ```bash
   tccutil reset Microphone     ai.lirevo.app
   tccutil reset Accessibility  ai.lirevo.app
   ```
4. **`LIREVO_DEV_SKIP_PERMS=1`** short-circuits the `check_*` / `prompt_*`
   commands to "granted" and makes `test_mic` return a synthetic envelope.
   Debug builds only. Use it for UI iteration when you don't need real audio
   or real prompts.
5. **Text injection has two paths**: AXUIElement (preferred) with a pasteboard
   fallback. Pasteboard fallback overwrites the user's clipboard, restores it
   after the paste, and loses non-string clipboard content (images/files) in
   the process. Don't add new code paths that change this without a settings
   toggle.

## What NOT to commit

The repo is **public and open source** under Apache-2.0. Treat anything you
add to git as world-readable forever.

- **Model weights** (`*.bin`, `*.gguf`, `*.mlmodelc/`, `models/`). Multi-GB
  binaries; users provide their own — see README sections
  "Whisper model provisioning" / "LLM model provisioning".
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
- **What's the user-visible behaviour?** `README.md` (sections "Using the
  app" and the M1/M2 provisioning blocks).
