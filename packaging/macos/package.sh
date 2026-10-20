#!/usr/bin/env bash
# Builds OxyPad.app for one Mac architecture, signs it and packages it as a DMG:
#
#   $DIST/OxyPad-<version>-macos-<arch>.dmg (+ .sha256)
#
# Usage: packaging/macos/package.sh [--arch apple-silicon|intel] [--skip-build] [--app-only]
#
#   --arch        apple-silicon (arm64) or intel (x86_64); default: this Mac's
#   --skip-build  package what cargo already built
#   --app-only    stop after the app: $CARGO_TARGET_DIR/<target>/release/bundle/OxyPad.app
#
# Signing and notarization (environment, all optional):
#   DEVELOPER_ID_APPLICATION  "Developer ID Application: Name (TEAMID)"; signs with the hardened
#                             runtime. Without it the app is signed ad hoc, and Gatekeeper asks
#                             before the first launch. In CI, import-cert.sh installs the identity.
#   APPLE_ID, APPLE_TEAM_ID, APPLE_APP_PASSWORD
#                             notarize and staple the DMG (needs DEVELOPER_ID_APPLICATION)
#   BUILD_NUMBER              CFBundleVersion (CI uses the run number); default: the version
#   NPM_REGISTRY              registry for create-dmg (default: the public npm registry)
set -euo pipefail
# shellcheck source=../env.sh
. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"
HERE="$ROOT/packaging/macos"
CREATE_DMG_VERSION="8.1.0"

case "$(uname -m)" in
  arm64) ARCH=apple-silicon ;;
  *) ARCH=intel ;;
esac
SKIP_BUILD=0
APP_ONLY=0
while [ $# -gt 0 ]; do
  case "$1" in
    --arch) ARCH="$2"; shift 2 ;;
    --skip-build) SKIP_BUILD=1; shift ;;
    --app-only) APP_ONLY=1; shift ;;
    -h | --help) sed -n '2,20p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
case "$ARCH" in
  apple-silicon) TARGET=aarch64-apple-darwin; MACHINE=arm64 ;;
  intel) TARGET=x86_64-apple-darwin; MACHINE=x86_64 ;;
  *) echo "unknown --arch $ARCH (apple-silicon or intel)" >&2; exit 2 ;;
esac

# Keep in sync with LSMinimumSystemVersion in Info.plist.in.
export MACOSX_DEPLOYMENT_TARGET=11.0
IDENTITY="${DEVELOPER_ID_APPLICATION:-}"
NOTARIZE=0
if [ -z "$IDENTITY" ]; then
  warn "macOS: no DEVELOPER_ID_APPLICATION, so the app is signed ad hoc and not notarized"
elif [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ] && [ -n "${APPLE_APP_PASSWORD:-}" ]; then
  NOTARIZE=1
else
  warn "macOS: APPLE_ID / APPLE_TEAM_ID / APPLE_APP_PASSWORD incomplete, so the DMG is signed but not notarized"
fi

echo "==> OxyPad $VERSION for macOS $ARCH ($TARGET), notarize: $NOTARIZE"

# ---- build ---------------------------------------------------------------------------------------
if [ "$SKIP_BUILD" = 0 ]; then
  rustup target add "$TARGET" >/dev/null 2>&1 || true
  (cd "$ROOT" && cargo build --release --locked -p oxypad --target "$TARGET")
fi
BINARY="$CARGO_TARGET_DIR/$TARGET/release/oxypad"
BUILT="$(lipo -archs "$BINARY")"
[ "$BUILT" = "$MACHINE" ] || { echo "error: $BINARY is $BUILT, expected $MACHINE" >&2; exit 1; }

# ---- OxyPad.app ----------------------------------------------------------------------------------
# Assembled and signed outside the project: Finder and iCloud-synced folders (Documents, Desktop)
# add extended attributes to new files, and codesign rejects a bundle that has them.
WORK="$(mktemp -d "${TMPDIR:-/tmp}/oxypad-package.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
APP="$WORK/OxyPad.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BINARY" "$APP/Contents/MacOS/oxypad"
# Made by scripts/make-macos-icons.sh, within Apple's icon grid.
cp -X "$ROOT/oxypad-icons/macos/oxypad-macos.icns" "$APP/Contents/Resources/OxyPad.icns"
sed -e "s/@VERSION@/$VERSION/g" -e "s/@BUILD@/${BUILD_NUMBER:-$VERSION}/g" \
  "$HERE/Info.plist.in" >"$APP/Contents/Info.plist"
