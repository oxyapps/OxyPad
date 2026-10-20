#!/bin/bash
# Builds OxyPad for one Mac architecture and packages it as a DMG in dist/.
#
#   scripts/build-dmg.sh [apple-silicon|intel]
#
#   apple-silicon  arm64, for Macs with M-series chips (default)
#   intel          x86_64, for Intel Macs
#
#   Output: dist/OxyPad-<version>-macos-<variant>.dmg (+ .sha256)
#
# Optional environment:
#   DEVELOPER_ID_APPLICATION     e.g. "Developer ID Application: Name (TEAMID)"; without it the
#                                app is signed ad hoc and Gatekeeper warns on first launch
#   APPLE_ID, APPLE_TEAM_ID, APPLE_APP_PASSWORD
#                                notarize and staple the DMG (requires DEVELOPER_ID_APPLICATION)
#   BUILD_NUMBER                 CFBundleVersion (CI uses the run number)
#   NPM_REGISTRY                 registry for create-dmg (default: the public npm registry)
set -euo pipefail

cd "$(dirname "$0")/.."
ROOT="$(pwd)"
CREATE_DMG_VERSION="8.1.0"

VARIANT="${1:-apple-silicon}"
case "$VARIANT" in
    apple-silicon) TARGET="aarch64-apple-darwin" ARCH="arm64" ;;
    intel) TARGET="x86_64-apple-darwin" ARCH="x86_64" ;;
    *) echo "usage: $0 [apple-silicon|intel]" >&2; exit 2 ;;
esac

VERSION="$(cargo pkgid -p oxypad | sed 's/.*[#@]//')"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/oxypad-release.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

echo "==> OxyPad $VERSION ($VARIANT, $TARGET)"
rustup target add "$TARGET" >/dev/null 2>&1 || true
scripts/bundle-macos.sh "$TARGET"
APP="target/$TARGET/release/bundle/OxyPad.app"
BUILT_ARCHS="$(lipo -archs "$APP/Contents/MacOS/oxypad")"
[[ "$BUILT_ARCHS" == "$ARCH" ]] || { echo "error: built $BUILT_ARCHS, expected $ARCH" >&2; exit 1; }
ditto --noextattr --norsrc "$APP" "$WORK/OxyPad.app"

echo "==> Creating DMG"
mkdir -p "$ROOT/dist"
# Run from the work directory: create-dmg turns a license.txt in the
# current directory into a click-through agreement. It exits non-zero when
# it cannot sign the DMG, which is done below instead.
(cd "$WORK" && npx --yes --registry "${NPM_REGISTRY:-https://registry.npmjs.org}" \
    "create-dmg@$CREATE_DMG_VERSION" --overwrite --dmg-title="OxyPad $VERSION" \
    --no-code-sign "$WORK/OxyPad.app" "$WORK") || true
CREATED="$(ls "$WORK"/*.dmg 2>/dev/null | head -1)"
[[ -n "$CREATED" ]] || { echo "error: create-dmg produced no DMG" >&2; exit 1; }

DMG="$ROOT/dist/OxyPad-$VERSION-macos-$VARIANT.dmg"
mv -f "$CREATED" "$DMG"
if [[ -n "${DEVELOPER_ID_APPLICATION:-}" ]]; then
    codesign --sign "$DEVELOPER_ID_APPLICATION" --timestamp "$DMG"
fi

if [[ -n "${DEVELOPER_ID_APPLICATION:-}" && -n "${APPLE_ID:-}" && -n "${APPLE_APP_PASSWORD:-}" ]]; then
    echo "==> Notarizing"
    xcrun notarytool submit "$DMG" --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" \
        --password "$APPLE_APP_PASSWORD" --wait
    xcrun stapler staple "$DMG"
fi

(cd "$ROOT/dist" && shasum -a 256 "$(basename "$DMG")" > "$(basename "$DMG").sha256")
echo "==> $DMG"
cat "$DMG.sha256"
