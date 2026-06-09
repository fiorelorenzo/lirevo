#!/usr/bin/env bash
#
# Notarize + staple a built, already-bundled-and-re-signed macOS `.app`, and
# (optionally) a `.dmg` containing it.
#
# Where this fits in the release flow
# -----------------------------------
# The release `.app` is NOT distributable straight from `tauri build`: our
# `scripts/bundle-macos-install.sh` step relocates the bundled inference dylibs +
# ggml backend modules into the `.app` and RE-SIGNS it. Any modification after a
# notarization invalidates that notarization, so the order MUST be:
#
#   1. `tauri build` (sign only — Tauri's own auto-notarization is suppressed by
#      scoping the notarization creds out of its env; see the `dmg` recipe).
#   2. `bundle-macos-install.sh` (relocate dylibs + re-sign).
#   3. THIS SCRIPT: notarize the re-signed `.app`, then `stapler staple` it.
#   4. Build the `.dmg` from the now-stapled `.app`.
#   5. THIS SCRIPT (again, optionally): notarize + staple the `.dmg` too, so the
#      download itself passes Gatekeeper before it is even mounted.
#
# `notarytool` cannot submit a bare `.app` directory; it needs a `.zip`/`.dmg`/
# `.pkg`. We zip the `.app` (ditto, preserving symlinks/xattrs) purely as a
# submission container, then staple the ORIGINAL `.app` on success — the ticket
# is keyed on the code-signing hash, not the container.
#
# Credentials (one of the two styles; API key preferred)
# ------------------------------------------------------
# App Store Connect API key (preferred — no app-specific password to rotate):
#   APPLE_API_KEY      filesystem path to the .p8 private key
#   APPLE_API_KEY_ID   the 10-char Key ID
#   APPLE_API_ISSUER   the issuer UUID
# Apple ID:
#   APPLE_ID           developer Apple ID email
#   APPLE_PASSWORD     app-specific password (NOT your account password)
#   APPLE_TEAM_ID      developer Team ID
#
# If NEITHER complete set is present this script prints a clear warning and
# exits 0, so `just dmg` still produces an (un-notarized) build locally and in
# CI without Apple credentials. An un-notarized `.app` runs on THIS machine but
# is Gatekeeper-rejected ("cannot be opened because the developer cannot be
# verified") on any other machine.
#
# Usage:
#   scripts/notarize-macos.sh app <App.app>
#   scripts/notarize-macos.sh dmg <Disk.dmg>
set -euo pipefail

KIND="${1:?usage: notarize-macos.sh <app|dmg> <path>}"
TARGET_PATH="${2:?usage: notarize-macos.sh <app|dmg> <path>}"

if [ ! -e "$TARGET_PATH" ]; then
  echo "error: notarization target not found: $TARGET_PATH" >&2
  exit 1
fi

# --- decide which credential style is available ------------------------------
# Build the notarytool auth args into an array. Empty => no usable creds.
auth_args=()
auth_style=""
if [ -n "${APPLE_API_KEY:-}" ] && [ -n "${APPLE_API_KEY_ID:-}" ] && [ -n "${APPLE_API_ISSUER:-}" ]; then
  if [ ! -f "$APPLE_API_KEY" ]; then
    echo "error: APPLE_API_KEY is set but the .p8 file does not exist: $APPLE_API_KEY" >&2
    exit 1
  fi
  auth_style="App Store Connect API key (key-id ${APPLE_API_KEY_ID})"
  auth_args=(--key "$APPLE_API_KEY" --key-id "$APPLE_API_KEY_ID" --issuer "$APPLE_API_ISSUER")
elif [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_PASSWORD:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ]; then
  auth_style="Apple ID (${APPLE_ID})"
  auth_args=(--apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" --team-id "$APPLE_TEAM_ID")
else
  echo "notarize: skipping notarization (no APPLE_* creds in env)." >&2
  echo "notarize: the .app/.dmg is signed but NOT notarized; it will be" >&2
  echo "notarize: Gatekeeper-rejected on other machines. Set the App Store" >&2
  echo "notarize: Connect API key vars (APPLE_API_KEY/APPLE_API_KEY_ID/" >&2
  echo "notarize: APPLE_API_ISSUER) or the Apple ID vars (APPLE_ID/" >&2
  echo "notarize: APPLE_PASSWORD/APPLE_TEAM_ID) to enable it." >&2
  exit 0
fi

echo "notarize: authenticating via ${auth_style}"

# --- submit -----------------------------------------------------------------
case "$KIND" in
  app)
    # notarytool cannot take a bare .app; zip it as a submission container.
    submission="$(mktemp -d)/$(basename "$TARGET_PATH").zip"
    echo "notarize: zipping $TARGET_PATH for submission"
    /usr/bin/ditto -c -k --keepParent "$TARGET_PATH" "$submission"
    ;;
  dmg)
    submission="$TARGET_PATH"
    ;;
  *)
    echo "error: first arg must be 'app' or 'dmg', got: $KIND" >&2
    exit 1
    ;;
esac

echo "notarize: submitting $submission (this can take several minutes)"
xcrun notarytool submit "$submission" "${auth_args[@]}" --wait

# --- staple -----------------------------------------------------------------
echo "notarize: stapling ticket to $TARGET_PATH"
xcrun stapler staple "$TARGET_PATH"
xcrun stapler validate "$TARGET_PATH"

# Clean up the throwaway zip container (not the original .app/.dmg).
if [ "$KIND" = "app" ]; then
  rm -rf "$(dirname "$submission")"
fi

echo "notarize: done — $TARGET_PATH is notarized + stapled"
