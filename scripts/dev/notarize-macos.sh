#!/usr/bin/env bash
# Sign a macOS binary with a Developer ID certificate and notarise it (ADR-0005). Runs in the
# release workflow only when the Apple secrets exist; locally it needs the same six variables.
# A bare Mach-O cannot be stapled; Gatekeeper checks the notarisation ticket online.
set -euo pipefail
bin="${1:?usage: notarize-macos.sh <binary>}"
: "${APPLE_CERTIFICATE_P12:?}" "${APPLE_CERTIFICATE_PASSWORD:?}" "${APPLE_SIGNING_IDENTITY:?}"
: "${APPLE_ID:?}" "${APPLE_TEAM_ID:?}" "${APPLE_APP_PASSWORD:?}"

keychain="$RUNNER_TEMP/signing.keychain-db"
keychain_pw="$(openssl rand -hex 16)"
cert="$RUNNER_TEMP/cert.p12"
printf '%s' "$APPLE_CERTIFICATE_P12" | base64 --decode > "$cert"
security create-keychain -p "$keychain_pw" "$keychain"
security set-keychain-settings -lut 21600 "$keychain"
security unlock-keychain -p "$keychain_pw" "$keychain"
security import "$cert" -P "$APPLE_CERTIFICATE_PASSWORD" -A -t cert -f pkcs12 -k "$keychain"
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$keychain_pw" "$keychain"
security list-keychain -d user -s "$keychain"

codesign --force --options runtime --timestamp --sign "$APPLE_SIGNING_IDENTITY" "$bin"
codesign --verify --strict --verbose=2 "$bin"

zip -j "$RUNNER_TEMP/notarize.zip" "$bin"
xcrun notarytool submit "$RUNNER_TEMP/notarize.zip" \
  --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" --password "$APPLE_APP_PASSWORD" --wait
rm -f "$cert" "$RUNNER_TEMP/notarize.zip"
security delete-keychain "$keychain"
echo "signed and notarised $bin"
