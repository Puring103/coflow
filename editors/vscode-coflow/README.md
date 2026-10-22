# Coflow Language Support

VS Code language support for Coflow Type File (`.cft`) schemas and Coflow Data File (`.cfd`) data.

## Features

- `.cft` and `.cfd` file associations.
- TextMate syntax highlighting plus LSP semantic tokens for declarations, annotations, Option values, check rules, strings, numbers, operators, references, and built-in checks.
- Language configuration for comments, brackets, indentation, and auto-closing pairs.
- Snippets for table/data/singleton declarations, enums, aliases, constants, checks, Option fields, and supported annotations.
- LSP-backed completion items for CFT keywords, primitive types, annotations, built-in functions, workspace `const`/`enum`/`type` declarations, enum variants, and current type fields.
- LSP-backed hover text for core keywords, annotations, built-in functions, and discovered workspace symbols.
- LSP-backed outline symbols for constants, enums, enum variants, types, and fields.
- LSP-backed go to definition for workspace `const`, `enum`, `type`, enum variants, and simple field access.
- CFT and CFD document formatting through the shared language server.
- Project-aware schema diagnostics plus syntax and semantic diagnostics for open CFT/CFD text documents. Runtime data loading and record validation remain CLI/editor responsibilities.

## Run Locally

Open this folder in VS Code:

```powershell
code editors/vscode-coflow
```

Then press `F5` to start an Extension Development Host and open a `.cft` or `.cfd` file.

If VS Code asks for a launch target, choose the Coflow extension launch target.

The extension uses the `coflow` language server and discovers the nearest `coflow.yaml` / `coflow.yml`. The CLI accepts an explicit project through `--project`:

```powershell
coflow lsp --project <project-dir>
```

To select a project explicitly in VS Code, configure its absolute directory:

```json
{
  "coflow.diagnostics.args": ["lsp", "--project", "/absolute/path/to/project"]
}
```

When debugging from this source repository without installing a `coflow` binary, use Cargo with the same explicit project argument:

```json
{
  "coflow.diagnostics.command": "cargo",
  "coflow.diagnostics.args": ["run", "--quiet", "-p", "coflow", "--", "lsp", "--project", "/absolute/path/to/project"]
}
```

You can change this in VS Code settings:

- `coflow.diagnostics.enabled`: controls diagnostic publication only. LSP-backed completion,
  hover, definition, formatting, and semantic tokens remain available.
- `coflow.diagnostics.command`
- `coflow.diagnostics.args`
- `coflow.diagnostics.debounceMs`

## Package

Install `vsce` if needed, then package the extension:

```powershell
npm install -g @vscode/vsce
vsce package
```
