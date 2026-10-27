# Editor guide

Install the [full package](./install.md), launch the Coflow editor, and create a project or open an existing `coflow.yaml`. A project contains CFT types and CFD data; see [Getting started](./install.md) for a complete example.

## Edit and validate

Select CFT files to edit type definitions and CFD files to view records. Tables help compare fields across records, graphs show references, and source views allow direct CFT/CFD editing. Field values must satisfy their types. Diagnostics locate missing fields, invalid references, and failed rules in source.

Save source changes and review diagnostics before continuing. Data editing supports undo and redo. External CLI or AI changes refresh the project. Resolve diagnostics before treating the project as complete, even when some records remain visible.

## Dimensions, comparison, and extensions

Select a dimension to inspect base values and overrides. Edit explicit overrides as needed; other fields continue to fall back to base values. See [Localization and dimensions](../reference/10-localization.md).

Use Git comparison to review data changes against a repository version. Add custom views through [Editor plugins](../reference/13-plugins.md).

## VS Code

Download the release VSIX, choose “Install from VSIX” in VS Code, open the workspace containing `coflow.yaml`, and verify `coflow --version` is available. The extension provides CFT/CFD language support. For language server errors, select `Coflow Language Server` in the Output panel.

## Feedback

Open “Logs and feedback” to review recent incidents and generate a diagnostic preview. Review the content before downloading and submitting it. The entry is available without an open project. See [Diagnostics and troubleshooting](../reference/09-diagnostics/01-diagnostics.md).
