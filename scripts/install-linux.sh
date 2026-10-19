#!/bin/sh
# Installs OxyPad for the current user: binary, desktop entry and icons.
# The desktop file name matches the app id the window reports, so the
# launcher and taskbar show the OxyPad icon.
set -eu

cd "$(dirname "$0")/.."
PREFIX="${PREFIX:-$HOME/.local}"
APP_ID="org.oxyapps.oxypad"

cargo build --release -p oxypad
install -Dm755 target/release/oxypad "$PREFIX/bin/oxypad"
install -Dm644 "packaging/linux/$APP_ID.desktop" "$PREFIX/share/applications/$APP_ID.desktop"
for size in 16 24 32 48 64 128 256 512; do
    install -Dm644 "oxypad-icons/png/oxypad-$size.png" \
        "$PREFIX/share/icons/hicolor/${size}x${size}/apps/$APP_ID.png"
done
install -Dm644 oxypad-icons/oxypad.svg "$PREFIX/share/icons/hicolor/scalable/apps/$APP_ID.svg"

command -v update-desktop-database >/dev/null && update-desktop-database "$PREFIX/share/applications" || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q "$PREFIX/share/icons/hicolor" || true
echo "Installed OxyPad to $PREFIX"
