# Coflow 0.6.3

## Editor Updates

- The Windows CFD Editor can now check for complete application updates from the lower-left sidebar, download them with progress feedback, and launch the signed installer.
- The full Windows installer includes both the editor and `coflow` CLI, configures `PATH`, and installs Coflow skills for supported agents.

## Native Skill Management

- Added `coflow skill install`, `coflow skill uninstall`, and `coflow skill status` commands, including JSON output for automation.
- Skills are embedded in the CLI, so installation does not require Node.js or `npx`.
- Project installation is the default. Use `-g` to install globally for common agents; uninstall removes only files tracked by Coflow.

## Release Packages

- Windows provides a full editor-and-CLI installer and a separate CLI-only installer. The installers detect and migrate incompatible legacy Coflow installations.
- macOS currently provides CLI-only archives for Apple Silicon and Intel.
- Release assets are limited to the installers, updater manifest and signature, macOS CLI archives, and VS Code extension.

## Compatibility

- Removed the built-in Lark spreadsheet provider and remote `url` sources. Projects must migrate
  those inputs to local Excel, CSV, or CFD sources before upgrading.
- Local source formats and the JSON, MessagePack, and C# output contracts remain unchanged.
