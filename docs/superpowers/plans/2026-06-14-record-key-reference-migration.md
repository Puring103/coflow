# 记录 Key 引用语义迁移实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 Coflow 从字段级 `@id/@ref/@index` 迁移到统一的记录 key 引用模型，并支持 Excel 中显式 `@key` / `@key.path[index]` 引用。

**Architecture:** CFT 只定义结构和类型级 `@keyAsEnum`；顶层数据记录由 loader 提供 `key`。DataModel 统一用 `type + key` 建索引，所有引用在 DataModel build 阶段解析，路径引用复用 check 风格的字段访问和索引访问语义。导出器和 C# codegen 消费已经解析后的模型，记录以保留字段 `id` 输出。

**Tech Stack:** Rust workspace, `coflow-cft`, `coflow-data-model`, `coflow-cell-value`, `coflow-loader-excel`, `coflow-checker`, exporters, C# codegen, pipeline tests.

---

## 任务 1：CFT 注解语义迁移

**文件：**
- 修改：`crates/coflow-cft/src/schema/support.rs`
- 修改：`crates/coflow-cft/src/schema/compiler.rs`
- 修改：`crates/coflow-cft/tests/schema.rs`
- 修改：`crates/coflow-cft/tests/new_annotations.rs`
- 修改：`crates/coflow-cft-lsp/src/lib.rs`

- [x] **Step 1: 写失败测试**
  - `@keyAsEnum("SkillKey") type Skill { name: string; }` 编译通过。
  - `type Bad { id: string; }` 报 schema error。
  - 旧注解 `@id/@ref/@index/@IdAsEnum/@GenAsEnum` 报错。

- [x] **Step 2: 实现 schema 规则**
  - 注解白名单只保留 `struct/flag/display/deprecated/expand/keyAsEnum`。
  - `keyAsEnum` 只能用于 type，参数必须是一个字符串。
  - 字段名 `id` 禁止声明，继承字段集合也不能包含 `id`。
  - 删除旧 `@id/@ref/@index/IdAsEnum/GenAsEnum` 的专门校验。

- [x] **Step 3: 更新 LSP**
  - completion 中移除 `@id/@ref/@index/IdAsEnum/GenAsEnum`。
  - type 目标上补充 `@keyAsEnum("Name")`。

- [x] **Step 4: 验证**
  - `cargo test -p coflow-cft`
  - `cargo test -p coflow-cft-lsp`

## 任务 2：DataModel 记录 key、记录引用和路径引用

**文件：**
- 修改：`crates/coflow-data-model/src/model.rs`
- 修改：`crates/coflow-data-model/src/schema_view.rs`
- 修改：`crates/coflow-data-model/src/compiler.rs`
- 修改：`crates/coflow-data-model/src/diagnostic.rs`
- 修改：`crates/coflow-data-model/tests/model.rs`
- 修改：`crates/coflow-data-model/tests/edge_cases.rs`

- [x] **Step 1: 写失败测试**
  - 每条 `CfdInputRecord` 带 `key`，同一具体类型重复 key 报错。
  - 子类 key 可被父类字段引用，父类 key 不能赋给子类字段。
  - `RecordRef("fireball")` 解析为 `CfdValue::Ref { key, target }`。
  - `PathRef @drop.rewards[0]`、`@table.map[Fire]` 按字段/索引路径解析。
  - 路径字段不存在、数组越界、字典 key 缺失报错并包含路径。

- [x] **Step 2: 改 API**
  - `CfdInputRecord { key, actual_type, fields }`。
  - `CfdInputValue::RecordRef(String)`。
  - `CfdInputValue::PathRef { root: String, segments: Vec<CfdRefPathSegment> }`。
  - `CfdRefPathSegment` 支持 `Field(String)` 和 `Index(CfdInputRefIndex)`；index 支持 int/string/enum variant。
  - `CfdValue::Ref { key: String, target: CfdRecordId }`。

- [x] **Step 3: 改 build 流程**
  - Phase 1 校验记录结构并保留 key。
  - Phase 2 按 concrete type 和 polymorphic assignable range 建 key index。
  - Phase 3 解析 `RecordRef` 和 `PathRef`，所有结果按当前位置期望类型做兼容检查。
  - object 类型兼容使用 `schema.is_assignable(actual, expected)`。
  - 字段访问允许穿过 `Ref`，行为与 check 一致。

- [x] **Step 4: 验证**
  - `cargo test -p coflow-data-model`

## 任务 3：Cell Value 与 Excel Loader 迁移

**文件：**
- 修改：`crates/coflow-cell-value/src/lib.rs`
- 修改：`crates/coflow-cell-value/tests/cell_value.rs`
- 修改：`crates/coflow-loader-excel/src/lib.rs`
- 修改：`crates/coflow-loader-excel/tests/excel_loader.rs`

- [x] **Step 1: 写失败测试**
  - object 字段单元格 `@fireball` 解析为 `RecordRef("fireball")`。
  - object 字段单元格 `@drop.rewards[0]` 解析为 `PathRef`。
  - object 字段裸 `fireball` 报错并提示使用 `@fireball`。
  - string 字段 `@fireball` 保持字符串。
  - Excel sheet 缺少特殊 `id` 列报错。
  - Excel `id` 列生成 `CfdInputRecord.key`，不进入 fields。

