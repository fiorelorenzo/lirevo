#!/usr/bin/env bash
#
# Guard the single source of truth for the app version.
#
# The version lives in ONE place: `[package] version` in
# `app/src-tauri/Cargo.toml`. Everything else is derived from it:
#
#   Cargo.toml  --> env!("CARGO_PKG_VERSION") --> settings.app_version
#                                             --> Settings > About
#               --> tauri's Cargo.toml fallback --> CFBundleShortVersionString
#                                                --> Info.plist --> .dmg name
#               --> mirrored into app/package.json by `just release`
#
# `tauri.conf.json` deliberately has NO `version` key: when it is absent Tauri
# falls back to the crate version, which is what makes the bundle and the
# in-app "About" version the same value by construction rather than by
# discipline. Re-adding it silently re-opens that drift, so this script fails
# when the key comes back.
#
# `app/package.json`'s version is inert metadata (nothing reads it), but it is
# the first thing a contributor looks at, so it is kept as an exact mirror
# rather than left to rot — it sat at 0.1.0 while the app shipped 0.9.0.
#
# Usage:
#   scripts/check-versions.sh              # consistency only (run by `just lint`)
#   scripts/check-versions.sh --tag v1.2.3 # also assert the git tag matches
#
# Exits non-zero listing every mismatch found, not just the first.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cargo_toml="$repo_root/app/src-tauri/Cargo.toml"
cargo_lock="$repo_root/app/src-tauri/Cargo.lock"
package_json="$repo_root/app/package.json"
tauri_conf="$repo_root/app/src-tauri/tauri.conf.json"

tag=""
while [ $# -gt 0 ]; do
    case "$1" in
        --tag)
            tag="${2:-}"
            [ -n "$tag" ] || { echo "--tag needs a value" >&2; exit 2; }
            shift 2
            ;;
        *)
            echo "unknown argument: $1" >&2
            exit 2
            ;;
    esac
done

failed=0
fail() {
    printf 'version check FAILED: %s\n' "$1" >&2
    failed=1
}

# `[package] version` from the Tauri crate — the source of truth. Scoped to the
# [package] table so a dependency's `version = "..."` can never be picked up.
source_version="$(awk '
    /^\[package\]/ { in_package = 1; next }
    /^\[/          { in_package = 0 }
    in_package && /^version[[:space:]]*=/ {
        match($0, /"[^"]+"/)
        print substr($0, RSTART + 1, RLENGTH - 2)
        exit
    }
' "$cargo_toml")"

if [ -z "$source_version" ]; then
    echo "version check FAILED: no [package] version in $cargo_toml" >&2
    exit 1
fi

# Cargo.lock carries its own copy of the workspace member's version; a stale one
# means someone edited Cargo.toml without re-resolving, and the lock is what CI
# builds from with --frozen-lockfile-style flows.
lock_version="$(awk '
    /^name = "lirevo"$/ {
        getline
        match($0, /"[^"]+"/)
        print substr($0, RSTART + 1, RLENGTH - 2)
        exit
    }
' "$cargo_lock")"

package_version="$(node -p \
    "JSON.parse(require('fs').readFileSync(process.argv[1], 'utf8')).version ?? ''" \
    "$package_json")"

tauri_conf_version="$(node -p \
    "JSON.parse(require('fs').readFileSync(process.argv[1], 'utf8')).version ?? ''" \
    "$tauri_conf")"

[ "$lock_version" = "$source_version" ] || fail \
    "app/src-tauri/Cargo.lock has $lock_version, Cargo.toml has $source_version — run 'just release $source_version'"

[ "$package_version" = "$source_version" ] || fail \
    "app/package.json has $package_version, Cargo.toml has $source_version — run 'just release $source_version'"

[ -z "$tauri_conf_version" ] || fail \
    "app/src-tauri/tauri.conf.json defines version=$tauri_conf_version — remove the key so it falls back to Cargo.toml (see the header of this script)"

if [ -n "$tag" ]; then
    [ "${tag#v}" = "$source_version" ] || fail \
        "git tag $tag does not match Cargo.toml version $source_version — the release would be labelled wrong"
fi

if [ "$failed" -ne 0 ]; then
    exit 1
fi

echo "version check: $source_version (Cargo.toml, Cargo.lock, package.json${tag:+, tag $tag})"
