# Opening a pull request in Lirevo: what the kit's skill leaves to us

The workflow kit's `pr-creation` skill (`.agents/skills/pr-creation/SKILL.md`) is the
procedure; this file is the part only this repository does. Read the skill first, then
this. Where the two disagree, this file is the more specific one and wins.

## Before the first commit in a worktree

A fresh worktree has no dependencies and no shared build cache: `cd app && pnpm install
--frozen-lockfile`, and point `CARGO_TARGET_DIR` at the main checkout's `target/` (both
in `AGENTS.md`, "Working in a worktree"). Only one worktree can run `just dev` at a time
(Vite is fixed on `:1420`).

## What the PR body has to say here

These go in **How I verified it** or **Anything a reviewer should look at twice**:

- **Which scoped checks ran**, by name: `python3 .github/kit/bin/preflight` (it selects
  `app-check`, `rust-check` and `rust-test` from the diff), or the scoped commands under
  "Local verification" in `AGENTS.md`. Run the full `just check && just lint && just test`
  for release-critical changes (release pipeline, signing and notarization scripts,
  migrations) and say so.
- **The Linux and Windows gap**, when the diff touches `os-integration`, `audio-capture`
  or path and permission handling: whether `cargo check --workspace --target
  x86_64-unknown-linux-gnu` ran, and if it did not, that it did not.
- **A real TCC prompt**, when a flow needs one: the reviewer has to test with `just
  dev-bundle` or a fresh `.dmg`, because `just dev` cannot trigger the prompt.
- **A new dependency**, named, with why.

`check-macos` and `check-windows` do not run on a PR (`push: main` only), so a green `ci`
on a PR does not mean the macOS or Windows legs passed; they report on the merge commit.

## Screenshots

The app is a menu-bar and window UI. A change a person could see gets a before-and-after
pair as the template says. A TCC prompt cannot appear under `just dev`; when the pair
has to show one, it comes from `just dev-bundle` and the PR says so.

## Merge

Squash is the only method the ruleset allows: `gh pr merge <n> --auto --squash
--delete-branch`, then `git checkout main && git reset --hard origin/main` in the main
checkout, because local `main` diverges on every squash (`AGENTS.md`, "Pull requests").

## A release PR

`just release <version>` bumps `app/src-tauri/Cargo.toml`, re-resolves `Cargo.lock`,
mirrors `app/package.json` and inserts a dated `CHANGELOG.md` stub. Fill the stub in: it
becomes the GitHub Release body verbatim. Open the PR with the `chore(release):` type and
scope, merge it, then tag `v<version>` on `main`; `release.yml` builds, notarizes and
publishes the `.dmg`. Never hand-edit the version files (`AGENTS.md`, "Versioning").
