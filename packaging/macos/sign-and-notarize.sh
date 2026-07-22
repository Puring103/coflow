#!/usr/bin/env bash
#
# Codesign, notarize, and staple a Coflow Tools macOS bundle and its DMG.
#
# Usage:
#   sign-and-notarize.sh <app_bundle> <dmg>
#
# Required environment variables:
#   APPLE_SIGNING_IDENTITY   e.g. "Developer ID Application: RONGQIAN GAO (AWRX78M8WM)"
#   APPLE_API_KEY_PATH       path to the App Store Connect .p8 private key
#   APPLE_API_KEY_ID         Key ID (10-char)
#   APPLE_API_ISSUER_ID      Issuer UUID
#
# The script assumes the `.app` was produced by `tauri build --bundles app,dmg`
# with the Hardened Runtime entitlements at packaging/macos/entitlements.plist.
# Tauri already signs the bundle with APPLE_SIGNING_IDENTITY during build; this
# script performs the notarize + staple steps that Tauri does not automate.

set -euo pipefail

APP_BUNDLE="${1:?usage: sign-and-notarize.sh <app_bundle> <dmg>}"
DMG_PATH="${2:?usage: sign-and-notarize.sh <app_bundle> <dmg>}"

: "${APPLE_SIGNING_IDENTITY:?APPLE_SIGNING_IDENTITY must be set}"
: "${APPLE_API_KEY_PATH:?APPLE_API_KEY_PATH must be set}"
: "${APPLE_API_KEY_ID:?APPLE_API_KEY_ID must be set}"
: "${APPLE_API_ISSUER_ID:?APPLE_API_ISSUER_ID must be set}"

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
entitlements="$script_dir/entitlements.plist"

echo "==> Sign nested Mach-O binaries with Hardened Runtime"
# Sign every embedded binary first (sidecars, frameworks, helpers), then the
# outer .app. --force lets us re-sign anything Tauri already signed without
# entitlements, and --options runtime enables Hardened Runtime, which
# notarization requires.
while IFS= read -r -d '' item; do
  case "$item" in
    *.app|"$APP_BUNDLE") continue ;;
  esac
  /usr/bin/codesign --force --timestamp --options runtime \
    --entitlements "$entitlements" \
    --sign "$APPLE_SIGNING_IDENTITY" \
    "$item"
done < <(find "$APP_BUNDLE/Contents" -type f \( -perm -u+x -o -name '*.dylib' \) -print0)

echo "==> Sign outer .app"
/usr/bin/codesign --force --timestamp --options runtime \
  --entitlements "$entitlements" \
  --sign "$APPLE_SIGNING_IDENTITY" \
  "$APP_BUNDLE"

echo "==> Verify signature"
/usr/bin/codesign --verify --deep --strict --verbose=2 "$APP_BUNDLE"
# Do not pipe to `head`: `codesign` writes to stderr and SIGPIPE would fail
# the script under `set -euo pipefail`. Trim in Bash instead.
codesign_info="$(/usr/bin/codesign --display --verbose=4 "$APP_BUNDLE" 2>&1 || true)"
printf '%s\n' "$codesign_info" | awk 'NR<=20'

echo "==> Notarize .app"
# Zip the .app first because notarytool accepts .zip, .dmg, or .pkg.
notary_zip="$(dirname "$APP_BUNDLE")/$(basename "$APP_BUNDLE" .app)-notary.zip"
/usr/bin/ditto -c -k --keepParent "$APP_BUNDLE" "$notary_zip"

xcrun notarytool submit "$notary_zip" \
  --key "$APPLE_API_KEY_PATH" \
  --key-id "$APPLE_API_KEY_ID" \
  --issuer "$APPLE_API_ISSUER_ID" \
  --wait \
  --timeout 30m

rm -f "$notary_zip"

echo "==> Staple .app"
xcrun stapler staple "$APP_BUNDLE"
xcrun stapler validate "$APP_BUNDLE"

echo "==> Sign and notarize DMG"
# The DMG is produced after the .app is signed, but Tauri does not attach the
# hardened-runtime signature to the outer DMG. Re-sign it, then notarize+staple
# so Gatekeeper accepts a directly downloaded DMG.
/usr/bin/codesign --force --timestamp \
  --sign "$APPLE_SIGNING_IDENTITY" \
  "$DMG_PATH"

xcrun notarytool submit "$DMG_PATH" \
  --key "$APPLE_API_KEY_PATH" \
  --key-id "$APPLE_API_KEY_ID" \
  --issuer "$APPLE_API_ISSUER_ID" \
  --wait \
  --timeout 30m

xcrun stapler staple "$DMG_PATH"
xcrun stapler validate "$DMG_PATH"

echo ""
echo "Signed, notarized, and stapled:"
echo "  $APP_BUNDLE"
echo "  $DMG_PATH"
