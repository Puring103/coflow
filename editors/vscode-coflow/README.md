# Coflow Language Support

VS Code language support for Coflow Type File (`.cft`) schemas and Coflow Data File (`.cfd`) data.

## Features

- `.cft` and `.cfd` file associations.
- TextMate syntax highlighting plus LSP semantic tokens for declarations, annotations, strings, numbers, keywords, operators, primitive types, and built-in check functions.
- Language configuration for comments, brackets, indentation, and auto-closing pairs.
- Snippets for `const`, `enum`, `type`, `check`, `when`, quantifier blocks, and common annotations.
- LSP-backed completion items for CFT keywords, primitive types, annotations, built-in functions, workspace `const`/`enum`/`type` declarations, enum variants, and current type fields.
- LSP-backed hover text for core keywords, annotations, built-in functions, and discovered workspace symbols.
- LSP-backed outline symbols for constants, enums, enum variants, types, and fields.
- LSP-backed go to definition for workspace `const`, `enum`, `type`, enum variants, and simple field access.
- Document formatting through the CFT language server.
- Project-aware diagnostics from `coflow lsp`, including schema, data, and editor-oriented diagnostics.

## Run Locally

Open this folder in VS Code:

```powershell
code editors/vscode-coflow
```

Then press `F5` to start an Extension Development Host and open a `.cft` or `.cfd` file.

If VS Code asks for a launch target, choose the Coflow extension launch target.

Diagnostics start the `coflow` language server by default and resolve the nearest `coflow.yaml` / `coflow.yml`:

```powershell
coflow lsp <project-dir>
```

When debugging from this source repository without installing a `coflow` binary, override the settings:

```json
{
  "coflow.diagnostics.command": "cargo",
  "coflow.diagnostics.args": ["run", "--quiet", "-p", "coflow", "--", "lsp"]
}
```

The extension appends the resolved project directory to the configured arguments.

You can change this in VS Code settings:

- `coflow.diagnostics.enabled`
- `coflow.diagnostics.command`
- `coflow.diagnostics.args`
- `coflow.diagnostics.debounceMs`

## Package

Install `vsce` if needed, then package the extension:

```powershell
npm install -g @vscode/vsce
vsce package
```
