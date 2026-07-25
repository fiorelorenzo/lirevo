#!/usr/bin/env bash
#
# Print the CHANGELOG.md section for one version, for use as the GitHub Release
# body.
#
# Every release before v0.9.1 shipped with an empty body: the changelog was
# written, then never propagated to the release page. This makes CHANGELOG.md
# the single place release notes are authored, and `release.yml` pipes this
# script's output into the release.
#
# A section is everything from `## [<version>]` up to the next `## [` heading.
# Missing, empty, or still-a-stub sections are a hard error — publishing notes
# that say "TODO" is worse than the empty body this replaces.
#
# Usage:
#   scripts/changelog-section.sh 0.9.1
#   scripts/changelog-section.sh v0.9.1   # a leading v is accepted and stripped
set -euo pipefail

version="${1:-}"
[ -n "$version" ] || { echo "usage: $0 <version>" >&2; exit 2; }
version="${version#v}"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
changelog="$repo_root/CHANGELOG.md"

section="$(awk -v version="$version" '
    $0 ~ "^## \\[" version "\\]" { printing = 1; print; next }
    printing && /^## \[/         { exit }
    printing                     { print }
' "$changelog")"

if [ -z "$section" ]; then
    echo "no '## [$version]' section in CHANGELOG.md — write the release notes before tagging" >&2
    exit 1
fi

# Strip the heading and blank lines: what remains is the actual notes.
body="$(printf '%s\n' "$section" | tail -n +2 | tr -d '[:space:]')"
if [ -z "$body" ]; then
    echo "the '## [$version]' section in CHANGELOG.md is empty" >&2
    exit 1
fi

if printf '%s\n' "$section" | grep -q 'TODO'; then
    echo "the '## [$version]' section in CHANGELOG.md still contains a TODO stub" >&2
    exit 1
fi

printf '%s\n' "$section"
