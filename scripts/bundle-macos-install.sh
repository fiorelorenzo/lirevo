#!/usr/bin/env bash
#
# Relocate the two inference engines' native libraries into a built macOS `.app`
# so the bundle is self-contained and runs the ggml DYNAMIC backends without the
# dev-only absolute rpaths baked in by `cargo build`.
#
# Why this exists
# ---------------
# The host binary links two ggml-based engines as `GGML_BACKEND_DL` dynamic
# backends:
#   * parakeet-cpp (STT) — libparakeet.dylib + ggml 0.13.x + Metal/CPU modules
#   * llama-cpp-2  (LLM) — libllama.dylib   + ggml 0.9.x  + Metal/CPU modules
# Both ship a `libggml-base.0.dylib` / `libggml.0.dylib` with the SAME `@rpath`
# install name but DIFFERENT, ABI-incompatible versions. `build.rs` already
# disambiguates parakeet's copies with a `lirevo_pk_` install-name prefix so dyld
# keeps both; llama keeps the bare names. We must PRESERVE that disambiguation in
# the bundle (we copy the already-renamed staged parakeet dylibs verbatim).
#
# At build time the binary's only rpaths point into `target/.../build/.../out`,
# which obviously don't exist on a user's machine. This script:
#   1. copies every engine dylib into `Contents/Frameworks`,
#   2. copies the loadable `.so` backend modules into
#      `Contents/Resources/backends/{parakeet,llama}`,
#   3. rewrites the main binary's rpaths to a single `@loader_path/../Frameworks`,
#   4. adds an rpath to each backend module so its `@rpath/libggml-base*` (or the
#      `lirevo_pk_`-prefixed parakeet variant) resolves to `Contents/Frameworks`
#      when the module is dlopen'd by absolute path,
#   5. re-signs every dylib/module and the whole app.
#
# Runtime resolution of the two backend dirs is done in Rust
# (`engine::backend::bundled_backends`): it locates `Contents/Resources/backends`
# relative to the running executable, so no absolute path is baked in.
#
# This step is reusable for `just dev-bundle`, `just dmg`, and the Phase 3b fetch
# flow.
#
# Usage:
#   scripts/bundle-macos-install.sh <App.app> <profile: debug|release> [target-triple]
#
# Env:
#   APPLE_SIGNING_IDENTITY  re-sign with this identity (else ad-hoc `-`).
set -euo pipefail

APP="${1:?usage: bundle-macos-install.sh <App.app> <debug|release> [target]}"
PROFILE="${2:?usage: bundle-macos-install.sh <App.app> <debug|release> [target]}"
TARGET="${3:-aarch64-apple-darwin}"

# The `lirevo_pk_` ggml install-name disambiguation is applied by the host
# build.rs (which stages the renamed parakeet dylibs). We copy those renamed
# leaves into the bundle VERBATIM, so the prefix is preserved without this script
# re-applying it.
SIGN_ID="${APPLE_SIGNING_IDENTITY:--}"   # `-` == ad-hoc

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUILD_DIR="$ROOT/app/src-tauri/target/$TARGET/$PROFILE/build"

if [ ! -d "$APP" ]; then
  echo "error: app bundle not found: $APP" >&2
  exit 1
fi
if [ ! -d "$BUILD_DIR" ]; then
  echo "error: cargo build dir not found: $BUILD_DIR" >&2
  exit 1
fi

FRAMEWORKS="$APP/Contents/Frameworks"
BACKENDS="$APP/Contents/Resources/backends"
MACOS_DIR="$APP/Contents/MacOS"
mkdir -p "$FRAMEWORKS" "$BACKENDS/parakeet" "$BACKENDS/llama"

# --- locate the staged/build artifacts (hash-suffixed dirs) -------------------
# cargo can leave SEVERAL build-script output dirs for the same `links` crate
# (different feature/metadata units), and only ONE holds the runtime dylibs while
# the others hold static `.a`s. So we never pick "the first dir" — we locate a
# SENTINEL runtime FILE and take its parent. `-newest` by mtime in case of dupes.
newest_parent () {  # $1 = -path glob of a sentinel file
  find "$BUILD_DIR" -path "$1" -print0 2>/dev/null |
    xargs -0 stat -f '%m %N' 2>/dev/null | sort -rn | head -1 |
    cut -d' ' -f2- | xargs -I{} dirname {}
}
# parakeet: host build.rs stages renamed dylibs + modules into one flat dir.
PK_LIB="$(newest_parent '*/out/parakeet_engine/lib/libparakeet.dylib')"
# llama: libllama + bare-name ggml dylibs live next to libllama.0.dylib;
# the loadable modules live in the sibling out/backends.
LL_LIB="$(newest_parent '*/llama-cpp-sys-2-*/out/lib/libllama.0.dylib')"
LL_BACKENDS="$(newest_parent '*/llama-cpp-sys-2-*/out/backends/libggml-metal.so')"

if [ -z "$PK_LIB" ] || [ -z "$LL_LIB" ] || [ -z "$LL_BACKENDS" ]; then
  echo "error: could not locate engine artifacts under $BUILD_DIR" >&2
  echo "  parakeet lib:  ${PK_LIB:-<missing>}" >&2
  echo "  llama lib:     ${LL_LIB:-<missing>}" >&2
  echo "  llama backends:${LL_BACKENDS:-<missing>}" >&2
  exit 1
fi
echo "parakeet staged lib : $PK_LIB"
echo "llama lib           : $LL_LIB"
echo "llama backends      : $LL_BACKENDS"

