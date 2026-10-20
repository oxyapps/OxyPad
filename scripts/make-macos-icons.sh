#!/bin/sh
# Makes the macOS icons in oxypad-icons/macos/ from oxypad-icons/png/oxypad-1024.png.
# Run it after changing the artwork.
#
# The artwork fills its canvas, while macOS icons keep it within Apple's grid
# (824 of 1024 pixels, with a transparent margin), so OxyPad matches the size
# of other apps in the Dock and Launchpad.
#
#   Output: oxypad-icons/macos/oxypad-macos-1024.png
#           oxypad-icons/macos/oxypad-macos.icns
set -eu

cd "$(dirname "$0")/.."

SOURCE="oxypad-icons/png/oxypad-1024.png"
OUT="oxypad-icons/macos"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/oxypad-icons.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

mkdir -p "$OUT"
sips -Z 824 "$SOURCE" --out "$WORK/artwork.png" >/dev/null
sips -p 1024 1024 "$WORK/artwork.png" --out "$WORK/oxypad-macos-1024.png" >/dev/null

# Every size macOS asks for, including the 16 and 32 pt 1x variants.
ICONSET="$WORK/oxypad-macos.iconset"
mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
    sips -z "$size" "$size" "$WORK/oxypad-macos-1024.png" --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z "$double" "$double" "$WORK/oxypad-macos-1024.png" --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$WORK/oxypad-macos.icns"

# Without extended attributes (quarantine, Finder info), which codesign
# rejects once the icon is in the app.
cp -X "$WORK/oxypad-macos-1024.png" "$WORK/oxypad-macos.icns" "$OUT/"
echo "Made $OUT/oxypad-macos-1024.png and $OUT/oxypad-macos.icns"
