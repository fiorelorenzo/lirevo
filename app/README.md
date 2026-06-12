# Lirevo — Tauri app

This directory is the Lirevo desktop app: the Svelte 5 frontend plus the Tauri 2
host crate. It is the only part of the project bundled into the shipped DMG.

## Layout

- `src/` — the Svelte 5 (runes) + SvelteKit frontend, rendered in WKWebView:
  routes (home, overlay, settings, wizard), components, stores, and i18n
  (`locales/`).
- `src-tauri/` — the Rust host crate. It loads the inference backends in-process
  (audiopipe for STT, llama-cpp-2 for the LLM) — there is no sidecar process in
  the shipped app — and owns the IPC commands (`src/commands/`), menu-bar tray
  (`src/tray.rs`), push-to-talk hotkey coordinator (`src/hotkey.rs`), persisted
  settings (`src/settings.rs`), resource-aware model lifecycle (`src/engine/`),
  STT model layer (`src/stt/`), and local SQLite history (`src/db/`).

## Running it

Use the root `Justfile`; these recipes are the contract CI runs.

- `just dev` — Vite HMR with the dev bundle id (`ai.lirevo.app.dev`). Fast
  iteration, but macOS TCC permission prompts cannot appear from this bare
  binary. Add `LIREVO_DEV_SKIP_PERMS=1 just dev` to mock the permission and
  microphone-test paths in debug builds.
- `just dev-bundle` — builds a proper debug `.app` so the real Microphone and
  Accessibility prompts work; use this when iterating on permission flows.

First-time setup if you run the frontend on its own: `cd app && pnpm install`.

## More context

See the repository root `README.md` for end-user setup and model provisioning,
and `AGENTS.md` for the architecture, crate layout, conventions, and macOS
gotchas.