# --- copy dylibs into Contents/Frameworks ------------------------------------
# Resolve symlinks (cp -L) and flatten to a single dir. Install names already
# use @rpath/...; the `lirevo_pk_` prefix on parakeet's ggml is preserved as-is.
copy_dylibs () {  # $1 = src dir
  local src="$1"
  find "$src" -maxdepth 1 -name '*.dylib' -print0 |
    while IFS= read -r -d '' f; do
      cp -Lf "$f" "$FRAMEWORKS/$(basename "$f")"
    done
}
copy_dylibs "$PK_LIB"
copy_dylibs "$LL_LIB"

# --- copy backend modules into Contents/Resources/backends/{parakeet,llama} ---
copy_modules () {  # $1 = src dir, $2 = dst subdir
  find "$1" -maxdepth 1 -name '*.so' -print0 |
    while IFS= read -r -d '' f; do
      cp -Lf "$f" "$2/$(basename "$f")"
    done
}
# parakeet modules live alongside its dylibs in the staged flat lib dir.
copy_modules "$PK_LIB"      "$BACKENDS/parakeet"
copy_modules "$LL_BACKENDS" "$BACKENDS/llama"

# --- main binary: replace dev rpaths with @loader_path/../Frameworks ---------
BIN="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$APP/Contents/Info.plist")"
BIN_PATH="$MACOS_DIR/$BIN"
echo "main binary         : $BIN_PATH"

# Delete every absolute (dev `target/`) rpath, keep nothing else to chance.
otool -l "$BIN_PATH" | awk '/LC_RPATH/{r=1} r&&/path /{print $2; r=0}' |
  while IFS= read -r rp; do
    case "$rp" in
      "$ROOT"/*|/Users/*|*/target/*)
        install_name_tool -delete_rpath "$rp" "$BIN_PATH" 2>/dev/null || true
        ;;
    esac
  done
# Add the one bundle-relative rpath (idempotent: ignore "would duplicate").
install_name_tool -add_rpath '@loader_path/../Frameworks' "$BIN_PATH" 2>/dev/null || true

# --- backend modules: add rpath to Frameworks so @rpath/libggml-base* resolves
# Modules sit at Contents/Resources/backends/<engine>/<mod>.so; Frameworks is at
# Contents/Frameworks => ../../../Frameworks from a module's @loader_path. A
# dlopen'd module resolves its @rpath against ITS OWN rpaths first, so wire each
# module directly rather than relying on the main exe's rpath inheritance.
add_module_rpath () {  # $1 = dir of modules
  find "$1" -maxdepth 1 -name '*.so' -print0 |
    while IFS= read -r -d '' m; do
      install_name_tool -add_rpath '@loader_path/../../../Frameworks' "$m" 2>/dev/null || true
    done
}
add_module_rpath "$BACKENDS/parakeet"
add_module_rpath "$BACKENDS/llama"

# --- re-sign everything (install_name_tool invalidates signatures) -----------
# Sign leaves first, then the app last so its seal covers the rewritten libs.
#
# Entitlements: apply entitlements.plist ONLY when signing with a real identity.
# The plist carries the `cs.disable-library-validation` / `cs.allow-jit` /
# `cs.allow-unsigned-executable-memory` "restricted" entitlements that hardened
# runtime needs to load the inference libs. But applying those to an AD-HOC
# (no-Team) binary makes AMFI refuse to spawn it ("Launchd job spawn failed" /
# SIGKILL). Tauri's own ad-hoc debug build ships with NO entitlements and runs
# fine (a non-hardened binary has library validation off by default), so the
# ad-hoc path signs plainly — matching that working baseline.
ENTITLEMENTS="$ROOT/app/src-tauri/entitlements.plist"
# Strip any prior signature before re-signing. Re-signing in place over a stale
# signature (e.g. when this script is re-run on an already-processed .app) can
# leave an inconsistent CodeDirectory that AMFI rejects at dlopen ("Invalid
# Page" SIGKILL), so we always start from an unsigned Mach-O. Keeps the step
# idempotent for Phase 3b / repeated release builds.
sign_lib () { codesign --remove-signature "$1" 2>/dev/null || true; codesign --force -s "$SIGN_ID" "$1"; }
if [ "$SIGN_ID" = "-" ]; then
  # Ad-hoc: no entitlements, no hardened runtime (matches Tauri's working debug
  # signature). Works for both `just dev-bundle` (no identity) and an ad-hoc
  # `just dmg`.
  sign_app () { codesign --force -s - "$1"; }
elif [ "$PROFILE" = "debug" ]; then
  # dev-bundle with a real identity: stable hash for persistent TCC, but WITHOUT
  # hardened runtime (it would block the cross-Team inference libs). Mirrors the
  # historical `codesign --force --deep -s "$IDENTITY"` re-sign.
  sign_app () { codesign --force -s "$SIGN_ID" "$1"; }
else
  # release dmg with a real identity: hardened runtime + entitlements
  # (cs.disable-library-validation / cs.allow-jit) so the signed, notarizable app
  # can still load + JIT the inference libs.
  sign_app () { codesign --force --options runtime --entitlements "$ENTITLEMENTS" -s "$SIGN_ID" "$1"; }
fi
find "$FRAMEWORKS" -name '*.dylib' -print0 | while IFS= read -r -d '' f; do sign_lib "$f"; done
find "$BACKENDS" -name '*.so' -print0 | while IFS= read -r -d '' f; do sign_lib "$f"; done
sign_lib "$BIN_PATH"
sign_app "$APP"

echo "bundle install complete: $APP"
