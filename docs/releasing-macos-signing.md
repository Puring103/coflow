# macOS Signing and Notarization

The macOS release pipeline codesigns Coflow Tools with a Developer ID
Application certificate, notarizes it through Apple's notary service, and
staples the notarization ticket so Gatekeeper accepts the DMG on a first-time
download.

This document lists the GitHub secrets the release workflow requires and how to
produce each one. Nothing here is committed to the repository; all sensitive
material must live in the encrypted repository secrets store.

## Required GitHub secrets

| Secret | Purpose |
| --- | --- |
| `APPLE_SIGNING_IDENTITY` | Full identity string used by `codesign --sign`, e.g. `Developer ID Application: RONGQIAN GAO (AWRX78M8WM)`. |
| `APPLE_CERTIFICATE_P12` | Base64-encoded PKCS#12 bundle exported from Keychain Access. Includes the Developer ID Application certificate and its private key. |
| `APPLE_CERTIFICATE_PASSWORD` | Password chosen when exporting the `.p12`. |
| `APPLE_KEYCHAIN_PASSWORD` | Password used for the temporary CI keychain that holds the certificate. Any strong random string works. |
| `APPLE_API_KEY_BASE64` | Base64-encoded App Store Connect API key (`AuthKey_XXXXXXXXXX.p8`). Used by `notarytool submit`. |
| `APPLE_API_KEY_ID` | The 10-character Key ID that appears next to the key in App Store Connect. |
| `APPLE_API_ISSUER_ID` | The issuer UUID shown on the API Keys page. |
| `TAURI_SIGNING_PRIVATE_KEY` | Existing updater signing key. Unrelated to Apple identities, kept from earlier releases. |

## Exporting the Developer ID certificate

The certificate and its private key must be exported together as a `.p12`.

1. Open **Keychain Access** on the Mac that already has the certificate.
2. Select the **login** keychain and find the entry
   `Developer ID Application: RONGQIAN GAO (AWRX78M8WM)` (drop-down expanded so
   the private key is included).
3. Right-click the entry → **Export "Developer ID Application: …"** →
   File Format **Personal Information Exchange (.p12)**.
4. Choose a strong export password. This becomes `APPLE_CERTIFICATE_PASSWORD`.
5. Base64-encode the exported file:

   ```bash
   base64 -i ~/Downloads/coflow-signing.p12 -o ~/Downloads/coflow-signing.p12.base64
   ```

6. Paste the base64 contents into the `APPLE_CERTIFICATE_P12` secret.

## Creating an App Store Connect API key

1. Sign in to <https://appstoreconnect.apple.com/access/api> with an account
   that has **Admin** or **Developer** access on the team.
2. Click **Generate API Key** (**Keys** tab).
3. Give the key access role **Developer** (the minimum notarize needs).
4. Download the `AuthKey_XXXXXXXXXX.p8` file. **Apple only allows this download
   once** — keep an offline backup.
5. Note the **Key ID** (10-character alphanumeric) and the **Issuer ID** (UUID
   shown at the top of the page).
6. Base64-encode the private key and store it in `APPLE_API_KEY_BASE64`:

   ```bash
   base64 -i ~/Downloads/AuthKey_XXXXXXXXXX.p8 \
     -o ~/Downloads/AuthKey_XXXXXXXXXX.p8.base64
   ```

7. Save `APPLE_API_KEY_ID` and `APPLE_API_ISSUER_ID` in the corresponding
   secrets.

## Setting the secrets

Once every value is prepared, populate the repository secrets. Any GitHub UI
works; the CLI form is:

```bash
gh secret set APPLE_SIGNING_IDENTITY --body "Developer ID Application: RONGQIAN GAO (AWRX78M8WM)"
gh secret set APPLE_CERTIFICATE_P12 < ~/Downloads/coflow-signing.p12.base64
gh secret set APPLE_CERTIFICATE_PASSWORD --body "the-p12-password"
gh secret set APPLE_KEYCHAIN_PASSWORD --body "a-strong-random-string"
gh secret set APPLE_API_KEY_BASE64 < ~/Downloads/AuthKey_XXXXXXXXXX.p8.base64
gh secret set APPLE_API_KEY_ID --body "XXXXXXXXXX"
gh secret set APPLE_API_ISSUER_ID --body "00000000-0000-0000-0000-000000000000"
```

## Testing locally

`packaging/macos/sign-and-notarize.sh` performs the same steps as the CI job.
With every environment variable set, it operates on an unsigned `.app` and
`.dmg` produced by `tauri build`:

```bash
export APPLE_SIGNING_IDENTITY="Developer ID Application: RONGQIAN GAO (AWRX78M8WM)"
export APPLE_API_KEY_PATH="$HOME/private-keys/AuthKey_XXXXXXXXXX.p8"
export APPLE_API_KEY_ID="XXXXXXXXXX"
export APPLE_API_ISSUER_ID="00000000-0000-0000-0000-000000000000"

packaging/macos/sign-and-notarize.sh \
  "target/aarch64-apple-darwin/release/bundle/macos/Coflow Tools.app" \
  "target/aarch64-apple-darwin/release/bundle/dmg/Coflow Tools_0.7.3_aarch64.dmg"
```

Verify the stapled result with:

```bash
spctl --assess --type execute --verbose "…/Coflow Tools.app"
spctl --assess --type open --context context:primary-signature --verbose "…/Coflow Tools_*.dmg"
xcrun stapler validate "…/Coflow Tools.app"
```

## Rotation and revocation

* API keys can be revoked from the App Store Connect **Keys** page. Rotate
  `APPLE_API_KEY_BASE64` and `APPLE_API_KEY_ID` if the current one is
  compromised.
* Certificates expire (typically five years). When the Developer ID Application
  certificate is renewed, re-export the `.p12` and update both
  `APPLE_CERTIFICATE_P12` and `APPLE_SIGNING_IDENTITY` (the identity string
  contains the new certificate's serial fragment when regenerated).
