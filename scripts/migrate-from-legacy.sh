#!/usr/bin/env bash
#
# One-time migration helper: move data from the legacy `app.localdictation`
# bundle layout to the current `ai.lirevo.app` bundle layout.
#
# Reason: the rebrand from "local-dictation-app" to Lirevo (2026-05-25) changed
# the bundle identifier from `app.localdictation` to `ai.lirevo.app`. macOS
# treats these as completely separate apps — different TCC entities, different
# Application Support and Logs directories. Without this script, a user who
# installed the old build would have orphaned multi-GB model downloads sitting
# under the old path forever.
#
# What this script does:
# - Detects `~/Library/Application Support/app.localdictation/` if present.
# - MOVES its contents (models, settings, logs) into the new
#   `~/Library/Application Support/ai.lirevo.app/` location. If the new dir
#   already has files, prompts before overwriting.
# - Removes the empty legacy directory.
# - Resets TCC grants for both bundle IDs so the next launch shows fresh
#   permission prompts under the new identity.
#
# Safe to run on a system that never had the legacy install — exits early.
# Safe to re-run; the migration becomes a no-op once the legacy dir is gone.

set -euo pipefail

readonly OLD_BUNDLE_ID="app.localdictation"
readonly NEW_BUNDLE_ID="ai.lirevo.app"
readonly OLD_DATA="$HOME/Library/Application Support/$OLD_BUNDLE_ID"
readonly NEW_DATA="$HOME/Library/Application Support/$NEW_BUNDLE_ID"
readonly OLD_LOGS="$HOME/Library/Logs/$OLD_BUNDLE_ID"
readonly NEW_LOGS="$HOME/Library/Logs/$NEW_BUNDLE_ID"

# Refuse to run while either app could be alive.
for proc in "local-dictation-app" "Lirevo"; do
  if pgrep -x "$proc" >/dev/null 2>&1; then
    echo "error: $proc is running. Quit it first (Cmd+Q or tray → Quit) and re-run." >&2
    exit 1
  fi
done

migrated_anything=false

if [[ -d "$OLD_DATA" ]]; then
  echo "==> Found legacy data at $OLD_DATA"
  size=$(du -sh "$OLD_DATA" 2>/dev/null | cut -f1 || echo "?")
  echo "    Size: $size"

  if [[ -d "$NEW_DATA" ]] && [[ -n "$(ls -A "$NEW_DATA" 2>/dev/null)" ]]; then
    echo "    Destination $NEW_DATA is not empty."
    read -r -p "    Overwrite conflicting files? [y/N] " ans
    case "$ans" in [yY]|[yY][eE][sS]) ;; *) echo "    Skipped data migration."; exit 0 ;; esac
  fi

  mkdir -p "$NEW_DATA"
  # Use rsync to preserve permissions + handle existing-dir merge cleanly.
  rsync -a --remove-source-files "$OLD_DATA/" "$NEW_DATA/"
  # Remove now-empty directories left by rsync.
  find "$OLD_DATA" -type d -empty -delete 2>/dev/null || true
  echo "    ✓ Data migrated to $NEW_DATA"
  migrated_anything=true
fi

if [[ -d "$OLD_LOGS" ]]; then
  echo "==> Found legacy logs at $OLD_LOGS"
  # Logs are low-value (replaced by fresh logs at next launch). Just remove,
  # don't bother migrating.
  rm -rf "$OLD_LOGS"
  echo "    ✓ Legacy logs removed (fresh logs will appear at $NEW_LOGS)"
  migrated_anything=true
fi

echo "==> Resetting TCC grants for both bundle IDs"
for bundle in "$OLD_BUNDLE_ID" "$NEW_BUNDLE_ID"; do
  tccutil reset Accessibility "$bundle" 2>/dev/null || true
  tccutil reset Microphone    "$bundle" 2>/dev/null || true
done
echo "    ✓ TCC reset (mic + accessibility prompts will appear fresh on next launch)"

if ! "$migrated_anything"; then
  echo ""
  echo "Nothing to migrate — no legacy bundle data found. Safe no-op."
else
  echo ""
  echo "Done. Launch Lirevo to verify everything is in place."
  echo "If you had Launch at Login enabled for the old bundle, also remove it:"
  echo "  System Settings → General → Login Items → remove 'local-dictation-app'"
fi
