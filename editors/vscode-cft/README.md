# CFT Language Support

VS Code language support for Coflow Type File (`.cft`) schemas.

## Features

- `.cft` file association.
- TextMate syntax highlighting for declarations, annotations, strings, numbers, keywords, operators, primitive types, and built-in check functions.
- Language configuration for comments, brackets, indentation, and auto-closing pairs.
- Snippets for `const`, `enum`, `type`, `check`, `when`, quantifier blocks, and common annotations.
- Completion items for CFT keywords, primitive types, annotations, built-in functions, local `const`/`enum`/`type` declarations, enum variants, and current type fields.
- Hover text for core keywords, annotations, built-in functions, and discovered local symbols.
- Outline symbols for constants, enums, enum variants, types, and fields.
- Go to definition for workspace `const`, `enum`, `type`, enum variants, and simple field access.
- Project-aware diagnostics from `coflow cft check`, including lex, syntax, schema, and check type errors.

## Run Locally

Open this folder in VS Code:

```powershell
code editors/vscode-cft
```

Then press `F5` to start an Extension Development Host and open a `.cft` file.

If VS Code asks for a launch target, choose `Run CFT Extension`.

Diagnostics run through Cargo by default and resolve the nearest `coflow.yaml` / `coflow.yml`:

```powershell
cargo run --quiet -p coflow -- cft check --json --stdin-path schema/item.cft
```

You can change this in VS Code settings:

- `coflowCft.diagnostics.enabled`
- `coflowCft.diagnostics.command`
- `coflowCft.diagnostics.args`
- `coflowCft.diagnostics.debounceMs`

## Package

Install `vsce` if needed, then package the extension:

```powershell
npm install -g @vscode/vsce
vsce package
```
