#!/bin/sh
# Make the icon files of the app from assets/logo.svg: an .icns file for the
# macOS bundle and an .ico file for the Windows program.
#
# Run it after a change to the logo, then commit the two files. The build
# does not run it, so a person who builds Onyx needs none of these tools.
#
# It needs rsvg-convert and python3, and iconutil, which only macOS has.
set -eu

cd "$(dirname "$0")/.."
svg=assets/logo.svg
out=assets/icons
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$out"

for size in 16 24 32 48 64 128 256 512 1024; do
    rsvg-convert -w "$size" -h "$size" "$svg" -o "$work/$size.png"
done

# Windows: an .ico file that holds PNG images, which Windows reads since
# Vista.
python3 - "$work" "$out/onyx.ico" <<'PY'
import struct, sys
work, target = sys.argv[1], sys.argv[2]
sizes = [16, 24, 32, 48, 64, 128, 256]
images = [open(f"{work}/{s}.png", "rb").read() for s in sizes]
header = struct.pack("<HHH", 0, 1, len(sizes))
offset = 6 + 16 * len(sizes)
entries = b""
for size, data in zip(sizes, images):
    side = 0 if size == 256 else size  # 0 means 256 in an .ico entry.
    entries += struct.pack("<BBBBHHII", side, side, 0, 0, 1, 32, len(data), offset)
    offset += len(data)
with open(target, "wb") as f:
    f.write(header + entries + b"".join(images))
PY

# macOS: an .icns file from an iconset folder.
set_dir="$work/onyx.iconset"
mkdir -p "$set_dir"
for size in 16 32 128 256 512; do
    cp "$work/$size.png" "$set_dir/icon_${size}x${size}.png"
    cp "$work/$((size * 2)).png" "$set_dir/icon_${size}x${size}@2x.png"
done
iconutil -c icns "$set_dir" -o "$out/onyx.icns"

echo "Wrote $out/onyx.icns and $out/onyx.ico."
