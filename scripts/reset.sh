#!/usr/bin/env bash
#
# Reset the app's runtime state so the next launch starts from scratch:
# the wizard runs again, TCC permission grants are cleared (so the
# Accessibility / Microphone dialogs fire fresh), and on-disk logs are
# wiped. By default the downloaded model files are KEPT — they're
# multi-GB and rarely the thing you actually want to re-download. Pass
# `--models` (or `-m`) to also delete them.
#
# Handles BOTH the release build (bundle id `ai.lirevo.app`, data under
# `~/Library/Application Support/Lirevo`) and the debug/dev-bundle build
# (bundle id `ai.lirevo.app.dev`, data under `…/Lirevo (Dev)`). Data and
# log directories are keyed by APP NAME, not bundle id — see
# `app/src-tauri/src/paths.rs` (`app_dir_name` + `rebase`). TCC grants,
# by contrast, are keyed by BUNDLE ID, so both ids are reset.
#
# The local SQLite history DB (`data.db`) is intentionally PRESERVED — a
# permissions/first-run reset shouldn't cost you your dictation history.
# Only settings + tauri-plugin-store JSON, logs, and (with `--models`)
# the model files are removed.
#
# Intended for local dev iteration on first-run / permissions flows.
# Does NOT touch source code, build caches, or the system installation
# of the app itself; `just clean` covers the former and uninstalling
# the .app from /Applications covers the latter.
#
# Safe to re-run. Each step is best-effort and reports what it did.

set -euo pipefail

readonly PROC_NAME="Lirevo"
# Release + debug variants. APP_NAMES index the data/log dirs (by app name);
# BUNDLE_IDS index the TCC grants (by bundle id). Order is release, debug.
readonly APP_NAMES=("Lirevo" "Lirevo (Dev)")
readonly BUNDLE_IDS=("ai.lirevo.app" "ai.lirevo.app.dev")
# Legacy bundle id from before the 2026-05-25 rename. Cleaned up alongside
# the current ones so a reset doesn't leave orphaned dirs under the old name.
readonly LEGACY_BUNDLE_ID="app.localdictation"

readonly SUPPORT_BASE="$HOME/Library/Application Support"
readonly LOGS_BASE="$HOME/Library/Logs"

wipe_models=false
for arg in "$@"; do
  case "$arg" in
    -m|--models) wipe_models=true ;;
    -h|--help)
      cat <<'EOF'
Usage: scripts/reset.sh [--models]

Resets app runtime state so the next launch shows the setup wizard.
Covers both the release (Lirevo / ai.lirevo.app) and debug
(Lirevo (Dev) / ai.lirevo.app.dev) builds. The history DB is preserved.

Options:
  -m, --models    Also delete the downloaded model files (multi-GB).
                  Without this flag, models in the data dir are kept.
  -h, --help      Show this help.
EOF
      exit 0
      ;;
    *)
      echo "unknown arg: $arg (try --help)" >&2
      exit 2
      ;;
  esac
done

# Refuse to run while the app is alive — tccutil + settings.json edits
# while a live process is reading them produce confusing partial state.
# Both the release and dev-bundle processes share the product name "Lirevo".
if pgrep -x "$PROC_NAME" >/dev/null 2>&1; then
  echo "error: $PROC_NAME is running. Quit it first (Cmd+Q or tray → Quit) and re-run." >&2
  exit 1
fi

# Confirm before deleting model files (sum across both variants).
if "$wipe_models"; then
  found_models=false
  for name in "${APP_NAMES[@]}"; do
    models_dir="$SUPPORT_BASE/$name/models"
    if [[ -d "$models_dir" ]]; then
      size=$(du -sh "$models_dir" 2>/dev/null | cut -f1 || echo "?")
      echo "About to delete model files at $models_dir (size: $size)."
      found_models=true
    fi
  done
  if "$found_models"; then
    read -r -p "Continue? [y/N] " ans
    case "$ans" in [yY]|[yY][eE][sS]) ;; *) echo "aborted."; exit 0 ;; esac
  fi
fi

# TCC: reset for every bundle id (release + debug).
for bid in "${BUNDLE_IDS[@]}"; do
  echo "→ resetting TCC grants for $bid"
  tccutil reset Accessibility "$bid" 2>/dev/null || true
  tccutil reset Microphone    "$bid" 2>/dev/null || true
done

# Per variant: wipe settings + tauri-plugin-store JSON, logs, and (optional)
# model files. The history DB (data.db*) is left untouched on purpose.
for name in "${APP_NAMES[@]}"; do
  data_dir="$SUPPORT_BASE/$name"
  logs_dir="$LOGS_BASE/$name"

  if [[ -d "$data_dir" ]]; then
    echo "→ clearing settings/store JSON in $data_dir (history db kept)"
    find "$data_dir" -maxdepth 1 -name '*.json' -type f -print -delete 2>/dev/null || true
    if "$wipe_models" && [[ -d "$data_dir/models" ]]; then
      echo "→ removing $data_dir/models"
      rm -rf "$data_dir/models"
    fi
  fi

  if [[ -d "$logs_dir" ]]; then
    echo "→ removing $logs_dir"
    rm -rf "$logs_dir"
  fi
done

# Legacy bundle cleanup: pre-2026-05-25 the app stored data under the
# bundle id `app.localdictation`. Any leftovers are orphaned; wipe them.
# Use scripts/migrate-from-legacy.sh first if you want to keep old models.
readonly LEGACY_APP_DATA="$SUPPORT_BASE/$LEGACY_BUNDLE_ID"
readonly LEGACY_APP_LOGS="$LOGS_BASE/$LEGACY_BUNDLE_ID"
if [[ -d "$LEGACY_APP_DATA" ]] || [[ -d "$LEGACY_APP_LOGS" ]]; then
  echo "→ wiping legacy $LEGACY_BUNDLE_ID dirs (run scripts/migrate-from-legacy.sh first to keep models)"
  rm -rf "$LEGACY_APP_DATA" "$LEGACY_APP_LOGS"
  tccutil reset Accessibility "$LEGACY_BUNDLE_ID" 2>/dev/null || true
  tccutil reset Microphone    "$LEGACY_BUNDLE_ID" 2>/dev/null || true
fi

# Heads-up on the one corner of state this script does NOT manage —
# the Launch-at-Login registration is owned by tauri-plugin-autostart
# and lives in macOS Login Items. Removing it programmatically is
# brittle (Apple's API has changed twice in the last few macOS releases)
# so we just point the user at the right pane if it's relevant.
if [[ "${LIREVO_RESET_QUIET:-}" != "1" ]]; then
  echo
  echo "Done. Next launch will run the setup wizard."
  if "$wipe_models"; then
    echo "Models were also wiped — you'll need to re-download from Settings → Models."
  fi
  echo "If you had Launch at Login enabled, also clear it manually:"
  echo "  System Settings → General → Login Items → remove '$PROC_NAME'"
fi
