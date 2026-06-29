# Agent Workflow

Before pushing any branch, run all four checks from the repository root:

```powershell
cargo check --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Do not push while any of these commands fail.

When the user specifies a version to package or release, reinstall the local Cargo CLI after the checks pass:

```powershell
cargo install --path . --force
```

If files under `skills/` changed in that version, refresh installed skills as well. For this local
skill package, re-run `add` without `--all`; `--all` expands to every agent and can include
project-only agents during global installs.

```powershell
npx skills add . -g --skill "*" --copy -y
```

For skills installed from a remote package with version tracking, use the updater instead:

```powershell
npx skills update -g -y
```

## Project Maintenance Notes

Keep README focused on user-facing installation, features, configuration, and usage. Put
internal architecture notes, development workflow, repository checks, and specification indexes
in this file or in `docs/`.

### Internal Crate Boundaries

- `coflow-api` defines provider traits, diagnostics, source locations, artifacts, and write contracts.
- `coflow-project` handles project configuration, path resolution, configuration diagnostics, schema file discovery, and project initialization.
- `coflow-engine` owns the shared project runtime: schema compilation, source resolve/load, data model, check, diagnostics, and source/record/file indexes.
- `coflow-builtins` registers the default provider registry for the CLI, editor, and LSP hosts.
- The root `coflow` crate owns CLI command orchestration, terminal/JSON output, export/codegen staging, and artifact commit.
- `editors/cfd-editor/src-tauri` is the editor backend host. It reuses `coflow-engine` and keeps only editor wire DTOs, graph/table views, and write command bridging.
- Provider shared algorithms live in `coflow-loader-table-core` and `coflow-exporter-core`; they do not belong in `coflow-api`.

### Specification Documents

Detailed specs live under `docs/spec/`:

- `docs/spec/01-cft.md`: CFT language specification.
- `docs/spec/02-data-model.md`: data model.
- `docs/spec/02-schema-api.md`: schema API.
- `docs/spec/03-cell-value.md`: cell value syntax.
- `docs/spec/04-excel-loader.md`: Excel loader.
- `docs/spec/05-json-export.md`: JSON export format.
- `docs/spec/06-csharp-codegen.md`: C# code generation.
- `docs/spec/07-project-pipeline.md`: project pipeline.
- `docs/spec/08-messagepack-export.md`: MessagePack export format.
- `docs/spec/09-cli.md`: CLI command behavior.
- `docs/spec/10-diagnostics.md`: diagnostics.
- `docs/spec/11-project-architecture.html`: project introduction page.
- `docs/spec/12-cfd.md`: CFD text configuration syntax.