plutil -lint "$APP/Contents/Info.plist"
printf 'APPL????' >"$APP/Contents/PkgInfo"
xattr -cr "$APP"

if [ -n "$IDENTITY" ]; then
  codesign --force --options runtime --timestamp --sign "$IDENTITY" "$APP"
else
  codesign --force --sign - "$APP"
fi
codesign --verify --strict --verbose=2 "$APP"

BUNDLE_DIR="$CARGO_TARGET_DIR/$TARGET/release/bundle"
rm -rf "$BUNDLE_DIR/OxyPad.app"
mkdir -p "$BUNDLE_DIR"
ditto --noextattr --norsrc "$APP" "$BUNDLE_DIR/OxyPad.app"
echo "==> $BUNDLE_DIR/OxyPad.app"
[ "$APP_ONLY" = 1 ] && exit 0

# ---- notarization --------------------------------------------------------------------------------
# `notarytool submit --wait` succeeds even when Apple rejects the file: check the status.
notarize() {
  local file="$1" out id status
  echo "==> notarizing $(basename "$file") (this can take a few minutes)"
  out="$(xcrun notarytool submit "$file" --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" \
    --password "$APPLE_APP_PASSWORD" --wait --timeout 1h --output-format json)" || true
  echo "$out"
  id="$(printf '%s' "$out" | plutil -extract id raw -o - - 2>/dev/null || true)"
  status="$(printf '%s' "$out" | plutil -extract status raw -o - - 2>/dev/null || true)"
  if [ "$status" != "Accepted" ]; then
    if [ -n "$id" ]; then
      xcrun notarytool log "$id" --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" \
        --password "$APPLE_APP_PASSWORD" || true
    fi
    echo "error: notarization of $(basename "$file") failed (status: ${status:-unknown})" >&2
    exit 1
  fi
}

# ---- DMG -----------------------------------------------------------------------------------------
echo "==> creating the DMG"
STAGE="$WORK/dmg"
mkdir -p "$STAGE"
# Run from the stage directory: create-dmg turns a license.txt in the current directory into a
# click-through agreement. It exits non-zero when it cannot sign the DMG, which is done below.
(cd "$STAGE" && npx --yes --registry "${NPM_REGISTRY:-https://registry.npmjs.org}" \
  "create-dmg@$CREATE_DMG_VERSION" --overwrite --dmg-title="OxyPad $VERSION" \
  --no-code-sign "$APP" "$STAGE") || true
CREATED="$(find "$STAGE" -maxdepth 1 -name '*.dmg' | head -1)"
[ -n "$CREATED" ] || { echo "error: create-dmg produced no DMG" >&2; exit 1; }
DMG="$DIST/OxyPad-$VERSION-macos-$ARCH.dmg"
mv -f "$CREATED" "$DMG"
if [ -n "$IDENTITY" ]; then
  codesign --sign "$IDENTITY" --timestamp "$DMG"
  codesign --verify --strict --verbose=2 "$DMG"
fi

# The app inside the finished image, and a copy taken out of it, must pass the same strict check
# as the app before packaging, so a signature on the image cannot hide a damaged app.
verify_dmg_app() {
  local mount="$WORK/dmg-check" copy="$WORK/dmg-check-copy" rc=0
  mkdir -p "$mount" "$copy"
  hdiutil attach -readonly -nobrowse -noautoopen -mountpoint "$mount" "$DMG" >/dev/null
  codesign --verify --deep --strict --verbose=2 "$mount/OxyPad.app" || rc=1
  if [ "$rc" = 0 ]; then
    ditto "$mount/OxyPad.app" "$copy/OxyPad.app" || rc=1
  fi
  if [ "$rc" = 0 ]; then
    codesign --verify --deep --strict --verbose=2 "$copy/OxyPad.app" || rc=1
    [ "$(lipo -archs "$copy/OxyPad.app/Contents/MacOS/oxypad")" = "$MACHINE" ] || rc=1
  fi
  hdiutil detach "$mount" >/dev/null || hdiutil detach "$mount" -force >/dev/null || rc=1
  return "$rc"
}
verify_dmg_app || { echo "error: OxyPad.app in $(basename "$DMG") fails verification" >&2; exit 1; }

if [ "$NOTARIZE" = 1 ]; then
  notarize "$DMG"
  xcrun stapler staple "$DMG"
  xcrun stapler validate "$DMG"
fi

write_sha256 "$DMG"
echo "==> $DMG"
cat "$DMG.sha256"
