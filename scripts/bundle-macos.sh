#!/bin/sh
# Builds OxyPad.app.
#
#   scripts/bundle-macos.sh [TARGET]
#
#   TARGET  a Rust target such as aarch64-apple-darwin or x86_64-apple-darwin;
#           the current Mac's architecture when omitted
#
#   Output: target/release/bundle/OxyPad.app, or target/TARGET/release/bundle/OxyPad.app
#
# The bundle gives the app its icon, its name in the menu bar and Dock, and
# its bundle identifier.
#
# Optional environment:
#   DEVELOPER_ID_APPLICATION  e.g. "Developer ID Application: Name (TEAMID)": signs with
#                             the hardened runtime, as notarization requires; without it
#                             the app is signed ad hoc and Gatekeeper warns on first launch
#   BUILD_NUMBER              CFBundleVersion (CI uses the run number); defaults to the version
set -eu

cd "$(dirname "$0")/.."

APP_NAME="OxyPad"
BUNDLE_ID="org.oxyapps.oxypad"
BINARY="oxypad"
ICON_SOURCE="oxypad-icons/macos/oxypad-macos-1024.png"
TARGET="${1:-}"

if [ -n "$TARGET" ]; then
    cargo build --release -p oxypad --target "$TARGET"
    OUT="target/$TARGET/release"
else
    cargo build --release -p oxypad
    OUT="target/release"
fi

VERSION=$(cargo pkgid -p oxypad | sed 's/.*[#@]//')
BUILD="${BUILD_NUMBER:-$VERSION}"
BUNDLE="$OUT/bundle/$APP_NAME.app"
rm -rf "$BUNDLE"
mkdir -p "$BUNDLE/Contents/MacOS" "$BUNDLE/Contents/Resources"
cp "$OUT/$BINARY" "$BUNDLE/Contents/MacOS/$BINARY"

# Every size macOS asks for, including the 16 and 32 pt 1x variants.
ICONSET="$OUT/bundle/$APP_NAME.iconset"
rm -rf "$ICONSET"
mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
    sips -z "$size" "$size" "$ICON_SOURCE" --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z "$double" "$double" "$ICON_SOURCE" --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$BUNDLE/Contents/Resources/$APP_NAME.icns"
rm -rf "$ICONSET"

cat > "$BUNDLE/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>$APP_NAME</string>
    <key>CFBundleDisplayName</key>
    <string>$APP_NAME</string>
    <key>CFBundleIdentifier</key>
    <string>$BUNDLE_ID</string>
    <key>CFBundleExecutable</key>
    <string>$BINARY</string>
    <key>CFBundleIconFile</key>
    <string>$APP_NAME</string>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <!-- The interface languages (crates/app/locales), so the system's own
         panels (open, save) match them. -->
    <key>CFBundleLocalizations</key>
    <array>
        <string>en</string>
        <string>pt-BR</string>
    </array>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>$VERSION</string>
    <key>CFBundleVersion</key>
    <string>$BUILD</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>LSApplicationCategoryType</key>
    <string>public.app-category.developer-tools</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
EOF

if [ -n "${DEVELOPER_ID_APPLICATION:-}" ]; then
    codesign --force --options runtime --timestamp \
        --entitlements packaging/macos/OxyPad.entitlements \
        --sign "$DEVELOPER_ID_APPLICATION" "$BUNDLE"
else
    codesign --force --sign - "$BUNDLE"
fi
codesign --verify --strict "$BUNDLE"
echo "Built $BUNDLE ($VERSION, $(lipo -archs "$BUNDLE/Contents/MacOS/$BINARY"))"
