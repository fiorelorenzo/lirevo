#!/usr/bin/env bash
#
# Reset the app's runtime state so the next launch starts from scratch:
# the wizard runs again, TCC permission grants are cleared (so the
# Accessibility / Microphone dialogs fire fresh), and on-disk logs are
# wiped. By default the downloaded model files are KEPT — they're
# multi-GB and rarely the thing you actually want to re-download. Pass
# `--models` (or `-m`) to also delete them.
#
# Intended for local dev iteration on first-run / permissions flows.
# Does NOT touch source code, build caches, or the system installation
# of the app itself; `just clean` covers the former and uninstalling
# the .app from /Applications covers the latter.
#
# Safe to re-run. Each step is best-effort and reports what it did.

set -euo pipefail

readonly BUNDLE_ID="app.localdictation"
readonly PROC_NAME="local-dictation-app"
readonly APP_DATA="$HOME/Library/Application Support/$BUNDLE_ID"
readonly APP_LOGS="$HOME/Library/Logs/$BUNDLE_ID"
readonly MODELS_DIR="$APP_DATA/models"
readonly SETTINGS_FILE="$APP_DATA/settings.json"

wipe_models=false
for arg in "$@"; do
  case "$arg" in
    -m|--models) wipe_models=true ;;
    -h|--help)
      cat <<'EOF'
Usage: scripts/reset.sh [--models]

Resets app runtime state so the next launch shows the setup wizard.

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
# Match by exact process name so we don't false-match on `just dev`'s
# `cargo` / `node` processes.
if pgrep -x "$PROC_NAME" >/dev/null 2>&1; then
  echo "error: $PROC_NAME is running. Quit it first (Cmd+Q or tray → Quit) and re-run." >&2
  exit 1
fi

if "$wipe_models"; then
  if [[ -d "$MODELS_DIR" ]]; then
    size=$(du -sh "$MODELS_DIR" 2>/dev/null | cut -f1 || echo "?")
    echo "About to delete model files at $MODELS_DIR (size: $size)."
    read -r -p "Continue? [y/N] " ans
    case "$ans" in [yY]|[yY][eE][sS]) ;; *) echo "aborted."; exit 0 ;; esac
  fi
fi

echo "→ resetting TCC grants for $BUNDLE_ID"
# `-` prefix in shell would suppress errors but we use `|| true` for clarity.
# tccutil exits 0 if the grant existed and was reset, 1 if it never existed
# (e.g. first reset, or after a previous reset). Either is fine here.
tccutil reset Accessibility "$BUNDLE_ID" 2>/dev/null || true
tccutil reset Microphone    "$BUNDLE_ID" 2>/dev/null || true

if [[ -f "$SETTINGS_FILE" ]]; then
  echo "→ removing $SETTINGS_FILE"
  rm -f "$SETTINGS_FILE"
fi

# Other tauri-plugin-store files live alongside settings.json. Sweep
# them too so the app starts with a fully blank slate.
if [[ -d "$APP_DATA" ]]; then
  find "$APP_DATA" -maxdepth 1 -name '*.json' -type f -print -delete 2>/dev/null || true
fi

if [[ -d "$APP_LOGS" ]]; then
  echo "→ removing $APP_LOGS"
  rm -rf "$APP_LOGS"
fi

if "$wipe_models" && [[ -d "$MODELS_DIR" ]]; then
  echo "→ removing $MODELS_DIR"
  rm -rf "$MODELS_DIR"
fi

# Heads-up on the one corner of state this script does NOT manage —
# the Launch-at-Login registration is owned by tauri-plugin-autostart
# and lives in macOS Login Items. Removing it programmatically is
# brittle (Apple's API has changed twice in the last few macOS releases)
# so we just point the user at the right pane if it's relevant.
if [[ "${LDA_RESET_QUIET:-}" != "1" ]]; then
  echo
  echo "Done. Next launch will run the setup wizard."
  if "$wipe_models"; then
    echo "Models were also wiped — you'll need to re-download from Settings → Models."
  fi
  echo "If you had Launch at Login enabled, also clear it manually:"
  echo "  System Settings → General → Login Items → remove '$PROC_NAME'"
fi