- [x] **Step 2: 实现解析**
  - `parse_value` 在目标类型为 object / nullable object / array object item / dict object value 时识别 `@` 引用。
  - 引用路径 grammar：`@name` 后接零个或多个 `.field` 或 `[index]`；index 支持 int、quoted string、enum variant 或 `Enum.Variant`。
  - 其他目标类型不解析 `@`，按原有标量/字符串/枚举逻辑走。

- [x] **Step 3: Excel loader**
  - header 解析必须发现 `id` 列。
  - 每行读取 `id` 作为 record key；空 key 报错。
  - `id` 列不参与字段列映射。
  - origin mapping 要能把 record-level key 错误定位到 `id` 列。

- [x] **Step 4: 验证**
  - `cargo test -p coflow-cell-value`
  - `cargo test -p coflow-loader-excel`

## 任务 4：Checker 与 Exporter 适配

**文件：**
- 修改：`crates/coflow-checker/src/check/*`
- 修改：`crates/coflow-exporter-core/src/lib.rs`
- 修改：`crates/coflow-exporter-core/tests/exporter_core.rs`
- 修改：`crates/coflow-exporter-json/tests/json_export.rs`
- 修改：`crates/coflow-exporter-messagepack/tests/messagepack_export.rs`

- [x] **Step 1: 写失败测试**
  - check 中 `ref_field.some_field` 继续可访问。
  - check 中虚拟 `id` 返回当前顶层记录 key。
  - exporter 对每条顶层记录输出保留字段 `id`。
  - 引用字段导出目标 key。

- [x] **Step 2: Checker**
  - 运行 top-level record check 时把 record key 放入 evaluation context。
  - 字段访问 `id` 优先返回虚拟 key；由于 CFT 禁止 `id` 字段，不会冲突。
  - `Ref` 透明解引用保持原行为。

- [x] **Step 3: Exporter**
  - 表选择不再依赖 `@id` 字段。
  - record map 先写 `id`，再写 CFT 字段。
  - `CfdValue::Ref` 输出 `key`。

- [x] **Step 4: 验证**
  - `cargo test -p coflow-checker`
  - `cargo test -p coflow-exporter-core`
  - `cargo test -p coflow-exporter-json`
  - `cargo test -p coflow-exporter-messagepack`

## 任务 5：C# Codegen 与 Pipeline 迁移

**文件：**
- 修改：`crates/coflow-codegen-csharp/src/*`
- 修改：`crates/coflow-codegen-csharp/templates/*`
- 修改：`crates/coflow-pipeline/src/*`
- 修改：`crates/coflow-pipeline/tests/*`

- [x] **Step 1: 写失败测试**
  - `@keyAsEnum` 类型生成 enum。
  - 生成类型有只读 `Id` 属性，来自导出记录 `id`。
  - object 字段生成解析后引用属性。
  - C# loader 按 key 建索引并解析引用 key。

- [x] **Step 2: Codegen**
  - schema view 删除字段 id/ref/index 概念。
  - 每个 table 的 key 类型默认为 string；有 `@keyAsEnum` 时使用生成 enum。
  - 生成 database lookup `FindType(key)`。
  - loader 第一遍读取 record `id`，第二遍解析 object field 引用。

- [x] **Step 3: Pipeline**
  - key-as-enum variants 从 DataModel record key 收集，而不是从 `@id` 字段收集。
  - lockfile 逻辑继续稳定分配 enum 值。

- [x] **Step 4: 验证**
  - `cargo test -p coflow-codegen-csharp`
  - `cargo test -p coflow-pipeline`

## 任务 6：示例、CLI 和文档迁移

**文件：**
- 修改：`examples/rpg/schema/*.cft`
- 修改：`examples/rpg/scripts/build-rpg-workbook.mjs`
- 修改：`examples/rpg/coflow.yaml`
- 修改：`examples/rpg/README.md`
- 修改：`tests/cli.rs`
- 修改：`docs/spec/*.md`

- [x] **Step 1: 示例 schema**
  - 删除所有 `@id/@ref/@index` 和字段 `id`。
  - `xxx_id: string` 迁移为 `xxx: Type` 或 `xxx: Type?`。
  - check 中旧 `xxx_id.id` 改为 `xxx.id` 或直接访问目标字段。

- [x] **Step 2: 示例 Excel**
  - 保留每个 sheet 的 `id` 列作为特殊记录 key。
  - 引用单元格改成 `@key` 或 `@key.path[index]`。

- [x] **Step 3: CLI 和文档**
  - 更新 CLI 测试里的 inline schema/data。
  - 更新 CFT、DataModel、Cell Value、Excel、JSON、MessagePack、C# codegen 文档。

- [x] **Step 4: 全量验证**
  - `cargo fmt --check`
  - `cargo test --workspace`

## 完成要求

- 每个任务遵循 TDD：先写失败测试，再实现。
- 不回滚用户已有改动，特别是当前 RPG schema 拆分。
- 所有测试通过后提交。
- 提交后推送 `codex/record-key-reference-migration` 到 origin。
