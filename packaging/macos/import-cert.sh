#!/usr/bin/env bash
# Imports the Developer ID certificate into a temporary keychain, for CI.
#
#   P12=<base64 of the .p12> P12_PASSWORD=<its password> packaging/macos/import-cert.sh
#
# The keychain lives in $RUNNER_TEMP (or $TMPDIR) and is added to the search
# list, so codesign finds the identity; package.sh signs with it through
# DEVELOPER_ID_APPLICATION.
set -euo pipefail

: "${P12:?base64 of the Developer ID .p12}"
: "${P12_PASSWORD:?password of the .p12}"
TEMP="${RUNNER_TEMP:-${TMPDIR:-/tmp}}"
KEYCHAIN="$TEMP/signing.keychain-db"
KEYCHAIN_PASSWORD="$(uuidgen)"

security create-keychain -p "$KEYCHAIN_PASSWORD" "$KEYCHAIN"
security set-keychain-settings -lut 3600 "$KEYCHAIN"
security unlock-keychain -p "$KEYCHAIN_PASSWORD" "$KEYCHAIN"
echo "$P12" | base64 --decode >"$TEMP/certificate.p12"
security import "$TEMP/certificate.p12" -k "$KEYCHAIN" -P "$P12_PASSWORD" -T /usr/bin/codesign
rm "$TEMP/certificate.p12"
security set-key-partition-list -S apple-tool:,apple: -s -k "$KEYCHAIN_PASSWORD" "$KEYCHAIN" >/dev/null
# shellcheck disable=SC2046 # one argument per existing keychain
security list-keychains -d user -s "$KEYCHAIN" $(security list-keychains -d user | tr -d '"')
echo "imported into $KEYCHAIN"
