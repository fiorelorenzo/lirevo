#!/usr/bin/env bash
#
# Regenerate Lirevo app icons (PNG variants + .icns) and the tray icons.
#
# Source of truth: app/src-tauri/icons/icon.svg (the brand waveform on a dark
# squircle). The tray icons are generated inline below from the same waveform
# mark, as monochrome macOS template images.
#
# Requirements:
# - rsvg-convert (brew install librsvg)
# - sips (built-in on macOS)
# - iconutil (built-in on macOS)
#
# Re-run after editing icon.svg.

set -euo pipefail

cd "$(dirname "$0")/../app/src-tauri/icons"

readonly MASTER="icon.svg"

if [[ ! -f "$MASTER" ]]; then
  echo "error: $MASTER not found" >&2
  exit 1
fi

echo "==> Generating standard app icons from $MASTER"

# PNG variants — Tauri expects these specific filenames in tauri.conf.json
rsvg-convert -w 32   -h 32   "$MASTER" -o 32x32.png
rsvg-convert -w 128  -h 128  "$MASTER" -o 128x128.png
rsvg-convert -w 256  -h 256  "$MASTER" -o 128x128@2x.png
rsvg-convert -w 1024 -h 1024 "$MASTER" -o icon.png

# Windows-style logos for cross-platform packaging (kept for v2)
for size in 30 44 71 89 107 142 150 284 310; do
  rsvg-convert -w $size -h $size "$MASTER" -o "Square${size}x${size}Logo.png"
done
rsvg-convert -w 50 -h 50 "$MASTER" -o StoreLogo.png

echo "==> Generating .icns from $MASTER"

# Apple iconset → .icns pipeline
tmp_iconset="$(mktemp -d)/icon.iconset"
mkdir -p "$tmp_iconset"

# macOS .icns expects these specific sizes
rsvg-convert -w 16   -h 16   "$MASTER" -o "$tmp_iconset/icon_16x16.png"
rsvg-convert -w 32   -h 32   "$MASTER" -o "$tmp_iconset/icon_16x16@2x.png"
rsvg-convert -w 32   -h 32   "$MASTER" -o "$tmp_iconset/icon_32x32.png"
rsvg-convert -w 64   -h 64   "$MASTER" -o "$tmp_iconset/icon_32x32@2x.png"
rsvg-convert -w 128  -h 128  "$MASTER" -o "$tmp_iconset/icon_128x128.png"
rsvg-convert -w 256  -h 256  "$MASTER" -o "$tmp_iconset/icon_128x128@2x.png"
rsvg-convert -w 256  -h 256  "$MASTER" -o "$tmp_iconset/icon_256x256.png"
rsvg-convert -w 512  -h 512  "$MASTER" -o "$tmp_iconset/icon_256x256@2x.png"
rsvg-convert -w 512  -h 512  "$MASTER" -o "$tmp_iconset/icon_512x512.png"
rsvg-convert -w 1024 -h 1024 "$MASTER" -o "$tmp_iconset/icon_512x512@2x.png"

iconutil -c icns -o icon.icns "$tmp_iconset"
rm -rf "$(dirname "$tmp_iconset")"

# Windows .ico (basic, single 256px frame — Tauri Windows packaging refines if needed)
sips -s format ico icon.png --out icon.ico 2>/dev/null || true

echo "==> Generating tray template icons (waveform mark)"

# Tray icons are state-driven monochrome template images (black + alpha; macOS
# auto-tints per light/dark menu bar). Geometry: 5 pill bars in a 36x36 box,
# centerline y=18, bar width 4, rx 2, centers x = 4,11,18,25,32. A bar with
# half-height hh is x=cx-2 y=18-hh w=4 h=2*hh. The READY icon's amplitude
# encodes the active energy profile; recording/loading/error use dedicated
# treatments (see app/src-tauri/src/tray.rs). Rendered at 44x44 to match the
# previous assets' size.
mkdir -p tray

# $1 = output filename, $2 = inner SVG body
tray_png() {
  printf '%s' "<svg viewBox=\"0 0 36 36\" xmlns=\"http://www.w3.org/2000/svg\">$2</svg>" \
    | rsvg-convert -w 44 -h 44 -o "tray/$1"
}

# Five pill bars from a space-separated list of "cx:halfHeight" pairs.
bars() {
  local out="" pair cx hh y h
  for pair in "$@"; do
    cx="${pair%%:*}"; hh="${pair##*:}"
    y="$(echo "18 - $hh" | bc -l)"
    h="$(echo "2 * $hh" | bc -l)"
    out+="<rect x=\"$((cx - 2))\" y=\"$y\" width=\"4\" height=\"$h\" rx=\"2\" fill=\"#000\"/>"
  done
  printf '%s' "$out"
}

# Ready: amplitude = energy profile.
tray_png tray-ready-power_saver.png "<g>$(bars 4:2 11:3.5 18:5 25:3.5 32:2)</g>"
tray_png tray-ready-balanced.png    "<g>$(bars 4:2.5 11:6 18:9 25:6 32:2.5)</g>"
tray_png tray-ready-performance.png "<g>$(bars 4:5 11:9 18:13 25:9 32:5)</g>"

# Recording: a lively two-frame "dancing" waveform (profile-independent).
tray_png tray-recording-1.png "<g>$(bars 4:9 11:4 18:13 25:6 32:10)</g>"
tray_png tray-recording-2.png "<g>$(bars 4:5 11:11 18:7 25:13 32:4)</g>"

# Loading: a 6-frame waveform "wave" the tray cycles, matching the in-app
# logo's loading animation (bars rippling like an audio meter).
tray_png tray-loading-1.png "<g>$(bars 4:4.5 11:8.3 18:6.9 25:2.1 32:2)</g>"
tray_png tray-loading-2.png "<g>$(bars 4:8 11:7.5 18:2.9 25:2 32:3.7)</g>"
tray_png tray-loading-3.png "<g>$(bars 4:8 11:3.7 18:2 25:2.9 32:7.5)</g>"
tray_png tray-loading-4.png "<g>$(bars 4:4.5 11:2 18:2.1 25:6.9 32:8.3)</g>"
tray_png tray-loading-5.png "<g>$(bars 4:2 11:2 18:6.1 25:8.5 32:5.3)</g>"
tray_png tray-loading-6.png "<g>$(bars 4:2 11:5.3 18:8.5 25:6.1 32:2)</g>"

# Error: a clean exclamation glyph.
tray_png tray-error.png "<rect x=\"16\" y=\"6\" width=\"4\" height=\"15\" rx=\"2\" fill=\"#000\"/><circle cx=\"18\" cy=\"26\" r=\"2.4\" fill=\"#000\"/>"

# Attention: shown when Accessibility/Microphone permission is missing — the
# Balanced waveform plus a dot badge at the top-right corner.
tray_png tray-attention.png "<g>$(bars 4:2.5 11:6 18:9 25:6 32:2.5)</g><circle cx=\"31\" cy=\"7\" r=\"4\" fill=\"#000\"/>"

echo "==> Done. Generated icons:"
ls -la *.png *.icns *.ico tray/*.png 2>/dev/null | awk '{print "  " $NF}'
