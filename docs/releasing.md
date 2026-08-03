# Release Packaging

The release workflow publishes a full Windows product, a Windows CLI-only
installer, full macOS editor bundles (arm64 and x64), macOS CLI archives,
and the VS Code extension.

## Updater signing

The updater public key is committed in
`editors/cfd-editor/src-tauri/tauri.conf.json`. The matching private key
must never be committed. Configure its complete contents as the repository
secret `TAURI_SIGNING_PRIVATE_KEY`:

```powershell
Get-Content -Raw "$HOME/.tauri/coflow-updater.key" |
  gh secret set TAURI_SIGNING_PRIVATE_KEY
```

Keep an offline backup of the private key. Existing editor installations cannot
accept future updates if this key is lost. The key is intentionally passwordless
so CI needs only one secret; access to that secret must remain restricted.

Tauri's bundled signing path prompts for an empty password, so the workflow
builds the NSIS installer first and then signs it explicitly:

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY = Get-Content -Raw "$HOME/.tauri/coflow-updater.key"
$args = @(
  "editors/cfd-editor/node_modules/@tauri-apps/cli/tauri.js",
  "signer", "sign", "--password", "", "PATH_TO_INSTALLER"
)
& node $args
```

This updater signature verifies package integrity. Windows Authenticode signing
is not part of the initial release setup, so Windows may still show an unknown
publisher warning.

## macOS codesign and notarization

macOS bundles are codesigned with a Developer ID Application certificate,
notarized through Apple's notary service, and stapled so Gatekeeper accepts a
freshly downloaded DMG. The standalone CLI archives are codesigned with the
Hardened Runtime and notarized with the same certificate, but a bare Mach-O
binary cannot carry a stapled ticket, so Gatekeeper verifies the CLI online on
first run (see `packaging/macos/sign-notarize-cli.sh`). The workflow reads the
certificate, its export password, the App Store Connect API key, and the
identity string from repository secrets. Provisioning steps live in
[releasing-macos-signing.md](./releasing-macos-signing.md).

## Published assets

The public release contains only:

- `coflow-tools-windows-x64-setup.exe` and its updater `.sig`;
- `coflow-cli-windows-x64-setup.exe`;
- `coflow-tools-macos-arm64.dmg` and `coflow-tools-macos-x64.dmg`
  (user-facing downloads);
- `coflow-tools-macos-arm64.app.tar.gz` and its updater `.sig`, plus the
  matching x64 pair (Tauri updater artifacts — Tauri's macOS updater expects a
  gzipped `.app` bundle, so the DMG is not the signed artifact);
- `coflow-cli-macos-arm64.tar.gz` and `coflow-cli-macos-x64.tar.gz`;
- `latest.json` (merged updater manifest covering every signed platform);
- the packaged VS Code extension.

Raw editor executables and duplicate portable Windows archives remain CI build
details and are not release assets.

## Installer behavior

Both Windows installers run `coflow skill install -g` after installation. Their
uninstallers run `coflow skill uninstall -g`; the full NSIS installer skips that
step during an updater-driven internal uninstall. Installing the full product
migrates an existing CLI-only installation. The CLI-only installer refuses to
install while the full product is present.

macOS bundles and CLI archives have no installer hook. After moving
`Coflow Tools.app` (from the DMG) to `/Applications` or extracting the CLI
tarball, users run `coflow skill install -g` themselves. The DMG is signed and
notarized, so Gatekeeper accepts a first-time launch without a warning.

The editor bundle (including its embedded CLI sidecar) auto-updates through the
Tauri updater. The standalone macOS CLI does not, so it ships `coflow
self-update`: it queries the GitHub `releases/latest` API, matches the
`coflow-cli-macos-<arch>` archive for the current platform, and replaces the
running binary in place. Use `coflow self-update --check` to only report
whether a newer release exists. The Windows CLI ships as an installer rather
than a plain archive, so it is updated by re-running that installer;
`self-update` reports the platform as unsupported there.

Release tags must match the root Cargo package version exactly (`vX.Y.Z`). Run
the full release gate from `AGENTS.md` before tagging.
