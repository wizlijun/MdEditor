#!/usr/bin/env bash
#
# Regenerate the macOS app icon (src-tauri/icons/icon.icns) from icon-source.png.
#
#   scripts/gen-macos-icon.sh
#
# Why this exists: macOS icons are NOT full-bleed. Apple's icon grid puts the
# rounded-square body in an 824×824 area centred inside a 1024×1024 canvas —
# the remaining 100 px margin is what makes every app in the Dock / ⌘-Tab
# switcher look the same size. icon-source.png is drawn full-bleed (correct for
# iOS/Android, where the system does the masking), so shipping it as-is made
# note.md render ~24 % larger than its neighbours.
#
# This script produces:
#   icon-source-macos.png      1024 canvas, 824 body, transparent margin
#   src-tauri/icons/icon.icns  the full 16…512@2x representation ladder
#
# tauri.conf.json lists icon.icns first in bundle.icon so the bundler ships it
# instead of deriving a single-resolution icns from icon.png.
set -euo pipefail

cd "$(dirname "$0")/.."

SRC=icon-source.png
PADDED=icon-source-macos.png
OUT=src-tauri/icons/icon.icns
BODY=824          # Apple macOS grid, in a 1024 canvas
STAGE=$(mktemp -d)/notemd.iconset

command -v python3 >/dev/null || { echo "python3 with Pillow required" >&2; exit 1; }

python3 - "$SRC" "$PADDED" "$BODY" "$STAGE" <<'PY'
import sys, os
from PIL import Image

src_path, padded_path, body, stage = sys.argv[1], sys.argv[2], int(sys.argv[3]), sys.argv[4]
src = Image.open(src_path).convert('RGBA')
if src.size != (1024, 1024):
    src = src.resize((1024, 1024), Image.LANCZOS)

canvas = Image.new('RGBA', (1024, 1024), (0, 0, 0, 0))
off = (1024 - body) // 2
canvas.paste(src.resize((body, body), Image.LANCZOS), (off, off))
canvas.save(padded_path)

os.makedirs(stage, exist_ok=True)
for px, name in [(16, '16x16'), (32, '16x16@2x'), (32, '32x32'), (64, '32x32@2x'),
                 (128, '128x128'), (256, '128x128@2x'), (256, '256x256'),
                 (512, '256x256@2x'), (512, '512x512'), (1024, '512x512@2x')]:
    canvas.resize((px, px), Image.LANCZOS).save(f'{stage}/icon_{name}.png')
PY

iconutil -c icns "$STAGE" -o "$OUT"
echo "wrote $PADDED and $OUT"
