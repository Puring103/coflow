# Coflow 0.7.3

## Highlights

### CFD Editor Extensions

- Added a local extension host with a dedicated Extensions view for installing,
  enabling, disabling, and removing frontend plugins stored in Coflow's app-data
  directory.
- Added a first field-value rendering API. Extensions target a declared CFD type
  and can render table cells or complex-value foldout headers while native field
  editing remains available.
- Added the external ChemicalExpression renderer example, including a chemical
  equation schema and sample data.

### Record Grouping

- Added multi-record group creation and group assignment through the table
  context menu.

---

# Coflow 0.7.2

## Highlights

### Readable Complex Values In Tables

- Added bounded, Markdown-style tree previews for object, array, and dictionary values in table and dimension views.
- Kept references, enums, booleans, and other scalar values visually consistent with their existing editor rendering.
- Used field names for nested complex branches, ordering only for complex array elements, and keys for dictionary entries.
- Added dynamic row measurement and wider complex columns while keeping complex table cells read-only and keyboard navigation stable.

## Included Changes Since v0.7.1

- Refined complex-value table previews and their focused selection styling.

---

# Coflow 0.7.1

## Highlights

### Editor Navigation And Data Inspection

- Added directional navigation across the file tree, view controls, search, table, record view, graph, and inspector.
- Refined table keyboard selection so boundary movement is predictable and inspector entry remains explicit.
- Flattened object, array, and dictionary cell inspection so nested content and collection actions are immediately available.
- Added file-type navigation and persistent document tabs for multi-type data sources.

### Editor Reliability

- Fixed table column resizing so widths track the pointer, respect a minimum width, and persist under `.coflow/editor.json`.
- Preserved manual column widths while retaining automatic sizing for columns without saved settings.
- Added project check/build actions, source-file opening, and restored configured output and dimension artifacts.

## Included Commits Since v0.7.0

- `cdc9b77e` feat(editor): unify inspector record navigation
- `869de11e` feat(editor): add directional workspace navigation
- `6f1ec42a` feat(editor): add project actions and source opening
- `679e5243` feat(editor): refine table keyboard interactions
- `00f63fc9` docs: clarify coflow skill scope
- `ff6fd5b2` feat(editor): add file type navigation and document tabs
- `5a2642bd` fix(build): restore configured outputs and dimension artifacts
- `0ddf3f92` feat(editor): refine panel navigation and cell inspection
- `27ceacc7` fix(editor): make table column resizing reliable

---

# Coflow 0.7.0

## Highlights

### Canonical Schema And Dimension Architecture

- Rebuilt the CFT pipeline around immutable parsed modules and one canonical `CftSchema`, shared by the runtime, LSP, editor, loaders, checker, exporters, and code generators.
- Reorganized `coflow-cft` into explicit syntax, module, diagnostics, schema, compiler, and execution-plan boundaries, removing the old container, reflection, compatibility, `compiled`, and mixed support layers.
- Replaced synthetic dimension types and records with record-owned dimension overlays, typed coordinates, precomputed indexes, and canonical check plans.
- Kept schema construction as the fixed two-argument `build_schema(modules, dimensions)` API. Structural protection remains internal and is not user-configurable.
- Schema generations are now runtime-owned and reused for data-only mutations; schema inputs are reparsed only when they change.
- Split dimension generation, commit, and mutation preparation into bounded planning, validation, and execution helpers during the final release review.

### Runtime Reliability

- Centralized dimension source discovery and mutation planning in the runtime.
- Hardened local and provider transactions, staging, compensation, generation publication, incremental checks, and dimension regeneration failure handling.
- Added artifact generation history under Coflow state while retaining atomic active-manifest publication.
- Expanded differential, diagnostic, transaction, reload, and boundary coverage across the schema and dimension pipeline.
- Updated public and bundled skill references to describe the final canonical dimension pipeline.

### Editor Workflow

- Added consistent keyboard selection and editing across table, record, and inspector views.
- Added cell-text copy and paste in the record view using the same parser and renderer as table editing.
- Added reusable searchable native selectors for enum, reference, polymorphic type, and dictionary-key editing.
- Improved focus transitions between search, record fields, nested values, and the record sidebar.
- Moved mutation and parse failures into unobtrusive floating notices instead of layout-shifting banners.

## Compatibility

- The built-in Lark spreadsheet provider, remote `url` sources, and URI source locations have been removed. Migrate those inputs to local Excel, CSV, or CFD sources before upgrading.
- Local source formats and the JSON, MessagePack, and C# output contracts remain unchanged.

## Included Commits Since v0.6.3

- `5e7f7147` refactor: remove lark and remote sources
- `a3517d4d` docs: add schema generation refactor plan
- `fbc99b95` feat: add immutable cft module set
- `e5988bb6` feat: build schema from parsed modules
- `a8ae9e8e` refactor: rename compiled schema to cft schema
- `ca84f615` refactor: compile cft schema from module set
- `2752a8d6` feat: synthesize dimensions during cft build
- `524189db` refactor: move runtime sessions to cft schema
- `36396246` refactor: share parsed modules with schema hosts
- `ee692faf` test: compile checker fixtures with cft schema
- `2ab79fc4` refactor: separate cft module identity from container
- `a52cc583` refactor: centralize schema generations in runtime
- `a600fcc2` refactor: remove remaining schema build terminology
- `a0ca7fd3` chore: clean schema refactor diff
- `897c5ba9` chore: remove trailing schema test whitespace
- `bc21382b` refactor: open editor sessions from schema generations
- `3c99266d` docs: plan canonical cft schema and dimension overlays
- `9eb16d31` refactor: unify cft module storage
- `0775795f` refactor: add typed cft schema names
- `821ef04e` refactor: establish canonical cft schema
- `5ad93a0f` refactor: replace dimension storage with record overlays
- `d122475f` feat: index and mutate dimension overlays
- `c78a752a` refactor: clarify coflow-cft module boundaries
- `dd16f40a` refactor: complete canonical schema and dimension edits
- `94ca65af` refactor: remove obsolete schema compatibility paths
- `bdbe6a40` docs: finalize cft schema migration
- `16c81af5` docs: mark schema migration complete
- `1013bd2d` Merge branch 'main' into codex/schema-generation-architecture
- `e9cd237c` docs: document remote source removal
- `032cd278` fix: centralize dimension source discovery
- `12aa95a5` fix: restore provider transaction compensation
- `10b595bf` refactor: share cft modules with lsp
- `dabea632` test: cover dimension diagnostics
- `a0d94daf` refactor: extract dimension mutation modules
- `e280a00e` fix: harden dimension generation transactions
- `f6467473` test: close dimension transaction coverage gaps
- `6502e0c0` test: compare complete dimension diagnostics
- `31a4b5c4` Merge pull request #16 from Puring103/codex/schema-generation-architecture
- `245be0be` feat(editor): select table cells in inspector
- `1d31d966` feat: keep artifact history under coflow state
- `05ba5d68` feat(editor): add keyboard cell editing and clipboard syntax
- `9aa1310a` fix: enforce schema and dimension source invariants
- `5e1b6321` feat(editor): unify keyboard selection interactions
- `259a821e` refactor: clarify cft compiler architecture
- `72c4a832` feat(editor): improve keyboard editing controls
- `39c8588d` chore: prepare v0.7.0 release
- `749c2c13` chore: satisfy v0.7.0 release gate
- `2c519607` refactor: close v0.7.0 release gate findings
- Final release-note publication: finalized the complete v0.7.0 changelog and release metadata.
