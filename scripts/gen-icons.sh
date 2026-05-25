#!/usr/bin/env bash
#
# Regenerate Lirevo app icons (PNG variants + .icns + tray template).
#
# Source: app/src-tauri/icons/icon.svg (master) + tray-template.svg.
# Output: PNG variants + icon.icns alongside.
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
readonly TRAY_MASTER="tray-template.svg"

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

echo "==> Generating tray template icons from $TRAY_MASTER"

# Tray icons are state-driven. For M3+ the .rs code references:
# - tray-ready.png      (default state)
# - tray-loading.png    (model loading)
# - tray-recording-1.png + tray-recording-2.png (pulse animation)
# - tray-error.png      (error state)
#
# All are rendered from the same two-dot template — variants differ via
# opacity / motion overlay added inline below.

mkdir -p tray

# Base template: just the two-dot mark in template-image style (black, will
# be auto-colored by macOS per light/dark mode)
rsvg-convert -w 44 -h 44 "$TRAY_MASTER" -o tray/tray-ready.png

# Loading state: dots overlapping (will be replaced with pulse animation later)
cat > /tmp/lirevo-tray-loading.svg <<'EOF'
<svg viewBox="0 0 44 44" xmlns="http://www.w3.org/2000/svg">
  <circle cx="22" cy="22" r="6" fill="#000000" opacity="0.5"/>
  <circle cx="22" cy="22" r="6" fill="#000000" opacity="0.5"/>
</svg>
EOF
rsvg-convert -w 44 -h 44 /tmp/lirevo-tray-loading.svg -o tray/tray-loading.png

# Recording frame 1: left dot active, right dot subdued
cat > /tmp/lirevo-tray-rec1.svg <<'EOF'
<svg viewBox="0 0 44 44" xmlns="http://www.w3.org/2000/svg">
  <circle cx="14" cy="22" r="7" fill="#000000"/>
  <circle cx="30" cy="22" r="5" fill="#000000" opacity="0.3"/>
</svg>
EOF
rsvg-convert -w 44 -h 44 /tmp/lirevo-tray-rec1.svg -o tray/tray-recording-1.png

# Recording frame 2: dots swapped (creates pulse effect when alternated)
cat > /tmp/lirevo-tray-rec2.svg <<'EOF'
<svg viewBox="0 0 44 44" xmlns="http://www.w3.org/2000/svg">
  <circle cx="14" cy="22" r="5" fill="#000000" opacity="0.3"/>
  <circle cx="30" cy="22" r="7" fill="#000000"/>
</svg>
EOF
rsvg-convert -w 44 -h 44 /tmp/lirevo-tray-rec2.svg -o tray/tray-recording-2.png

# Error: single dot only (visual signal of broken state)
cat > /tmp/lirevo-tray-error.svg <<'EOF'
<svg viewBox="0 0 44 44" xmlns="http://www.w3.org/2000/svg">
  <circle cx="22" cy="22" r="6" fill="#000000"/>
</svg>
EOF
rsvg-convert -w 44 -h 44 /tmp/lirevo-tray-error.svg -o tray/tray-error.png

rm -f /tmp/lirevo-tray-*.svg

echo "==> Done. Generated icons:"
ls -la *.png *.icns *.ico tray/*.png 2>/dev/null | awk '{print "  " $NF}'
