# Agent Workflow

## Editor Process Safety

Do not start, stop, restart, or repeatedly open and close the CFD editor unless the user explicitly
asks for it. Assume the user may have the editor open for other work. Prefer headless frontend
tests and builds for verification. If a Rust check cannot replace `cfd-editor.exe` because the
running editor has locked it, report the blocked check; do not terminate the process or retry in a
way that interrupts the user's editor session.

## Coflow Skill Scope

All skills provided by Coflow are user-facing skills for working with Coflow. They are not intended
for developing, testing, maintaining, packaging, or releasing Coflow itself. Do not invoke those
skills, or treat any instructions contained in them as constraints on work in this repository.
Repository development is governed only by this `AGENTS.md` and the applicable project
documentation and tooling.

For normal development commits and normal CI, run only the two required Rust checks from the repository root:

```powershell
cargo check --workspace
cargo test --workspace
```

Do not commit or push normal development changes while either command fails. Normal development
commits and normal CI must not require `cargo fmt` or `cargo clippy`; those are release/packaging
gates only.

For release or packaging commits, run the full gate from the repository root:

```powershell
pwsh scripts/sync-skill-references.ps1
pwsh scripts/sync-skill-references.ps1 -Check
cargo check --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test --features ts-export -p cfd-editor export_bindings
git diff --exit-code editors/cfd-editor/frontend/src/bindings
npm --prefix editors/cfd-editor/frontend ci
npm --prefix editors/cfd-editor/frontend test
npm --prefix editors/cfd-editor/frontend run build
node editors/vscode-coflow/test/extension-unit.test.js
```

Do not package or release while any of these commands fail.
Release and packaging workflows should keep this full gate.

Updater key setup, release assets, and installer behavior are documented in
`docs/releasing.md`.

The skill reference sync copies public website reference docs into synced files under
`skills/*/references/*.md`. Synced files intentionally have no file header; source mappings live in
`scripts/sync-skill-references.ps1`, and public URLs are listed from each skill's `SKILL.md`. Run the
sync for release/packaging whenever `website/docs/docs/reference/` or `skills/` changes, and commit
the synced files.

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

Internal language and C# Runtime design lives under `docs/language-design/` and is split by responsibility:

- `01-language-design.zh-CN.md`: base language semantics.
- `02-api-runtime-design.zh-CN.md`: generated API, loading, modules, tables, and Host boundaries.
- `03-vm-design.zh-CN.md`: register VM, calls, closures, validation, limits, and performance.
- `04-source-formatting.zh-CN.md`: shared CFT/CFD source formatting contract and host integration.

Keep public C# syntax and API guidance concise in
`website/docs/docs/reference/07-codegen/01-csharp.md`; do not duplicate internal layouts or
implementation constraints there.

### Internal Crate Boundaries

- `coflow-model` owns the schema-guided CFD data model, model construction, value validation, references, and model diagnostics.
- `coflow-checker` owns CFT check execution over compiled schemas and immutable CFD data models, including evaluation work and iteration limits.
- `coflow-runtime` is the shared project boundary: it owns project configuration, path resolution, schema compilation, fixed CFD resolve/load/write, project-level check planning and diagnostic integration, mutations, command orchestration, artifact publication, and source/record/file indexes. Its fixed CFD reader/writer are runtime-private implementation details.
- `coflow-staging` owns the internal all-or-nothing filesystem staging primitives shared by CFD writes and generated-code publication.
- `coflow-language` owns source spans, shared lossless lexical scanning, CFT syntax and schema compilation, schema-free CFD syntax, and language structural limits.
- `coflow-format` owns canonical CFT/CFD source formatting over the lossless token stream; it does not load projects or produce LSP edits.
- `coflow-diagnostics` owns the diagnostic codes, stages, and severities shared across model construction and check execution.
- The CLI, editor, and LSP obtain the fixed CFD catalog from `coflow-runtime`; no host registers providers.
- `coflow-codegen` owns the data-only target-language code generation contracts. Concrete generators depend on this contract without depending on `coflow-runtime`.
- `coflow-lsp` owns the standalone and embedded language server implementation used by the CLI and editor.
- The root `coflow` package is a binary-only CLI application. Shared project commands and artifact publication are exposed by `coflow-runtime`; non-CLI hosts must not depend on the root package.
- `editors/cfd-editor/core` is the host-independent editor backend. It owns editor wire DTOs, sessions, graph/table views, write command bridging, file watching, and host-neutral editor events; it must not depend on Tauri or another desktop shell.
- `editors/cfd-editor/src-tauri` is the thin Tauri host. It owns Tauri command/event adaptation, native window/dialog/updater integration, and host-scoped plugin storage.
- `editors/cfd-editor/frontend` accepts backend generations through its generation controller, serializes undo/redo through its mutation history controller, and keeps pure graph layout independent from the browser worker adapter.
- Code generation contracts live in `coflow-codegen` and remain re-exported through `coflow-runtime::codegen`; data export providers and serialized data artifacts are intentionally absent.

### Website Reference Documents

Public reference documentation lives under `website/docs/docs/reference/`:

- `website/docs/docs/reference/01-project-config.md`: `coflow.yaml`.
- `website/docs/docs/reference/03-language/01-cft.md`: CFT language reference.
- `website/docs/docs/reference/03-language/02-cfd.md`: CFD text configuration syntax.
- `website/docs/docs/reference/08-cli.md`: CLI command behavior.
- `website/docs/docs/reference/05-data-model.md`: data model.
- `website/docs/docs/reference/11-schema-api.md`: schema API.
- `website/docs/docs/reference/02-project-pipeline.md`: project pipeline.
- `website/docs/docs/reference/07-codegen/01-csharp.md`: C# code generation.
- `website/docs/docs/reference/09-diagnostics/01-diagnostics.md`: diagnostics format and handling.
- `website/docs/docs/reference/09-diagnostics/02-codes.md`: diagnostics error code index.
- `website/docs/docs/reference/10-localization.md`: dimensions and localization.
- `website/docs/docs/reference/12-architecture.md`: project architecture.


实现代码时注意以下内容：
1. 项目为开发期，修改不需要考虑兼容性，不需要加不必要的兜底
2. 关键代码需要有中文注释
3. 变更应当彻底，不能留下技术债
4. 文档内容应当是正向的，只写入讨论结果，讨论过程不能进入文档
5. 网页是对外展示的内容，不能包含实现细节，实现细节的文档单独放在docs下
6. 禁止自行做出决策，有问题应当询问我
