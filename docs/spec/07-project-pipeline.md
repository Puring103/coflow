# Project Pipeline

Project loading owns `coflow.yaml`, schema discovery, Excel source definitions, CLI command orchestration, JSON export, and C# codegen invocation.

## Inputs

- `coflow.yaml`
- CFT schema files discovered from project configuration
- Excel source definitions and command-line overrides

## Responsibilities

- Parse project configuration and resolve project-relative paths.
- Discover and compile schema into a `CftContainer`.
- Build parsed `ExcelSource` values and invoke `coflow-excel-loader`.
- Orchestrate CLI commands, including JSON export and C# codegen invocation.

## Non-responsibilities

- Reimplement schema compilation, Excel model building, or cell parsing.
- Own C# rendering rules; codegen receives a compiled `CftContainer` plus options.
- Act as the generated C# trusted artifact loader.
