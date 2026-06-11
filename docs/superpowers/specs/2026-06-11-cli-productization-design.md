# CLI Productization Design

## Context

Coflow already has working project-level commands for schema checking, Excel loading, JSON and MessagePack export, C# code generation, and LSP. The next step is to make the project easier to use and safer to maintain by adding a single build command, stricter project configuration validation, CI, and a root README.

## Build Command

Add `coflow build [CONFIG_OR_DIR]` as the default project pipeline command for users who want generated artifacts.

The command should:

- open and validate `coflow.yaml`;
- compile all configured CFT schema files;
- load configured Excel sources and run CFT checks;
- export data using `outputs.data.type`;
- generate C# code when `outputs.code.type: csharp` is configured;
- stop at the first failure and return a non-zero exit code.

Supported overrides:

- `--data-out DIR` overrides `outputs.data.dir`;
- `--code-out DIR` overrides `outputs.code.dir`;
- `--namespace NAME` overrides `outputs.code.namespace` for C# generation.

The first version only supports `outputs.data.type: json | messagepack` and `outputs.code.type: csharp`. If `outputs.code` is omitted, `build` still performs check and data export.

## Configuration Validation

Configuration validation belongs in `coflow-project` so all CLI commands benefit from the same rules. Parsing should reject unknown YAML fields using serde `deny_unknown_fields`.

Validation should report human-readable errors for:

- unsupported `outputs.data.type` values;
- unsupported `outputs.code.type` values;
- empty output directories;
- missing configured schema paths;
- missing source files;
- sources without sheets;
- sheets with empty names;
- duplicate column headers inside one sheet mapping.

The initial version keeps error output as plain stderr strings. It does not add a structured JSON configuration diagnostic model.

## CI

Add `.github/workflows/ci.yml` for pull requests and pushes. The workflow should use stable Rust and run:

- `cargo fmt --all -- --check`;
- `cargo clippy --workspace --all-targets -- -D warnings`;
- `cargo test --workspace`.

CI should not require extra platform setup beyond the Rust toolchain and the repository contents.

## README

Add a root `README.md` with:

- a short explanation of Coflow as a game configuration pipeline;
- a quick start based on `examples/rpg`;
- basic command examples for `check`, `build`, `export`, and `codegen`;
- a minimal `coflow.yaml` explanation;
- a compact CFT syntax overview covering constants, enums, types, annotations, defaults, refs, arrays, dictionaries, inheritance, checks, and built-ins;
- a note on JSON, MessagePack, and C# runtime dependencies.

The README should be practical and concise. Deep language and file format details stay in `docs/spec`.

## Testing

Add CLI tests that verify:

- `coflow build examples/rpg` exports JSON data and C# code;
- `coflow build` can export MessagePack based on config;
- invalid data and code output types are rejected during config validation;
- unknown config fields are rejected;
- invalid source and sheet definitions fail with clear messages.

Existing workspace tests remain the main regression suite.
