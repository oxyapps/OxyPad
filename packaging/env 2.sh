# shellcheck shell=bash
# Shared setup for the packaging scripts. Source it: `. "$(dirname "$0")/../env.sh"`.
#
# Exports:
#   ROOT              the repository root
#   VERSION           [workspace.package] version in Cargo.toml (override: OXYPAD_VERSION)
#   DIST              where release files go (default: $ROOT/dist)
#   CARGO_TARGET_DIR  cargo's target directory (default: $ROOT/target)

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export ROOT

# The version lives in one place: `[workspace.package] version` in Cargo.toml.
workspace_version() {
  awk '
    /^\[/ { in_package = ($0 == "[workspace.package]"); next }
    in_package && $1 == "version" { gsub(/[" ]/, "", $3); print $3; exit }
  ' "$ROOT/Cargo.toml"
}

VERSION="${OXYPAD_VERSION:-$(workspace_version)}"
if [ -z "$VERSION" ]; then
  echo "error: could not read [workspace.package] version from $ROOT/Cargo.toml" >&2
  exit 1
fi
export VERSION

DIST="${DIST:-$ROOT/dist}"
mkdir -p "$DIST"
DIST="$(cd "$DIST" && pwd)"
export DIST

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"

# A GitHub Actions warning, or plain stderr outside Actions.
warn() {
  if [ -n "${GITHUB_ACTIONS:-}" ]; then echo "::warning::$*"; else echo "warning: $*" >&2; fi
}

# Writes FILE.sha256 next to FILE, in `sha256sum -c` format.
write_sha256() {
  local file="$1" hash
  if command -v sha256sum >/dev/null; then
    hash="$(sha256sum "$file" | cut -d' ' -f1)"
  else
    hash="$(shasum -a 256 "$file" | cut -d' ' -f1)"
  fi
  printf '%s  %s\n' "$hash" "$(basename "$file")" >"$file.sha256"
}
