#!/usr/bin/env bash
#
# Codesign and notarize the standalone Coflow CLI binary.
#
# Usage:
#   sign-notarize-cli.sh <cli_binary>
#
# Required environment variables:
#   APPLE_SIGNING_IDENTITY   e.g. "Developer ID Application: RONGQIAN GAO (AWRX78M8WM)"
#   APPLE_API_KEY_PATH       path to the App Store Connect .p8 private key
#   APPLE_API_KEY_ID         Key ID (10-char)
#   APPLE_API_ISSUER_ID      Issuer UUID
#
# A bare Mach-O binary cannot carry a stapled notarization ticket (stapler only
# supports .app/.dmg/.pkg). We still sign it with the Hardened Runtime and submit
# it for notarization so Gatekeeper accepts it after an online check on first run.
# The binary is shipped as a plain .tar.gz, exactly as `coflow self-update`
# expects it.

set -euo pipefail

CLI_BINARY="${1:?usage: sign-notarize-cli.sh <cli_binary>}"

: "${APPLE_SIGNING_IDENTITY:?APPLE_SIGNING_IDENTITY must be set}"
: "${APPLE_API_KEY_PATH:?APPLE_API_KEY_PATH must be set}"
: "${APPLE_API_KEY_ID:?APPLE_API_KEY_ID must be set}"
: "${APPLE_API_ISSUER_ID:?APPLE_API_ISSUER_ID must be set}"

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
entitlements="$script_dir/cli-entitlements.plist"

echo "==> Sign CLI binary with Hardened Runtime"
/usr/bin/codesign --force --timestamp --options runtime \
  --entitlements "$entitlements" \
  --sign "$APPLE_SIGNING_IDENTITY" \
  "$CLI_BINARY"

echo "==> Verify signature"
/usr/bin/codesign --verify --strict --verbose=2 "$CLI_BINARY"

echo "==> Notarize CLI binary"
# notarytool accepts .zip, .dmg, or .pkg, so zip the bare binary first.
notary_zip="$(dirname "$CLI_BINARY")/$(basename "$CLI_BINARY")-notary.zip"
/usr/bin/ditto -c -k "$CLI_BINARY" "$notary_zip"

xcrun notarytool submit "$notary_zip" \
  --key "$APPLE_API_KEY_PATH" \
  --key-id "$APPLE_API_KEY_ID" \
  --issuer "$APPLE_API_ISSUER_ID" \
  --wait \
  --timeout 30m

rm -f "$notary_zip"

# A bare binary cannot be stapled; Gatekeeper verifies notarization online.
echo ""
echo "Signed and notarized (no staple; online verification):"
echo "  $CLI_BINARY"
