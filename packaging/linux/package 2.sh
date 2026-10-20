#!/usr/bin/env bash
# Builds and packages OxyPad for Linux (<arch> is x86_64 or aarch64), each with a .sha256:
#
#   $DIST/OxyPad-<version>-linux-<arch>.AppImage  any distribution with glibc >= the build host's
#   $DIST/OxyPad-<version>-linux-<arch>.deb       Debian, Ubuntu, Mint, Pop!_OS, …
#   $DIST/OxyPad-<version>-linux-<arch>.rpm       Fedora, openSUSE, RHEL, …
#   $DIST/OxyPad-<version>-linux-<arch>.tar.gz    the binary, desktop entry and icons (bin/, share/)
#
# Usage: packaging/linux/package.sh [--skip-build] [--formats "appimage deb rpm tar"]
#
# Needs: cargo; nfpm for deb/rpm (https://nfpm.goreleaser.com); appimagetool for the AppImage
# (downloaded into $CARGO_TARGET_DIR when missing). Build on an old distribution (CI: Ubuntu
# 22.04, glibc 2.35) so the binary runs on newer ones. Optional: desktop-file-validate and
# appstreamcli check the metadata.
#
# Environment: OXYPAD_MAINTAINER, the deb/rpm maintainer ("Name <email>").
set -euo pipefail
# shellcheck source=../env.sh
. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"
HERE="$ROOT/packaging/linux"
APP_ID=org.oxyapps.oxypad

SKIP_BUILD=0
FORMATS="appimage deb rpm tar"
while [ $# -gt 0 ]; do
  case "$1" in
    --skip-build) SKIP_BUILD=1; shift ;;
    --formats) FORMATS="$2"; shift 2 ;;
    -h | --help) sed -n '2,17p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

ARCH="$(uname -m)"
case "$ARCH" in
  x86_64) DEB_ARCH=amd64 ;;
  aarch64 | arm64) ARCH=aarch64; DEB_ARCH=arm64 ;;
  *) echo "unsupported architecture $ARCH" >&2; exit 2 ;;
esac
export OXYPAD_MAINTAINER="${OXYPAD_MAINTAINER:-OxyApps <noreply@oxyapps.org>}"
BASENAME="OxyPad-$VERSION-linux-$ARCH"
BUILD_DATE="$(date -u +%Y-%m-%d)"

echo "==> OxyPad $VERSION for Linux $ARCH ($FORMATS)"

if [ "$SKIP_BUILD" = 0 ]; then
  (cd "$ROOT" && cargo build --release --locked -p oxypad)
fi
WORK="$CARGO_TARGET_DIR/linux-package"
STAGE="$WORK/root"
rm -rf "$WORK"

# ---- stage a file tree shared by every format ---------------------------------------------------
install -Dm755 "$CARGO_TARGET_DIR/release/oxypad" "$STAGE/usr/bin/oxypad"
strip "$STAGE/usr/bin/oxypad" 2>/dev/null || true
install -Dm644 "$HERE/$APP_ID.desktop" "$STAGE/usr/share/applications/$APP_ID.desktop"
mkdir -p "$STAGE/usr/share/metainfo"
sed -e "s/@VERSION@/$VERSION/g" -e "s/@DATE@/$BUILD_DATE/g" \
  "$HERE/$APP_ID.metainfo.xml.in" >"$STAGE/usr/share/metainfo/$APP_ID.metainfo.xml"
for size in 16 24 32 48 64 128 256 512; do
  install -Dm644 "$ROOT/oxypad-icons/png/oxypad-$size.png" \
    "$STAGE/usr/share/icons/hicolor/${size}x${size}/apps/$APP_ID.png"
done
install -Dm644 "$ROOT/oxypad-icons/oxypad.svg" "$STAGE/usr/share/icons/hicolor/scalable/apps/$APP_ID.svg"
install -Dm644 "$ROOT/LICENSE" "$STAGE/usr/share/doc/oxypad/LICENSE"
install -Dm644 "$ROOT/README.md" "$STAGE/usr/share/doc/oxypad/README.md"
install -Dm644 "$ROOT/crates/syntax/vendor/lexilla/License.txt" "$STAGE/usr/share/doc/oxypad/LICENSE-Lexilla.txt"

if command -v desktop-file-validate >/dev/null; then
  desktop-file-validate "$STAGE/usr/share/applications/$APP_ID.desktop"
fi
if command -v appstreamcli >/dev/null; then
  appstreamcli validate --no-net --explain "$STAGE/usr/share/metainfo/$APP_ID.metainfo.xml"
fi

has() { case " $FORMATS " in *" $1 "*) return 0 ;; *) return 1 ;; esac; }

# ---- .tar.gz -------------------------------------------------------------------------------------
if has tar; then
  mkdir -p "$WORK/tar"
  cp -R "$STAGE/usr" "$WORK/tar/$BASENAME"
  tar -C "$WORK/tar" -czf "$DIST/$BASENAME.tar.gz" "$BASENAME"
  write_sha256 "$DIST/$BASENAME.tar.gz"
  echo "wrote $DIST/$BASENAME.tar.gz"
fi

# ---- .deb / .rpm ---------------------------------------------------------------------------------
if has deb || has rpm; then
  command -v nfpm >/dev/null || { echo "error: nfpm not found (https://nfpm.goreleaser.com/install/)" >&2; exit 1; }
  export NFPM_ARCH="$DEB_ARCH"
  # nfpm expands variables in fields like `version` and `arch`, but not in `contents[].src`.
  sed "s|\${STAGE}|$STAGE|g" "$HERE/nfpm.yaml" >"$WORK/nfpm.yaml"
  for format in deb rpm; do
    if has "$format"; then
      (cd "$ROOT" && nfpm package -f "$WORK/nfpm.yaml" -p "$format" -t "$DIST/$BASENAME.$format")
      write_sha256 "$DIST/$BASENAME.$format"
    fi
  done
fi

# ---- AppImage ------------------------------------------------------------------------------------
if has appimage; then
  APPDIR="$WORK/OxyPad.AppDir"
  cp -R "$STAGE" "$APPDIR"
  ln -s usr/bin/oxypad "$APPDIR/AppRun"
  cp "$HERE/$APP_ID.desktop" "$APPDIR/$APP_ID.desktop"
  cp "$ROOT/oxypad-icons/png/oxypad-256.png" "$APPDIR/$APP_ID.png"
  ln -s "$APP_ID.png" "$APPDIR/.DirIcon"

  TOOL="${APPIMAGETOOL:-$(command -v appimagetool || true)}"
  if [ -z "$TOOL" ]; then
    TOOL="$CARGO_TARGET_DIR/appimagetool-$ARCH.AppImage"
    if [ ! -x "$TOOL" ]; then
      curl -fsSL -o "$TOOL" "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-$ARCH.AppImage"
      chmod +x "$TOOL"
    fi
  fi
  OUT="$DIST/$BASENAME.AppImage"
  # Extract-and-run works without FUSE (containers, CI); the AppImage embeds the static runtime,
  # so users do not need libfuse2 either.
  ARCH="$ARCH" APPIMAGE_EXTRACT_AND_RUN=1 "$TOOL" --no-appstream "$APPDIR" "$OUT"
  write_sha256 "$OUT"
  echo "wrote $OUT"
fi

"$STAGE/usr/bin/oxypad" --version
echo "==> done"
ls -lh "$DIST"
