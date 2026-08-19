# Coflow 0.9.1

## 重点更新

### 格式化字符串

- CFD 和 Excel / CSV string 字段现在可以直接使用 `{field}`、`{&key.field}` 与 `{&Type::key.field}` 引用字段值。
- 引用支持穿过内联对象和记录引用；构建时会检查不存在的目标、无效路径与循环引用。
- check、JSON / MessagePack 导出和代码生成使用求值后的普通字符串，同时保留作者源码用于编辑器和 writer 回写。

### 编辑器富文本

- CFD Editor 的 string 字段新增 HTML / Unity 富文本标签补全；输入 `<` 后可用键盘选择并插入完整标签。
- 表格和记录视图会安全预览粗体、斜体、下划线、颜色、字号等受支持样式，不执行脚本、链接或外部资源。
- 格式化字段引用可以嵌入富文本，引用求值后直接显示最终预览；字段标签与说明也可通过 hover 查看。

### C# JSON 加载

- 生成的 JSON loader 改为接收 `Func<string, string?>`，按文件名加载 JSON 文本，便于直接接入 Unity `TextAsset`、Addressables 或自定义资源系统。
- MessagePack loader 保持目录加载方式不变。

## 兼容性

- JSON C# loader 的入口从 `Load(string dataDir)` 改为 `Load(Func<string, string?> loadText)`；现有调用方需要提供读取文本的回调。
- JSON、MessagePack 数据格式和 MessagePack C# loader 没有变化。升级现有项目前请运行 `coflow check` 和 `coflow build`。

---

# Coflow 0.9.0

## 重点更新

### 项目搜索与 CLI 工作流

- CFD 编辑器新增项目级搜索，支持按记录键或全文检索、显示字段级命中预览，并可直接跳转到匹配记录。
- 新增 `coflow data search`，支持在脚本中按记录键或全文查询，并提供类型、文件过滤与稳定分页。

### 更高效的编辑器工作流

- 持久化工作区标签页与视图，改进检查器导航和诊断展示，并支持删除多选记录。
- 新增 schema 感知的剪贴板处理，可将多个引用或记录直接粘贴到对应类型的列表中。
- 选中表格区域时会避开固定表头和固定列并完整显示；首行单元格超高时也不会异常下移。
- 优化文件树层级、图表及应用图标，并在编辑器各处统一使用正式 Coflow 品牌资源。
- 构建按钮新增待构建提示，通过比较实际生成产物与当前输出文件，仅在构建会改变托管内容时显示。

### Schema 与数据能力

- CFD 语法、表格单元格、校验、数据修改和编辑器控件全面支持复合枚举 flags。
- 重命名记录时同步迁移 `@idAsEnum` 锁值，保持生成枚举值稳定。
- 改进 singleton 路由、数据源校验，以及仍包含可修复诊断的中间数据编辑流程。

### 构建可靠性与分发

- 复用未变化的生成输出，并在替换托管输出目录时保留 Unity `.meta` 文件。
- macOS 独立 CLI 新增 `coflow self-update` 和 `--check`，arm64 与 x64 发布包均经过签名和公证。
- 加强产物 revision 唯一性，以及编辑器和 CLI 重复构建时的发布可靠性。

## 兼容性

- 本版本未主动改变 JSON、MessagePack 或 C# 输出契约，从 0.8.2 升级无需计划内数据迁移。
- 数据源校验更加严格，可能暴露过去未报告的配置问题。部署现有项目前请运行 `coflow check`。

---

# Coflow 0.8.2

## Highlights

### Schema And Editor Improvements

- Added schema display labels and descriptions to generated editor metadata.
- Added full-text table search in the CFD Editor, including highlighted matches in compact values.
- Improved enum presentation and schema-aware editor field behavior.

---

# Coflow 0.8.1

## Highlights

### CFD Editor Reliability

- Fixed graph layout execution to use one ELK worker, avoiding failures from a nested worker.
- Let field renderer plugins registered for a type also render its nullable form, while a renderer registered explicitly for `Type?` remains nullable-only.

---

# Coflow 0.8.0

## Highlights

### More Expressive Project Checks

- Added named top-level checks and `records(Type)` queries for project-wide
  validation rules.
- Added nullable-safe access, null coalescing, formatted strings, typed
  quantifier bindings, expanded validation functions, and custom check
  messages.
- Improved incremental check scheduling so record-set and dimension rules
  rerun only when their relevant data changes.

### Faster Data Editing

- Added record reordering and cross-file record transfers.
- Added table cell range selection, drag selection, clipboard editing, and
  batch inspector editing in the CFD Editor.
- Added saved custom views, view-specific field filtering, singleton-aware
  views, field reordering, and improved dimension editing.

### Editor Extensions

- Added project plugin references and read-only project data queries for CFD
  Editor frontend plugins.

### CLI And Diagnostics

- Made terminal-friendly output the default for CLI commands.
- Improved project-scoped artifact commands, enum lock handling, record query
  diagnostics, and language-server support for project-wide checks.

## Compatibility

- CFD string values must now be quoted. Update existing bare string values to
  use quoted CFD string syntax before upgrading.
- This release includes substantial runtime, data-model, and checker
  architecture changes. Run `coflow check` and `coflow build` in CI before
  deploying an existing project with this version.

---

# Coflow 0.7.4

## Highlights

### CFD Editor Usability

- Kept record-group headers anchored to the visible table area during
  horizontal scrolling.
- Kept record context menus fully inside the viewport, including after the
  group target list is expanded near the bottom edge.
- Prevented searchable reference and enum options from passing clicks through
  to controls underneath the menu.

### macOS Distribution

- Added full arm64 and x64 editor DMGs, signed updater archives, and matching
  CLI archives to the release workflow.
- Extended `latest.json` generation to publish updater assets for Windows and
  both supported macOS architectures.

---

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
