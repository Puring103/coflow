# MessagePack Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为 Coflow 增加 MessagePack 数据导出和匹配的 C# 运行时加载器，同时完成 loader/exporter crate 命名整理。

**Architecture:** 保留 `CfdDataModel` 作为唯一验证后数据模型。新增 `coflow-exporter-core`，它只提供 schema-aware 导出遍历和 encoder hook，不定义第二套 `ExportValue`。JSON 和 MessagePack exporter 复用 core traversal，各自负责最终编码；C# codegen 根据 `outputs.data.type` 选择 JSON 或 MessagePack loader 模板。

**Tech Stack:** Rust workspace、Cargo、clap、serde_json、rmp、rmpv、Tera、C#、Newtonsoft.Json、MessagePack-CSharp。

---

## File Structure

Create:

- `crates/coflow-exporter-core/Cargo.toml` - 公共 exporter traversal crate。
- `crates/coflow-exporter-core/src/lib.rs` - `ExportEncoder`、`export_model_with_encoder`、共享 schema-aware traversal。
- `crates/coflow-exporter-core/tests/exporter_core.rs` - core traversal 行为测试。
- `crates/coflow-exporter-messagepack/Cargo.toml` - MessagePack exporter crate。
- `crates/coflow-exporter-messagepack/src/lib.rs` - `export_messagepack_model` 和 MessagePack byte encoder。
- `crates/coflow-exporter-messagepack/tests/messagepack_export.rs` - MessagePack 编码测试。
- `crates/coflow-codegen-csharp/templates/database_messagepack.cs.tera` - MessagePack C# loader 模板。
- `docs/spec/08-messagepack-export.md` - MessagePack 导出格式文档。

Move:

- `crates/coflow-excel-loader/` -> `crates/coflow-loader-excel/`
- `crates/coflow-json-export/` -> `crates/coflow-exporter-json/`

Modify:

- `Cargo.toml` - workspace members 和 root dependencies。
- `Cargo.lock` - package rename 和新增 dependencies。
- `src/main.rs` - crate imports、`export messagepack` 命令、codegen format selection。
- `crates/coflow-exporter-json/Cargo.toml` - package name 和 core dependency。
- `crates/coflow-exporter-json/src/lib.rs` - 改为使用 `coflow-exporter-core`。
- `crates/coflow-exporter-json/tests/json_export.rs` - crate import rename。
- `crates/coflow-codegen-csharp/Cargo.toml` - description 可更新为 JSON/MessagePack runtime codegen。
- `crates/coflow-codegen-csharp/src/lib.rs` - public API 增加 format-aware generator。
- `crates/coflow-codegen-csharp/src/ir.rs` - `CsharpDataFormat` options。
- `crates/coflow-codegen-csharp/src/model.rs` - loader model 扩展为 JSON/MessagePack 共用字段信息。
- `crates/coflow-codegen-csharp/src/emit.rs` - 按 data format 生成 load steps 和 field reader expressions。
- `crates/coflow-codegen-csharp/src/render.rs` - 选择 JSON 或 MessagePack database template。
- `crates/coflow-codegen-csharp/templates/database.cs.tera` - 保持 JSON loader。
- `tests/cli.rs` - CLI、codegen、可选 .NET e2e 测试。
- `examples/rpg/coflow.yaml` - 保持 JSON 默认；测试内复制修改为 MessagePack。
- `docs/spec/05-json-export.md` - 提到 JSON exporter crate 新名。
- `docs/spec/06-csharp-codegen.md` - 说明 loader 跟随 `outputs.data.type`。
- `docs/spec/07-project-pipeline.md` - 说明 JSON 和 MessagePack export。

---

### Task 1: Rename Existing Loader And JSON Exporter Crates

**Files:**

- Move: `crates/coflow-excel-loader/` -> `crates/coflow-loader-excel/`
- Move: `crates/coflow-json-export/` -> `crates/coflow-exporter-json/`
- Modify: `Cargo.toml`
- Modify: `src/main.rs`
- Modify: `crates/coflow-loader-excel/Cargo.toml`
- Modify: `crates/coflow-exporter-json/Cargo.toml`
- Modify: `crates/coflow-exporter-json/tests/json_export.rs`
- Modify: `Cargo.lock`

- [ ] **Step 1: Capture baseline**

Run:

```powershell
cargo check --workspace
```

Expected: PASS before mechanical rename.

- [ ] **Step 2: Move directories**

Run:

```powershell
git mv crates/coflow-excel-loader crates/coflow-loader-excel
git mv crates/coflow-json-export crates/coflow-exporter-json
```

Expected: directories are moved and Git records renames.

- [ ] **Step 3: Update root workspace and dependencies**

Edit `Cargo.toml`:

```toml
[workspace]
members = [
    "crates/coflow-cft",
    "crates/coflow-data-model",
    "crates/coflow-cell-value",
    "crates/coflow-checker",
    "crates/coflow-loader-excel",
    "crates/coflow-exporter-json",
    "crates/coflow-codegen-csharp",
    "crates/coflow-project",
    "crates/coflow-cft-lsp",
]
resolver = "2"

[dependencies]
clap = { version = "4", features = ["derive"] }
coflow-cft = { path = "crates/coflow-cft" }
coflow-cft-lsp = { path = "crates/coflow-cft-lsp" }
coflow-checker = { path = "crates/coflow-checker" }
coflow-codegen-csharp = { path = "crates/coflow-codegen-csharp" }
coflow-loader-excel = { path = "crates/coflow-loader-excel" }
coflow-exporter-json = { path = "crates/coflow-exporter-json" }
coflow-project = { path = "crates/coflow-project" }
serde = { version = "1", features = ["derive"] }
serde_json = { version = "1", features = ["preserve_order"] }
```

- [ ] **Step 4: Update moved package names**

Edit `crates/coflow-loader-excel/Cargo.toml`:

```toml
[package]
name = "coflow-loader-excel"
version = "0.1.0"
edition = "2021"
description = "Excel loader for Coflow data models."
license = "MIT OR Apache-2.0"
repository = "https://github.com/wtlll/ScriptForGame"
readme = "../../README.md"
keywords = ["config", "game-data", "excel"]
categories = ["config", "game-development"]
publish = false
```

Keep the existing `[lints]` and `[dependencies]` sections in that file.

Edit `crates/coflow-exporter-json/Cargo.toml`:

```toml
[package]
name = "coflow-exporter-json"
version = "0.1.0"
edition = "2021"
description = "JSON exporter for validated Coflow data models."
license = "MIT OR Apache-2.0"
repository = "https://github.com/wtlll/ScriptForGame"
readme = "../../README.md"
keywords = ["config", "game-data", "json"]
categories = ["config", "game-development"]
publish = false
```

Keep existing lints and dependencies for now.

- [ ] **Step 5: Update Rust import names**

Edit `src/main.rs`:

```rust
use coflow_loader_excel::{
    load_excel, ExcelDiagnostic, ExcelDiagnostics, ExcelLoadError, ExcelLocation, ExcelSheet,
    ExcelSource,
};
use coflow_exporter_json::export_json_model;
```

Edit `crates/coflow-exporter-json/tests/json_export.rs`:

```rust
use coflow_exporter_json::export_json_model;
```

- [ ] **Step 6: Verify rename**

Run:

```powershell
cargo check --workspace
```

Expected: PASS. `Cargo.lock` updates package names.

- [ ] **Step 7: Commit**

Run:

```powershell
git add Cargo.toml Cargo.lock src/main.rs crates/coflow-loader-excel crates/coflow-exporter-json
git commit -m "refactor: rename loader and json exporter crates"
```

Expected: commit succeeds.

---

### Task 2: Add Exporter Core Traversal

**Files:**

- Create: `crates/coflow-exporter-core/Cargo.toml`
- Create: `crates/coflow-exporter-core/src/lib.rs`
- Create: `crates/coflow-exporter-core/tests/exporter_core.rs`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`

- [ ] **Step 1: Write failing exporter core tests**

Create `crates/coflow-exporter-core/tests/exporter_core.rs`:

```rust
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use coflow_cft::{CftContainer, ModuleId};
use coflow_data_model::{CfdDataModel, CfdInputDictKey, CfdInputValue};
use coflow_exporter_core::{export_model_with_encoder, ExportEncoder};
use std::collections::BTreeMap;

type TestResult = Result<(), String>;

#[derive(Debug, Clone, PartialEq)]
enum TestValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Array(Vec<TestValue>),
    Map(Vec<(String, TestValue)>),
}

#[derive(Debug, Default)]
struct TestEncoder;

impl ExportEncoder for TestEncoder {
    type Value = TestValue;
    type Error = String;

    fn null(&mut self) -> Result<Self::Value, Self::Error> { Ok(TestValue::Null) }
    fn bool(&mut self, value: bool) -> Result<Self::Value, Self::Error> { Ok(TestValue::Bool(value)) }
    fn int(&mut self, value: i64) -> Result<Self::Value, Self::Error> { Ok(TestValue::Int(value)) }
    fn float(&mut self, value: f64) -> Result<Self::Value, Self::Error> { Ok(TestValue::Float(value)) }
    fn string(&mut self, value: &str) -> Result<Self::Value, Self::Error> { Ok(TestValue::String(value.to_string())) }
    fn array(&mut self, items: Vec<Self::Value>) -> Result<Self::Value, Self::Error> { Ok(TestValue::Array(items)) }
    fn map(&mut self, entries: Vec<(String, Self::Value)>) -> Result<Self::Value, Self::Error> { Ok(TestValue::Map(entries)) }
}

fn compile_schema(source: &str) -> Result<CftContainer, String> {
    let mut container = CftContainer::new();
    container
        .add_module(ModuleId::from("main"), source)
        .map_err(|err| format!("add schema: {err:?}"))?;
    container
        .compile()
        .map_err(|err| format!("compile schema: {err:?}"))?;
    Ok(container)
}

fn build_model(builder: coflow_data_model::CfdModelBuilder<'_>) -> Result<CfdDataModel, String> {
    builder.build().map_err(|err| format!("build model: {err:?}"))
}

fn export(
    schema: &CftContainer,
    model: &CfdDataModel,
) -> Result<BTreeMap<String, TestValue>, String> {
    export_model_with_encoder(schema, model, &mut TestEncoder)
        .map_err(|err| err.to_string())
}

#[test]
fn exports_tables_fields_refs_type_tags_and_dict_keys() -> TestResult {
    let schema = compile_schema(
        r#"
            enum Rarity { Common = 0, Rare = 10, }
            type Item { @id id: string; rarity: Rarity = Rarity.Common; }
            abstract type Reward { id: string; }
            type ItemReward : Reward {
                @ref(Item)
                item_id: string;
                attrs: {int: string} = {};
            }
            type DropTable {
                @id id: string;
                rewards: [Reward];
            }
        "#,
    )?;

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Item",
        [
            ("id", CfdInputValue::from("item_1")),
            ("rarity", CfdInputValue::enum_variant("Rarity", "Rare")),
        ],
    );
    builder.add_record(
        "DropTable",
        [
            ("id", CfdInputValue::from("drop_1")),
            (
                "rewards",
                CfdInputValue::Array(vec![CfdInputValue::object(
                    "ItemReward",
                    [
                        ("id", CfdInputValue::from("reward_1")),
                        ("item_id", CfdInputValue::from("item_1")),
                        (
                            "attrs",
                            CfdInputValue::dict([(
                                CfdInputDictKey::from(7_i64),
                                CfdInputValue::from("seven"),
                            )]),
                        ),
                    ],
                )]),
            ),
        ],
    );
    let model = build_model(builder)?;
    let tables = export(&schema, &model)?;

    assert_eq!(
        tables["Item"],
        TestValue::Array(vec![TestValue::Map(vec![
            ("id".to_string(), TestValue::String("item_1".to_string())),
            ("rarity".to_string(), TestValue::Int(10)),
        ])])
    );
    assert_eq!(
        tables["DropTable"],
        TestValue::Array(vec![TestValue::Map(vec![
            ("id".to_string(), TestValue::String("drop_1".to_string())),
            (
                "rewards".to_string(),
                TestValue::Array(vec![TestValue::Map(vec![
                    ("$type".to_string(), TestValue::String("ItemReward".to_string())),
                    ("id".to_string(), TestValue::String("reward_1".to_string())),
                    ("item_id".to_string(), TestValue::String("item_1".to_string())),
                    (
                        "attrs".to_string(),
                        TestValue::Map(vec![(
                            "7".to_string(),
                            TestValue::String("seven".to_string()),
                        )]),
                    ),
                ])]),
            ),
        ])])
    );
    Ok(())
}

#[test]
fn exports_empty_tables_for_missing_concrete_id_types() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item { @id id: string; }
            type Monster { @id id: string; }
            type InlineOnly { value: string; }
        "#,
    )?;
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("Item", [("id", CfdInputValue::from("item_1"))]);
    let model = build_model(builder)?;
    let tables = export(&schema, &model)?;

    assert!(tables.contains_key("Item"));
    assert!(tables.contains_key("Monster"));
    assert!(!tables.contains_key("InlineOnly"));
    assert_eq!(tables["Monster"], TestValue::Array(Vec::new()));
    Ok(())
}
```

- [ ] **Step 2: Run tests to verify RED**

Run:

```powershell
cargo test -p coflow-exporter-core
```

Expected: FAIL because package `coflow-exporter-core` does not exist.

- [ ] **Step 3: Add core crate to workspace**

Edit `Cargo.toml` workspace members:

```toml
    "crates/coflow-loader-excel",
    "crates/coflow-exporter-core",
    "crates/coflow-exporter-json",
```

Create `crates/coflow-exporter-core/Cargo.toml`:

```toml
[package]
name = "coflow-exporter-core"
version = "0.1.0"
edition = "2021"
description = "Shared schema-aware exporter traversal for Coflow data models."
license = "MIT OR Apache-2.0"
repository = "https://github.com/wtlll/ScriptForGame"
readme = "../../README.md"
keywords = ["config", "game-data", "export"]
categories = ["config", "game-development"]
publish = false

[lints]
workspace = true

[dependencies]
coflow-cft = { path = "../coflow-cft" }
coflow-data-model = { path = "../coflow-data-model" }
```

- [ ] **Step 4: Implement minimal core traversal**

Create `crates/coflow-exporter-core/src/lib.rs` with:

```rust
//! Shared schema-aware exporter traversal for validated Coflow data models.

#![cfg_attr(
    not(test),
    deny(
        clippy::dbg_macro,
        clippy::expect_used,
        clippy::panic,
        clippy::panic_in_result_fn,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable,
        clippy::unwrap_used
    )
)]

use coflow_cft::{
    CftAnnotation, CftAnnotationValue, CftContainer, CftSchemaField, CftSchemaTypeRef,
};
use coflow_data_model::{CfdDataModel, CfdDictKey, CfdIdValue, CfdRecord, CfdTable, CfdValue};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

pub trait ExportEncoder {
    type Value;
    type Error;

    fn null(&mut self) -> Result<Self::Value, Self::Error>;
    fn bool(&mut self, value: bool) -> Result<Self::Value, Self::Error>;
    fn int(&mut self, value: i64) -> Result<Self::Value, Self::Error>;
    fn float(&mut self, value: f64) -> Result<Self::Value, Self::Error>;
    fn string(&mut self, value: &str) -> Result<Self::Value, Self::Error>;
    fn array(&mut self, items: Vec<Self::Value>) -> Result<Self::Value, Self::Error>;
    fn map(&mut self, entries: Vec<(String, Self::Value)>) -> Result<Self::Value, Self::Error>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportError {
    message: String,
}

impl ExportError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(f)
    }
}

impl std::error::Error for ExportError {}

pub fn export_model_with_encoder<E>(
    schema: &CftContainer,
    model: &CfdDataModel,
    encoder: &mut E,
) -> Result<BTreeMap<String, E::Value>, ExportError>
where
    E: ExportEncoder,
    E::Error: fmt::Display,
{
    Exporter::new(schema, model, encoder).export()
}

struct Exporter<'a, E> {
    schema: SchemaView<'a>,
    model: &'a CfdDataModel,
    encoder: &'a mut E,
}

impl<'a, E> Exporter<'a, E>
where
    E: ExportEncoder,
    E::Error: fmt::Display,
{
    fn new(schema: &'a CftContainer, model: &'a CfdDataModel, encoder: &'a mut E) -> Self {
        Self {
            schema: SchemaView::new(schema),
            model,
            encoder,
        }
    }

    fn export(&mut self) -> Result<BTreeMap<String, E::Value>, ExportError> {
        let mut out = BTreeMap::new();
        for schema_type in self.schema.schema.all_types() {
            if schema_type.is_abstract {
                continue;
            }
            let has_id_field = schema_type
                .all_fields
                .iter()
                .any(|field| has_annotation(&field.annotations, "id"));
            if !has_id_field {
                continue;
            }

            let value = if let Some(table) = self.model.table(&schema_type.name) {
                self.encode_table(table)?
            } else {
                self.call_encoder(|encoder| encoder.array(Vec::new()))?
            };
            out.insert(schema_type.name.clone(), value);
        }
        Ok(out)
    }

    fn encode_table(&mut self, table: &CfdTable) -> Result<E::Value, ExportError> {
        let mut records = Vec::with_capacity(table.records.len());
        for record_id in &table.records {
            let record = self.model.record(*record_id).ok_or_else(|| {
                ExportError::new(format!(
                    "table `{}` references missing record `{record_id}`",
                    table.type_name
                ))
            })?;
            records.push(self.encode_record(&table.type_name, record, TypeTagMode::Never)?);
        }
        self.call_encoder(|encoder| encoder.array(records))
    }

    fn encode_record(
        &mut self,
        declared_type: &str,
        record: &CfdRecord,
        tag_mode: TypeTagMode,
    ) -> Result<E::Value, ExportError> {
        let mut entries = Vec::new();
        if tag_mode == TypeTagMode::WhenPolymorphic
            && self.schema.range_is_polymorphic(declared_type)
        {
            let actual = record.actual_type.clone();
            entries.push((
                "$type".to_string(),
                self.call_encoder(|encoder| encoder.string(&actual))?,
            ));
        }

        for field in self.schema.full_fields(&record.actual_type)? {
            let value = record.fields.get(&field.name).ok_or_else(|| {
                ExportError::new(format!(
                    "record `{}` is missing field `{}`",
                    record.actual_type, field.name
                ))
            })?;
            let field_value = self.encode_field(&field, value)?;
            entries.push((field.name, field_value));
        }
        self.call_encoder(|encoder| encoder.map(entries))
    }

    fn encode_field(&mut self, field: &FieldMeta, value: &CfdValue) -> Result<E::Value, ExportError> {
        if field.ref_target.is_some() {
            return self.encode_ref(value);
        }
        self.encode_value(&field.ty, value)
    }

    fn encode_value(
        &mut self,
        declared_type: &CftSchemaTypeRef,
        value: &CfdValue,
    ) -> Result<E::Value, ExportError> {
        if let CftSchemaTypeRef::Nullable(inner) = declared_type {
            return match value {
                CfdValue::Null => self.call_encoder(ExportEncoder::null),
                other => self.encode_value(inner, other),
            };
        }

        match value {
            CfdValue::Null => self.call_encoder(ExportEncoder::null),
            CfdValue::Bool(value) => self.call_encoder(|encoder| encoder.bool(*value)),
            CfdValue::Int(value) => self.call_encoder(|encoder| encoder.int(*value)),
            CfdValue::Float(value) => self.call_encoder(|encoder| encoder.float(*value)),
            CfdValue::String(value) => self.call_encoder(|encoder| encoder.string(value)),
            CfdValue::Enum(value) => self.call_encoder(|encoder| encoder.int(value.value)),
            CfdValue::Object(record) => {
                let type_name = match declared_type {
                    CftSchemaTypeRef::Named(type_name) => type_name,
                    other => {
                        return Err(ExportError::new(format!(
                            "object value has non-object declared type `{}`",
                            display_type_ref(other)
                        )))
                    }
                };
                self.encode_record(type_name, record, TypeTagMode::WhenPolymorphic)
            }
            CfdValue::Ref { .. } => self.encode_ref(value),
            CfdValue::Array(items) => {
                let inner = match declared_type {
                    CftSchemaTypeRef::Array(inner) => inner,
                    other => {
                        return Err(ExportError::new(format!(
                            "array value has non-array declared type `{}`",
                            display_type_ref(other)
                        )))
                    }
                };
                let values = items
                    .iter()
                    .map(|item| self.encode_value(inner, item))
                    .collect::<Result<Vec<_>, _>>()?;
                self.call_encoder(|encoder| encoder.array(values))
            }
            CfdValue::Dict(entries) => {
                let value_ty = match declared_type {
                    CftSchemaTypeRef::Dict(_, value_ty) => value_ty,
                    other => {
                        return Err(ExportError::new(format!(
                            "dict value has non-dict declared type `{}`",
                            display_type_ref(other)
                        )))
                    }
                };
                let mut out = Vec::with_capacity(entries.len());
                for (key, value) in entries {
                    out.push((dict_key_string(key), self.encode_value(value_ty, value)?));
                }
                self.call_encoder(|encoder| encoder.map(out))
            }
        }
    }

    fn encode_ref(&mut self, value: &CfdValue) -> Result<E::Value, ExportError> {
        match value {
            CfdValue::Null => self.call_encoder(ExportEncoder::null),
            CfdValue::Ref { id, .. } => self.encode_id(id),
            other => Err(ExportError::new(format!(
                "expected ref value, got `{}`",
                value_kind(other)
            ))),
        }
    }

    fn encode_id(&mut self, id: &CfdIdValue) -> Result<E::Value, ExportError> {
        match id {
            CfdIdValue::String(value) => self.call_encoder(|encoder| encoder.string(value)),
            CfdIdValue::Int(value) => self.call_encoder(|encoder| encoder.int(*value)),
        }
    }

    fn call_encoder(
        &mut self,
        call: impl FnOnce(&mut E) -> Result<E::Value, E::Error>,
    ) -> Result<E::Value, ExportError> {
        call(self.encoder).map_err(|err| ExportError::new(err.to_string()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TypeTagMode {
    Never,
    WhenPolymorphic,
}

struct SchemaView<'a> {
    schema: &'a CftContainer,
    children_by_parent: BTreeMap<String, Vec<String>>,
}

impl<'a> SchemaView<'a> {
    fn new(schema: &'a CftContainer) -> Self {
        let mut children_by_parent = BTreeMap::<String, Vec<String>>::new();
        for schema_type in schema.all_types() {
            if let Some(parent) = &schema_type.parent {
                children_by_parent
                    .entry(parent.clone())
                    .or_default()
                    .push(schema_type.name.clone());
            }
        }
        Self { schema, children_by_parent }
    }

    fn full_fields(&self, type_name: &str) -> Result<Vec<FieldMeta>, ExportError> {
        let mut out = Vec::new();
        self.fill_fields(type_name, &mut out, &mut BTreeSet::new())?;
        Ok(out)
    }

    fn fill_fields(
        &self,
        type_name: &str,
        out: &mut Vec<FieldMeta>,
        seen: &mut BTreeSet<String>,
    ) -> Result<(), ExportError> {
        if !seen.insert(type_name.to_string()) {
            return Ok(());
        }
        let schema_type = self.schema.resolve_type(type_name).ok_or_else(|| {
            ExportError::new(format!("unknown CFT type `{type_name}` during export"))
        })?;
        if let Some(parent) = &schema_type.parent {
            self.fill_fields(parent, out, seen)?;
        }
        for field in &schema_type.fields {
            out.push(FieldMeta::from_schema(field));
        }
        Ok(())
    }

    fn range_is_polymorphic(&self, type_name: &str) -> bool {
        self.schema
            .resolve_type(type_name)
            .is_some_and(|schema_type| schema_type.is_abstract)
            || self
                .children_by_parent
                .get(type_name)
                .is_some_and(|children| !children.is_empty())
    }
}

#[derive(Debug, Clone)]
struct FieldMeta {
    name: String,
    ty: CftSchemaTypeRef,
    ref_target: Option<String>,
}

impl FieldMeta {
    fn from_schema(field: &CftSchemaField) -> Self {
        Self {
            name: field.name.clone(),
            ty: field.ty_ref.clone(),
            ref_target: ref_target(&field.annotations),
        }
    }
}

fn ref_target(annotations: &[CftAnnotation]) -> Option<String> {
    annotations
        .iter()
        .find(|annotation| annotation.name == "ref")
        .and_then(|annotation| annotation.args.first())
        .and_then(|arg| match arg {
            CftAnnotationValue::Name(name) => Some(name.clone()),
            _ => None,
        })
}

fn has_annotation(annotations: &[CftAnnotation], name: &str) -> bool {
    annotations.iter().any(|annotation| annotation.name == name)
}

fn dict_key_string(key: &CfdDictKey) -> String {
    match key {
        CfdDictKey::String(value) => value.clone(),
        CfdDictKey::Int(value) => value.to_string(),
        CfdDictKey::Enum(value) => value.value.to_string(),
    }
}

const fn value_kind(value: &CfdValue) -> &'static str {
    match value {
        CfdValue::Null => "null",
        CfdValue::Bool(_) => "bool",
        CfdValue::Int(_) => "int",
        CfdValue::Float(_) => "float",
        CfdValue::String(_) => "string",
        CfdValue::Enum(_) => "enum",
        CfdValue::Object(_) => "object",
        CfdValue::Ref { .. } => "ref",
        CfdValue::Array(_) => "array",
        CfdValue::Dict(_) => "dict",
    }
}

fn display_type_ref(ty: &CftSchemaTypeRef) -> String {
    match ty {
        CftSchemaTypeRef::Int => "int".to_string(),
        CftSchemaTypeRef::Float => "float".to_string(),
        CftSchemaTypeRef::Bool => "bool".to_string(),
        CftSchemaTypeRef::String => "string".to_string(),
        CftSchemaTypeRef::Named(name) => name.clone(),
        CftSchemaTypeRef::Array(inner) => format!("[{}]", display_type_ref(inner)),
        CftSchemaTypeRef::Dict(key, value) => {
            format!("{{{}: {}}}", display_type_ref(key), display_type_ref(value))
        }
        CftSchemaTypeRef::Nullable(inner) => format!("{}?", display_type_ref(inner)),
    }
}
```

- [ ] **Step 5: Run tests to verify GREEN**

Run:

```powershell
cargo test -p coflow-exporter-core
```

Expected: PASS.

- [ ] **Step 6: Run formatter**

Run:

```powershell
cargo fmt
```

Expected: no errors.

- [ ] **Step 7: Commit**

Run:

```powershell
git add Cargo.toml Cargo.lock crates/coflow-exporter-core
git commit -m "feat: add shared exporter traversal"
```

Expected: commit succeeds.

---

### Task 3: Migrate JSON Exporter To Exporter Core

**Files:**

- Modify: `crates/coflow-exporter-json/Cargo.toml`
- Modify: `crates/coflow-exporter-json/src/lib.rs`
- Modify: `crates/coflow-exporter-json/tests/json_export.rs`

- [ ] **Step 1: Run existing JSON tests as baseline**

Run:

```powershell
cargo test -p coflow-exporter-json
```

Expected: PASS before refactor.

- [ ] **Step 2: Add core dependency**

Edit `crates/coflow-exporter-json/Cargo.toml` dependencies:

```toml
[dependencies]
coflow-cft = { path = "../coflow-cft" }
coflow-data-model = { path = "../coflow-data-model" }
coflow-exporter-core = { path = "../coflow-exporter-core" }
serde_json = { version = "1", features = ["preserve_order"] }
```

- [ ] **Step 3: Replace local traversal with JSON encoder**

Replace `crates/coflow-exporter-json/src/lib.rs` body after lint attributes with:

```rust
use coflow_cft::CftContainer;
use coflow_data_model::CfdDataModel;
use coflow_exporter_core::{export_model_with_encoder, ExportEncoder};
use serde_json::{Map, Number, Value};
use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonExportError {
    pub message: String,
}

impl JsonExportError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for JsonExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(f)
    }
}

impl std::error::Error for JsonExportError {}

pub fn export_json_model(
    schema: &CftContainer,
    model: &CfdDataModel,
) -> Result<BTreeMap<String, Value>, JsonExportError> {
    export_model_with_encoder(schema, model, &mut JsonEncoder)
        .map_err(|err| JsonExportError::new(format!("failed to export JSON model: {err}")))
}

#[derive(Debug, Default)]
struct JsonEncoder;

impl ExportEncoder for JsonEncoder {
    type Value = Value;
    type Error = JsonExportError;

    fn null(&mut self) -> Result<Self::Value, Self::Error> {
        Ok(Value::Null)
    }

    fn bool(&mut self, value: bool) -> Result<Self::Value, Self::Error> {
        Ok(Value::Bool(value))
    }

    fn int(&mut self, value: i64) -> Result<Self::Value, Self::Error> {
        Ok(Value::Number(Number::from(value)))
    }

    fn float(&mut self, value: f64) -> Result<Self::Value, Self::Error> {
        Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| JsonExportError::new("cannot export non-finite float"))
    }

    fn string(&mut self, value: &str) -> Result<Self::Value, Self::Error> {
        Ok(Value::String(value.to_string()))
    }

    fn array(&mut self, items: Vec<Self::Value>) -> Result<Self::Value, Self::Error> {
        Ok(Value::Array(items))
    }

    fn map(&mut self, entries: Vec<(String, Self::Value)>) -> Result<Self::Value, Self::Error> {
        Ok(Value::Object(entries.into_iter().collect::<Map<_, _>>()))
    }
}
```

- [ ] **Step 4: Run JSON tests**

Run:

```powershell
cargo test -p coflow-exporter-json
```

Expected: PASS. Existing JSON output remains unchanged.

- [ ] **Step 5: Run exporter core tests**

Run:

```powershell
cargo test -p coflow-exporter-core
```

Expected: PASS.

- [ ] **Step 6: Commit**

Run:

```powershell
git add crates/coflow-exporter-json Cargo.lock
git commit -m "refactor: reuse exporter core for json"
```

Expected: commit succeeds.

---

### Task 4: Add MessagePack Exporter Crate

**Files:**

- Create: `crates/coflow-exporter-messagepack/Cargo.toml`
- Create: `crates/coflow-exporter-messagepack/src/lib.rs`
- Create: `crates/coflow-exporter-messagepack/tests/messagepack_export.rs`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`

- [ ] **Step 1: Write failing MessagePack exporter tests**

Create `crates/coflow-exporter-messagepack/tests/messagepack_export.rs`:

```rust
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use coflow_cft::{CftContainer, ModuleId};
use coflow_data_model::{CfdDataModel, CfdInputDictKey, CfdInputValue};
use coflow_exporter_messagepack::export_messagepack_model;
use rmpv::Value;

type TestResult = Result<(), String>;

fn compile_schema(source: &str) -> Result<CftContainer, String> {
    let mut container = CftContainer::new();
    container
        .add_module(ModuleId::from("main"), source)
        .map_err(|err| format!("add schema: {err:?}"))?;
    container
        .compile()
        .map_err(|err| format!("compile schema: {err:?}"))?;
    Ok(container)
}

fn build_model(builder: coflow_data_model::CfdModelBuilder<'_>) -> Result<CfdDataModel, String> {
    builder.build().map_err(|err| format!("build model: {err:?}"))
}

fn decode(bytes: &[u8]) -> Result<Value, String> {
    let mut cursor = std::io::Cursor::new(bytes);
    rmpv::decode::read_value(&mut cursor).map_err(|err| format!("decode msgpack: {err}"))
}

#[test]
fn exports_messagepack_tables_with_json_equivalent_shape() -> TestResult {
    let schema = compile_schema(
        r#"
            enum Rarity { Common = 0, Rare = 10, }
            type Item { @id id: string; rarity: Rarity = Rarity.Common; }
            abstract type Reward { id: string; }
            type ItemReward : Reward {
                @ref(Item)
                item_id: string;
                attrs: {int: string} = {};
            }
            type DropTable {
                @id id: string;
                rewards: [Reward];
            }
        "#,
    )?;

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Item",
        [
            ("id", CfdInputValue::from("item_1")),
            ("rarity", CfdInputValue::enum_variant("Rarity", "Rare")),
        ],
    );
    builder.add_record(
        "DropTable",
        [
            ("id", CfdInputValue::from("drop_1")),
            (
                "rewards",
                CfdInputValue::Array(vec![CfdInputValue::object(
                    "ItemReward",
                    [
                        ("id", CfdInputValue::from("reward_1")),
                        ("item_id", CfdInputValue::from("item_1")),
                        (
                            "attrs",
                            CfdInputValue::dict([(
                                CfdInputDictKey::from(7_i64),
                                CfdInputValue::from("seven"),
                            )]),
                        ),
                    ],
                )]),
            ),
        ],
    );
    let model = build_model(builder)?;
    let tables = export_messagepack_model(&schema, &model)
        .map_err(|err| format!("export messagepack: {err}"))?;

    assert!(tables.contains_key("Item"));
    assert!(tables.contains_key("DropTable"));
    let item = decode(&tables["Item"])?;
    let drop_table = decode(&tables["DropTable"])?;

    assert_eq!(
        item,
        Value::Array(vec![Value::Map(vec![
            (Value::from("id"), Value::from("item_1")),
            (Value::from("rarity"), Value::from(10_i64)),
        ])])
    );
    assert_eq!(
        drop_table,
        Value::Array(vec![Value::Map(vec![
            (Value::from("id"), Value::from("drop_1")),
            (
                Value::from("rewards"),
                Value::Array(vec![Value::Map(vec![
                    (Value::from("$type"), Value::from("ItemReward")),
                    (Value::from("id"), Value::from("reward_1")),
                    (Value::from("item_id"), Value::from("item_1")),
                    (
                        Value::from("attrs"),
                        Value::Map(vec![(Value::from("7"), Value::from("seven"))]),
                    ),
                ])]),
            ),
        ])])
    );
    Ok(())
}
```

- [ ] **Step 2: Run tests to verify RED**

Run:

```powershell
cargo test -p coflow-exporter-messagepack
```

Expected: FAIL because package `coflow-exporter-messagepack` does not exist.

- [ ] **Step 3: Add crate and dependencies**

Edit `Cargo.toml` workspace members:

```toml
    "crates/coflow-exporter-core",
    "crates/coflow-exporter-json",
    "crates/coflow-exporter-messagepack",
```

Create `crates/coflow-exporter-messagepack/Cargo.toml`:

```toml
[package]
name = "coflow-exporter-messagepack"
version = "0.1.0"
edition = "2021"
description = "MessagePack exporter for validated Coflow data models."
license = "MIT OR Apache-2.0"
repository = "https://github.com/wtlll/ScriptForGame"
readme = "../../README.md"
keywords = ["config", "game-data", "messagepack"]
categories = ["config", "game-development"]
publish = false

[lints]
workspace = true

[dependencies]
coflow-cft = { path = "../coflow-cft" }
coflow-data-model = { path = "../coflow-data-model" }
coflow-exporter-core = { path = "../coflow-exporter-core" }
rmp = "0.8"

[dev-dependencies]
rmpv = "1"
```

- [ ] **Step 4: Implement MessagePack byte encoder**

Create `crates/coflow-exporter-messagepack/src/lib.rs`:

```rust
//! MessagePack exporter for validated Coflow data models.

#![cfg_attr(
    not(test),
    deny(
        clippy::dbg_macro,
        clippy::expect_used,
        clippy::panic,
        clippy::panic_in_result_fn,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable,
        clippy::unwrap_used
    )
)]

use coflow_cft::CftContainer;
use coflow_data_model::CfdDataModel;
use coflow_exporter_core::{export_model_with_encoder, ExportEncoder};
use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessagePackExportError {
    message: String,
}

impl MessagePackExportError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for MessagePackExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(f)
    }
}

impl std::error::Error for MessagePackExportError {}

pub fn export_messagepack_model(
    schema: &CftContainer,
    model: &CfdDataModel,
) -> Result<BTreeMap<String, Vec<u8>>, MessagePackExportError> {
    export_model_with_encoder(schema, model, &mut MessagePackEncoder)
        .map_err(|err| MessagePackExportError::new(format!("failed to export MessagePack model: {err}")))
}

#[derive(Debug, Default)]
struct MessagePackEncoder;

impl ExportEncoder for MessagePackEncoder {
    type Value = Vec<u8>;
    type Error = MessagePackExportError;

    fn null(&mut self) -> Result<Self::Value, Self::Error> {
        let mut out = Vec::new();
        rmp::encode::write_nil(&mut out).map_err(encode_error)?;
        Ok(out)
    }

    fn bool(&mut self, value: bool) -> Result<Self::Value, Self::Error> {
        let mut out = Vec::new();
        rmp::encode::write_bool(&mut out, value).map_err(encode_error)?;
        Ok(out)
    }

    fn int(&mut self, value: i64) -> Result<Self::Value, Self::Error> {
        let mut out = Vec::new();
        rmp::encode::write_sint(&mut out, value).map_err(encode_error)?;
        Ok(out)
    }

    fn float(&mut self, value: f64) -> Result<Self::Value, Self::Error> {
        let mut out = Vec::new();
        rmp::encode::write_f64(&mut out, value).map_err(encode_error)?;
        Ok(out)
    }

    fn string(&mut self, value: &str) -> Result<Self::Value, Self::Error> {
        let mut out = Vec::new();
        rmp::encode::write_str(&mut out, value).map_err(encode_error)?;
        Ok(out)
    }

    fn array(&mut self, items: Vec<Self::Value>) -> Result<Self::Value, Self::Error> {
        let mut out = Vec::new();
        rmp::encode::write_array_len(&mut out, len_to_u32(items.len())?).map_err(encode_error)?;
        for item in items {
            out.extend(item);
        }
        Ok(out)
    }

    fn map(&mut self, entries: Vec<(String, Self::Value)>) -> Result<Self::Value, Self::Error> {
        let mut out = Vec::new();
        rmp::encode::write_map_len(&mut out, len_to_u32(entries.len())?).map_err(encode_error)?;
        for (key, value) in entries {
            rmp::encode::write_str(&mut out, &key).map_err(encode_error)?;
            out.extend(value);
        }
        Ok(out)
    }
}

fn len_to_u32(len: usize) -> Result<u32, MessagePackExportError> {
    u32::try_from(len)
        .map_err(|_| MessagePackExportError::new(format!("MessagePack collection too large: {len}")))
}

fn encode_error(err: impl fmt::Display) -> MessagePackExportError {
    MessagePackExportError::new(err.to_string())
}
```

- [ ] **Step 5: Run MessagePack tests**

Run:

```powershell
cargo test -p coflow-exporter-messagepack
```

Expected: PASS.

- [ ] **Step 6: Commit**

Run:

```powershell
git add Cargo.toml Cargo.lock crates/coflow-exporter-messagepack
git commit -m "feat: add messagepack exporter"
```

Expected: commit succeeds.

---

### Task 5: Add CLI MessagePack Export

**Files:**

- Modify: `Cargo.toml`
- Modify: `src/main.rs`
- Modify: `tests/cli.rs`
- Modify: `Cargo.lock`

- [ ] **Step 1: Write failing CLI tests**

Add to `tests/cli.rs`:

```rust
#[test]
fn export_messagepack_writes_msgpack_tables() {
    let root_dir = std::env::temp_dir().join(format!(
        "coflow-messagepack-export-test-{}",
        std::process::id()
    ));
    let project_dir = root_dir.join("project");
    let out_dir = root_dir.join("out");
    if root_dir.exists() {
        std::fs::remove_dir_all(&root_dir).expect("clean old output dir");
    }
    copy_dir_recursive(std::path::Path::new("examples/rpg"), &project_dir);
    let config_path = project_dir.join("coflow.yaml");
    let config = std::fs::read_to_string(&config_path)
        .expect("read copied coflow.yaml")
        .replace("type: json", "type: messagepack");
    std::fs::write(&config_path, config).expect("write messagepack coflow.yaml");

    let output = coflow()
        .args([
            "export",
            "messagepack",
            project_dir.to_str().expect("utf8 temp path"),
            "--out",
            out_dir.to_str().expect("utf8 temp path"),
        ])
        .output()
        .expect("run coflow");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("MessagePack data exported to"),
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(out_dir.join("Item.msgpack").is_file());
    assert!(out_dir.join("DropTable.msgpack").is_file());
    std::fs::remove_dir_all(root_dir).expect("clean output dir");
}

#[test]
fn export_messagepack_validates_declared_output_type() {
    let out_dir = std::env::temp_dir().join(format!(
        "coflow-messagepack-type-test-{}",
        std::process::id()
    ));
    if out_dir.exists() {
        std::fs::remove_dir_all(&out_dir).expect("clean old output dir");
    }

    let output = coflow()
        .args([
            "export",
            "messagepack",
            "examples/rpg",
            "--out",
            out_dir.to_str().expect("utf8 temp path"),
        ])
        .output()
        .expect("run coflow");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("required `messagepack` for `coflow export messagepack`"),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn copy_dir_recursive(source: &std::path::Path, target: &std::path::Path) {
    std::fs::create_dir_all(target).expect("create target dir");
    for entry in std::fs::read_dir(source).expect("read source dir") {
        let entry = entry.expect("source entry");
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        if source_path.is_dir() {
            copy_dir_recursive(&source_path, &target_path);
        } else {
            std::fs::copy(&source_path, &target_path).expect("copy file");
        }
    }
}
```

- [ ] **Step 2: Run tests to verify RED**

Run:

```powershell
cargo test --test cli export_messagepack
```

Expected: FAIL because `export messagepack` subcommand does not exist.

- [ ] **Step 3: Add root dependency**

Edit root `Cargo.toml` dependencies:

```toml
coflow-exporter-messagepack = { path = "crates/coflow-exporter-messagepack" }
```

- [ ] **Step 4: Add CLI command and function**

Edit `src/main.rs` imports:

```rust
use coflow_exporter_messagepack::export_messagepack_model;
```

Extend `run` export match:

```rust
Command::Export(command) => match &command.command {
    ExportCommand::Json(args) => export_json(args),
    ExportCommand::Messagepack(args) => export_messagepack(args),
},
```

Extend `ExportCommand`:

```rust
#[derive(Debug, Subcommand)]
enum ExportCommand {
    /// Export data as JSON. The project config must declare outputs.data.type: json.
    Json(ExportJsonArgs),
    /// Export data as MessagePack. The project config must declare outputs.data.type: messagepack.
    Messagepack(ExportMessagePackArgs),
}
```

Add args:

```rust
#[derive(Debug, Args)]
struct ExportMessagePackArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    config_or_dir: Option<PathBuf>,
    /// Override outputs.data.dir for this invocation.
    #[arg(long = "out", value_name = "DIR")]
    out_dir: Option<PathBuf>,
}
```

Add function:

```rust
fn export_messagepack(args: &ExportMessagePackArgs) -> Result<bool, String> {
    let project = Project::open(args.config_or_dir.as_deref())?;
    let output = project.config.outputs.data.as_ref().ok_or_else(|| {
        "coflow.yaml missing outputs.data; required `type: messagepack` and `dir` for `coflow export messagepack`"
            .to_string()
    })?;
    if output.output_type != "messagepack" {
        return Err(format!(
            "coflow.yaml outputs.data.type is `{}`; required `messagepack` for `coflow export messagepack`",
            output.output_type
        ));
    }
    let dir = args.out_dir.as_deref().map_or_else(
        || project.resolve_path(&output.dir),
        |path| project.resolve_path(path),
    );
    let build = compile_schema_project(&project, None)?;
    let cft_diagnostics = dedupe_cft_diagnostics(build.diagnostics);
    if !cft_diagnostics.is_empty() {
        write_human_cft_diagnostics(&cft_diagnostics, &build.sources, &build.paths)?;
        return Ok(false);
    }
    let Some(schema) = build.container else {
        return Err("schema compilation did not produce a container".to_string());
    };
    let sources = excel_sources(&project);
    let load_output = match load_excel(&schema, &sources) {
        Ok(output) => output,
        Err(err) => {
            write_human_excel_error(&err)?;
            return Ok(false);
        }
    };
    if let Some(checks) = load_output.check_diagnostics {
        write_human_excel_diagnostics(&checks)?;
        return Ok(false);
    }

    let tables = export_messagepack_model(&schema, &load_output.model)
        .map_err(|err| format!("failed to export MessagePack model: {err}"))?;
    fs::create_dir_all(&dir)
        .map_err(|err| format!("failed to create output dir `{}`: {err}", dir.display()))?;
    for (table, bytes) in tables {
        let path = dir.join(format!("{table}.msgpack"));
        fs::write(&path, bytes)
            .map_err(|err| format!("failed to write `{}`: {err}", path.display()))?;
    }
    println!("MessagePack data exported to {}", dir.display());
    Ok(true)
}
```

- [ ] **Step 5: Run CLI tests**

Run:

```powershell
cargo test --test cli export_messagepack
```

Expected: PASS.

- [ ] **Step 6: Commit**

Run:

```powershell
git add Cargo.toml Cargo.lock src/main.rs tests/cli.rs
git commit -m "feat: add messagepack export command"
```

Expected: commit succeeds.

---

### Task 6: Make C# Codegen Format-Aware

**Files:**

- Modify: `crates/coflow-codegen-csharp/src/lib.rs`
- Modify: `crates/coflow-codegen-csharp/src/ir.rs`
- Modify: `crates/coflow-codegen-csharp/src/model.rs`
- Modify: `crates/coflow-codegen-csharp/src/emit.rs`
- Modify: `crates/coflow-codegen-csharp/src/render.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write failing unit test for format selection**

Add to `crates/coflow-codegen-csharp/src/lib.rs` tests:

```rust
#[test]
fn codegen_messagepack_uses_msgpack_loader_template() -> Result<(), String> {
    let schema = compile_schema("type Item { @id id: string; value: int; }")?;
    let files = generate_csharp(
        &schema,
        &CsharpCodegenOptions::new("Game.Config")
            .with_data_format(CsharpDataFormat::MessagePack),
    )
    .map_err(|err| err.to_string())?;
    let database = generated_file(&files, "GameConfig.cs")?;

    require_contains(database, "using MessagePack;")?;
    require_contains(database, "Path.Combine(dataDir, \"Item.msgpack\")")?;
    require_not_contains(database, "Newtonsoft.Json")?;
    Ok(())
}
```

- [ ] **Step 2: Run test to verify RED**

Run:

```powershell
cargo test -p coflow-codegen-csharp codegen_messagepack_uses_msgpack_loader_template
```

Expected: FAIL because `generate_csharp` and `CsharpDataFormat` do not exist.

- [ ] **Step 3: Add data format option**

Edit `crates/coflow-codegen-csharp/src/ir.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CsharpDataFormat {
    Json,
    MessagePack,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsharpCodegenOptions {
    pub namespace: String,
    pub database_class: String,
    pub data_format: CsharpDataFormat,
}

impl CsharpCodegenOptions {
    #[must_use]
    pub fn new(namespace: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            database_class: "GameConfig".to_string(),
            data_format: CsharpDataFormat::Json,
        }
    }

    #[must_use]
    pub fn with_database_class(mut self, database_class: impl Into<String>) -> Self {
        self.database_class = database_class.into();
        self
    }

    #[must_use]
    pub fn with_data_format(mut self, data_format: CsharpDataFormat) -> Self {
        self.data_format = data_format;
        self
    }
}
```

Edit `build_project` return:

```rust
Ok(CsharpProject {
    namespace: options.namespace.clone(),
    database_class: options.database_class.clone(),
    data_format: options.data_format,
    enums,
    types,
    database,
})
```

Edit `crates/coflow-codegen-csharp/src/model.rs`:

```rust
use crate::ir::CsharpDataFormat;

#[derive(Debug, Serialize)]
pub struct CsharpProject {
    pub namespace: String,
    pub database_class: String,
    pub data_format: CsharpDataFormat,
    pub enums: Vec<CsharpEnum>,
    pub types: Vec<CsharpType>,
    pub database: CsharpDatabase,
}
```

Add derive to `CsharpDataFormat`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum CsharpDataFormat {
    Json,
    MessagePack,
}
```

- [ ] **Step 4: Add public format-aware generator while preserving old API**

Edit `crates/coflow-codegen-csharp/src/lib.rs` exports:

```rust
pub use ir::{CsharpCodegenOptions, CsharpDataFormat};
```

Add:

```rust
pub fn generate_csharp(
    schema: &CftContainer,
    options: &CsharpCodegenOptions,
) -> Result<Vec<GeneratedFile>, CsharpCodegenError> {
    let project = ir::build_project(schema, options)?;
    render::render_project(&project)
}
```

Change existing JSON function:

```rust
pub fn generate_csharp_json(
    schema: &CftContainer,
    options: &CsharpCodegenOptions,
) -> Result<Vec<GeneratedFile>, CsharpCodegenError> {
    generate_csharp(schema, &options.clone().with_data_format(CsharpDataFormat::Json))
}
```

- [ ] **Step 5: Choose template in render**

Edit `crates/coflow-codegen-csharp/src/render.rs`:

```rust
const DATABASE_JSON_TEMPLATE: &str = include_str!("../templates/database.cs.tera");
const DATABASE_MESSAGEPACK_TEMPLATE: &str =
    include_str!("../templates/database_messagepack.cs.tera");
```

In `templates()` add:

```rust
tera.add_raw_template("database_json.cs.tera", DATABASE_JSON_TEMPLATE)
    .map_err(|err| CsharpCodegenError::new(format!("failed to add JSON database template: {err}")))?;
tera.add_raw_template("database_messagepack.cs.tera", DATABASE_MESSAGEPACK_TEMPLATE)
    .map_err(|err| CsharpCodegenError::new(format!("failed to add MessagePack database template: {err}")))?;
```

Select template:

```rust
let database_template = match project.data_format {
    crate::ir::CsharpDataFormat::Json => "database_json.cs.tera",
    crate::ir::CsharpDataFormat::MessagePack => "database_messagepack.cs.tera",
};
```

Use `database_template` when rendering `GameConfig.cs`.

- [ ] **Step 6: Add temporary MessagePack template**

Create `crates/coflow-codegen-csharp/templates/database_messagepack.cs.tera` with minimal compile-shaped content:

```csharp
// <auto-generated />
#nullable enable
using System;
using System.Collections.Generic;
using System.IO;
using MessagePack;

namespace {{ project.namespace }};

public partial class {{ project.database_class }}
{
{% for table in project.database.tables %}    public IReadOnlyList<{{ table.name }}> {{ table.list_property }} { get; }
{% endfor %}
    private {{ project.database_class }}(
{% for parameter in project.database.constructor_parameters %}        {{ parameter.ty }} {{ parameter.name }}{% if not loop.last %},{% endif %}
{% endfor %}    )
    {
{% for table in project.database.tables %}        {{ table.list_property }} = {{ table.list_var }};
{% endfor %}    }

    public static {{ project.database_class }} Load(string dataDir)
    {
{% for step in project.database.load_steps %}        {{ step }}
{% endfor %}
        return new {{ project.database_class }}(
{% for arg in project.database.constructor_args %}            {{ arg }}{% if not loop.last %},{% endif %}
{% endfor %}        );
    }
}
```

This template makes the RED test pass for format selection; Task 7 replaces it with the real loader.

- [ ] **Step 7: Update load file extension for MessagePack**

Edit `crates/coflow-codegen-csharp/src/emit.rs` signature:

```rust
pub fn build_csharp_database(
    view: &SchemaView,
    tables: &[String],
    _database_class: &str,
    data_format: CsharpDataFormat,
) -> Result<CsharpDatabase, CsharpCodegenError>
```

Use:

```rust
let table_extension = match data_format {
    CsharpDataFormat::Json => "json",
    CsharpDataFormat::MessagePack => "msgpack",
};
```

Keep the existing `LoadTable(Path.Combine(...), ..., LoadType)` step and substitute the extension.

Edit `ir.rs` call:

```rust
let database = build_csharp_database(&view, &tables, &options.database_class, options.data_format)?;
```

- [ ] **Step 8: Read data output type in CLI codegen**

Edit `src/main.rs` import:

```rust
use coflow_codegen_csharp::{generate_csharp, CsharpCodegenOptions, CsharpDataFormat};
```

In `codegen_csharp`, before options:

```rust
let data_output = project.config.outputs.data.as_ref().ok_or_else(|| {
    "coflow.yaml missing outputs.data; required `type: json` or `type: messagepack` for `coflow codegen csharp`"
        .to_string()
})?;
let data_format = match data_output.output_type.as_str() {
    "json" => CsharpDataFormat::Json,
    "messagepack" => CsharpDataFormat::MessagePack,
    other => {
        return Err(format!(
            "coflow.yaml outputs.data.type is `{other}`; required `json` or `messagepack` for `coflow codegen csharp`"
        ))
    }
};
```

Build options:

```rust
let options = CsharpCodegenOptions::new(namespace).with_data_format(data_format);
let files = generate_csharp(&schema, &options)
    .map_err(|err| format!("failed to generate C# code: {err}"))?;
```

- [ ] **Step 9: Run format selection tests**

Run:

```powershell
cargo test -p coflow-codegen-csharp codegen_messagepack_uses_msgpack_loader_template
cargo test --test cli codegen_csharp_writes_newtonsoft_json_loader
```

Expected: PASS.

- [ ] **Step 10: Commit**

Run:

```powershell
git add src/main.rs crates/coflow-codegen-csharp
git commit -m "feat: make csharp codegen data-format aware"
```

Expected: commit succeeds.

---

### Task 7: Generate Real MessagePack C# Loader

**Files:**

- Modify: `crates/coflow-codegen-csharp/src/model.rs`
- Modify: `crates/coflow-codegen-csharp/src/emit.rs`
- Modify: `crates/coflow-codegen-csharp/templates/database_messagepack.cs.tera`
- Modify: `crates/coflow-codegen-csharp/src/lib.rs`

- [ ] **Step 1: Write failing generation tests for MessagePack loader contents**

Add to `crates/coflow-codegen-csharp/src/lib.rs` tests:

```rust
#[test]
fn codegen_messagepack_emits_explicit_readers_type_dispatch_and_ref_resolution() -> Result<(), String> {
    let schema = compile_schema(
        r#"
            type Item { @id id: string; }
            abstract type Reward { id: string; }
            type ItemReward : Reward {
                @ref(Item)
                item_id: string;
                count: int = 1;
            }
            type DropTable {
                @id id: string;
                rewards: [Reward];
            }
        "#,
    )?;

    let files = generate_csharp(
        &schema,
        &CsharpCodegenOptions::new("Game.Config")
            .with_data_format(CsharpDataFormat::MessagePack),
    )
    .map_err(|err| err.to_string())?;
    let database = generated_file(&files, "GameConfig.cs")?;

    require_contains(database, "using MessagePack;")?;
    require_contains(database, "private delegate T MessagePackRowLoader<T>(")?;
    require_contains(database, "private static Item LoadItem(ref MessagePackReader reader, string path)")?;
    require_contains(database, "reader.ReadMapHeader()")?;
    require_contains(database, "case \"item_id\":")?;
    require_contains(database, "reader.Skip()")?;
    require_contains(database, "LoadRewardPolymorphic(ref reader, path)")?;
    require_contains(database, "ResolveRef(itemRefIndex")?;
    require_not_contains(database, "Newtonsoft.Json")?;
    Ok(())
}
```

- [ ] **Step 2: Run test to verify RED**

Run:

```powershell
cargo test -p coflow-codegen-csharp codegen_messagepack_emits_explicit_readers_type_dispatch_and_ref_resolution
```

Expected: FAIL because the temporary MessagePack template has no reader implementation.

- [ ] **Step 3: Extend loader model with source names and defaults**

Edit `crates/coflow-codegen-csharp/src/model.rs`:

```rust
#[derive(Debug, Serialize)]
pub struct CsharpLoadField {
    pub property: String,
    pub source_name: String,
    pub local_name: String,
    pub type_name: String,
    pub read_expr: String,
    pub messagepack_read_expr: String,
    pub default_expr: Option<String>,
    pub is_required: bool,
}
```

- [ ] **Step 4: Populate loader field metadata**

Edit `loader_methods` in `crates/coflow-codegen-csharp/src/emit.rs` so each `CsharpLoadField` is created as:

```rust
CsharpLoadField {
    property: pascal_case(&field.name),
    source_name: field.name.clone(),
    local_name: camel_case(&field.name),
    type_name: csharp_type(&field.ty),
    read_expr: read_field_expr(field, "obj", "path", view)?,
    messagepack_read_expr: read_messagepack_expr(&field.ty, "reader", "$\"{path}.FIELD_NAME\"", view)?
        .replace("FIELD_NAME", &field.name),
    default_expr: default_value_expr(field.default.as_ref(), &field.ty, view)?,
    is_required: field.default.is_none() && !field.has_default,
}
```

If a local name collides with a C# keyword, reuse existing generated-name validation and return `CsharpCodegenError`.

- [ ] **Step 5: Add MessagePack read expression builder**

Add in `emit.rs`:

```rust
fn read_messagepack_expr(
    ty: &FieldType,
    reader: &str,
    path: &str,
    view: &SchemaView,
) -> Result<String, CsharpCodegenError> {
    match ty {
        FieldType::Int => Ok(format!("ReadInt(ref {reader}, {path})")),
        FieldType::Float => Ok(format!("ReadFloat(ref {reader}, {path})")),
        FieldType::Bool => Ok(format!("ReadBool(ref {reader}, {path})")),
        FieldType::String => Ok(format!("ReadString(ref {reader}, {path})")),
        FieldType::Enum(name) => Ok(format!("ReadEnum<{name}>(ref {reader}, {path})")),
        FieldType::Type(name) if view.range_is_polymorphic(name) => {
            Ok(format!("Load{name}Polymorphic(ref {reader}, {path})"))
        }
        FieldType::Type(name) => Ok(format!("Load{name}(ref {reader}, {path})")),
        FieldType::Array(inner) => Ok(format!(
            "ReadArray(ref {reader}, {path}, static (ref MessagePackReader itemReader, string itemPath) => {})",
            read_messagepack_expr(inner, "itemReader", "itemPath", view)?
        )),
        FieldType::Dict(key, value) => Ok(format!(
            "ReadDict(ref {reader}, {path}, static (key, keyPath) => {}, static (ref MessagePackReader valueReader, string valuePath) => {})",
            read_messagepack_key_expr(key, "key", "keyPath")?,
            read_messagepack_expr(value, "valueReader", "valuePath", view)?
        )),
        FieldType::Nullable(inner) => Ok(format!(
            "{reader}.TryReadNil() ? null : {}",
            read_messagepack_expr(inner, reader, path, view)?
        )),
    }
}
```

Add:

```rust
fn read_messagepack_key_expr(
    ty: &FieldType,
    key: &str,
    path: &str,
) -> Result<String, CsharpCodegenError> {
    match ty.non_nullable() {
        FieldType::String => Ok(key.to_string()),
        FieldType::Int => Ok(format!("ReadIntKey({key}, {path})")),
        FieldType::Enum(name) => Ok(format!("ReadEnumKey<{name}>({key}, {path})")),
        _ => Err(CsharpCodegenError::new(
            "dictionary key type must be string, int, or enum",
        )),
    }
}
```

The field model stores both JSON and MessagePack read expressions. JSON templates continue using `field.read_expr`; MessagePack templates use `field.messagepack_read_expr`.

- [ ] **Step 6: Replace MessagePack template with real loader**

Edit `crates/coflow-codegen-csharp/templates/database_messagepack.cs.tera` to include:

```csharp
// <auto-generated />
#nullable enable
using System;
using System.Buffers;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using MessagePack;

namespace {{ project.namespace }};

public partial class {{ project.database_class }}
{
{% for table in project.database.tables %}    public IReadOnlyList<{{ table.name }}> {{ table.list_property }} { get; }
{% endfor %}{% for table in project.database.tables %}    private readonly Dictionary<{{ table.id_type }}, {{ table.name }}> {{ table.index_field }};
{% endfor %}{% for ref_index in project.database.ref_indexes %}    private readonly Dictionary<{{ ref_index.target_id_type }}, {{ ref_index.target_name }}> {{ ref_index.index_field }};
{% endfor %}{% for index in project.database.indexes %}    private readonly Dictionary<{{ index.key_type }}, List<{{ index.table_name }}>> {{ index.storage_field }};
{% endfor %}
    private delegate T MessagePackRowLoader<T>(ref MessagePackReader reader, string path);

    private {{ project.database_class }}(
{% for parameter in project.database.constructor_parameters %}        {{ parameter.ty }} {{ parameter.name }}{% if not loop.last %},{% endif %}
{% endfor %}    )
    {
{% for table in project.database.tables %}        {{ table.list_property }} = {{ table.list_var }};
        {{ table.index_field }} = {{ table.index_var }};

{% endfor %}{% for ref_index in project.database.ref_indexes %}        {{ ref_index.index_field }} = {{ ref_index.parameter_name }};
{% endfor %}{% for index in project.database.indexes %}        {{ index.storage_field }} = {{ index.parameter_name }};
{% endfor %}    }

{% for table in project.database.tables %}    public {{ table.name }}? Find{{ table.name }}({{ table.id_type }} id) =>
        {{ table.index_field }}.TryGetValue(id, out var value) ? value : null;

{% endfor %}{% for index in project.database.indexes %}    public IReadOnlyList<{{ index.table_name }}> Get{{ index.list_property }}By{{ index.field_property }}({{ index.key_type }} value) =>
        {{ index.storage_field }}.TryGetValue(value, out var list) ? list : Array.Empty<{{ index.table_name }}>();

{% endfor %}    public static {{ project.database_class }} Load(string dataDir)
    {
{% for step in project.database.load_steps %}        {{ step }}
{% endfor %}
        return new {{ project.database_class }}(
{% for arg in project.database.constructor_args %}            {{ arg }}{% if not loop.last %},{% endif %}
{% endfor %}        );
    }

{% for loader in project.database.loaders %}    private static {{ loader.type_name }} Load{{ loader.type_name }}(ref MessagePackReader reader, string path)
    {
        var count = reader.ReadMapHeader();
{% for field in loader.fields %}        var has{{ field.property }} = false;
        {{ field.type_name }} {{ field.local_name }}{% if field.default_expr %} = {{ field.default_expr }}{% else %} = default!{% endif %};
{% endfor %}
        for (var i = 0; i < count; i++)
        {
            var key = reader.ReadString();
            switch (key)
            {
{% for field in loader.fields %}                case "{{ field.source_name }}":
                    if (has{{ field.property }})
                    {
                        throw new CftLoadException($"duplicate field `{{ field.source_name }}`", $"{path}.{{ field.source_name }}", "unique field", "{{ field.source_name }}");
                    }
                    {{ field.local_name }} = {{ field.read_expr }};
                    has{{ field.property }} = true;
                    break;
{% endfor %}                default:
                    reader.Skip();
                    break;
            }
        }
{% for field in loader.fields %}{% if field.is_required %}        if (!has{{ field.property }})
        {
            throw new CftLoadException($"missing required field `{{ field.source_name }}`", $"{path}.{{ field.source_name }}", "present property", "missing");
        }
{% endif %}{% endfor %}
        return new {{ loader.type_name }} {
{% for field in loader.fields %}            {{ field.property }} = {{ field.local_name }},
{% endfor %}        };
    }

{% endfor %}
    // Keep existing BuildUniqueIndex, BuildMultiIndex, BuildRefIndex, ResolveRefs,
    // ResolveRef helpers from the JSON template with JToken-specific helpers removed.
}
```

The MessagePack template must include the same non-JSON-specific helper sections as `database.cs.tera`: constructor/index API, `ResolveRefs`, `BuildUniqueIndex`, `BuildMultiIndex`, `RefIndexSource`, `BuildRefIndex`, and `ResolveRef`. Do not call any `JToken`, `JObject`, `JArray`, or `Newtonsoft.Json` helper from the MessagePack template.

Add MessagePack helpers:

```csharp
private static List<T> LoadTable<T>(
    string file,
    string tableName,
    MessagePackRowLoader<T> loadRow)
{
    var bytes = File.ReadAllBytes(file);
    var reader = new MessagePackReader(new ReadOnlySequence<byte>(bytes));
    var count = reader.ReadArrayHeader();
    var result = new List<T>();
    for (var i = 0; i < count; i++)
    {
        result.Add(loadRow(ref reader, $"{tableName}[{i}]"));
    }
    return result;
}
```

Add scalar helpers:

```csharp
private static string ReadString(ref MessagePackReader reader, string path) =>
    reader.ReadString() ?? "";

private static long ReadInt(ref MessagePackReader reader, string path) =>
    reader.ReadInt64();

private static float ReadFloat(ref MessagePackReader reader, string path) =>
    (float)reader.ReadDouble();

private static bool ReadBool(ref MessagePackReader reader, string path) =>
    reader.ReadBoolean();

private static TEnum ReadEnum<TEnum>(ref MessagePackReader reader, string path)
    where TEnum : struct, Enum =>
    (TEnum)Enum.ToObject(typeof(TEnum), ReadInt(ref reader, path));
```

Add collection helpers:

```csharp
private delegate T MessagePackItemReader<T>(ref MessagePackReader reader, string path);
private delegate TKey MessagePackKeyReader<TKey>(string key, string path);

private static List<T> ReadArray<T>(
    ref MessagePackReader reader,
    string path,
    MessagePackItemReader<T> readItem)
{
    var count = reader.ReadArrayHeader();
    var result = new List<T>();
    for (var i = 0; i < count; i++)
    {
        result.Add(readItem(ref reader, $"{path}[{i}]"));
    }
    return result;
}

private static Dictionary<TKey, TValue> ReadDict<TKey, TValue>(
    ref MessagePackReader reader,
    string path,
    MessagePackKeyReader<TKey> readKey,
    MessagePackItemReader<TValue> readValue)
    where TKey : notnull
{
    var count = reader.ReadMapHeader();
    var result = new Dictionary<TKey, TValue>();
    for (var i = 0; i < count; i++)
    {
        var rawKey = reader.ReadString() ?? "";
        var keyPath = $"{path}.{rawKey}";
        var key = readKey(rawKey, keyPath);
        if (!result.TryAdd(key, readValue(ref reader, keyPath)))
        {
            throw new CftLoadException($"duplicate dictionary key `{rawKey}`", keyPath, "unique key", rawKey);
        }
    }
    return result;
}
```

Add key helpers:

```csharp
private static long ReadIntKey(string key, string path)
{
    if (long.TryParse(key, NumberStyles.Integer, CultureInfo.InvariantCulture, out var value))
    {
        return value;
    }
    throw new CftLoadException($"dictionary key `{key}` must be an integer", path, "integer key", key);
}

private static TEnum ReadEnumKey<TEnum>(string key, string path)
    where TEnum : struct, Enum =>
    (TEnum)Enum.ToObject(typeof(TEnum), ReadIntKey(key, path));
```

- [ ] **Step 7: Run MessagePack codegen tests**

Run:

```powershell
cargo test -p coflow-codegen-csharp codegen_messagepack
```

Expected: PASS.

- [ ] **Step 8: Run JSON codegen tests**

Run:

```powershell
cargo test -p coflow-codegen-csharp codegen_writes_newtonsoft_json_loader
cargo test -p coflow-codegen-csharp codegen_preserves_missing_field_default_and_nullable_required_semantics
```

Expected: PASS.

- [ ] **Step 9: Commit**

Run:

```powershell
git add crates/coflow-codegen-csharp
git commit -m "feat: generate messagepack csharp loader"
```

Expected: commit succeeds.

---

### Task 8: Add CLI Codegen Tests And MessagePack C# E2E

**Files:**

- Modify: `tests/cli.rs`

- [ ] **Step 1: Write failing CLI codegen tests**

Add to `tests/cli.rs`:

```rust
#[test]
fn codegen_csharp_writes_messagepack_loader_when_data_type_is_messagepack() {
    let root_dir = std::env::temp_dir().join(format!(
        "coflow-csharp-msgpack-codegen-test-{}",
        std::process::id()
    ));
    let project_dir = root_dir.join("project");
    let out_dir = root_dir.join("csharp");
    if root_dir.exists() {
        std::fs::remove_dir_all(&root_dir).expect("clean old output dir");
    }
    copy_dir_recursive(std::path::Path::new("examples/rpg"), &project_dir);
    let config_path = project_dir.join("coflow.yaml");
    let config = std::fs::read_to_string(&config_path)
        .expect("read copied coflow.yaml")
        .replace("type: json", "type: messagepack");
    std::fs::write(&config_path, config).expect("write messagepack coflow.yaml");

    let output = coflow()
        .args([
            "codegen",
            "csharp",
            project_dir.to_str().expect("utf8 temp path"),
            "--out",
            out_dir.to_str().expect("utf8 temp path"),
        ])
        .output()
        .expect("run coflow");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let game_config = std::fs::read_to_string(out_dir.join("GameConfig.cs"))
        .expect("GameConfig.cs should be written");
    assert!(game_config.contains("using MessagePack;"));
    assert!(game_config.contains("Item.msgpack"));
    assert!(!game_config.contains("Newtonsoft.Json"));
    std::fs::remove_dir_all(root_dir).expect("clean output dir");
}
```

- [ ] **Step 2: Run test**

Run:

```powershell
cargo test --test cli codegen_csharp_writes_messagepack_loader_when_data_type_is_messagepack
```

Expected: PASS if Task 6 and 7 are complete.

- [ ] **Step 3: Add optional .NET MessagePack e2e test**

Add to `tests/cli.rs`:

```rust
#[test]
fn generated_csharp_compiles_and_loads_exported_messagepack() {
    let suffix = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos()
    );
    let root_dir = std::env::temp_dir().join(format!("coflow-csharp-msgpack-e2e-test-{suffix}"));
    let project_dir = root_dir.join("project");
    let export_dir = root_dir.join("export");
    let csharp_dir = root_dir.join("csharp");
    let dotnet_dir = root_dir.join("dotnet");
    if root_dir.exists() {
        std::fs::remove_dir_all(&root_dir).expect("clean old output dir");
    }
    copy_dir_recursive(std::path::Path::new("examples/rpg"), &project_dir);
    let config_path = project_dir.join("coflow.yaml");
    let config = std::fs::read_to_string(&config_path)
        .expect("read copied coflow.yaml")
        .replace("type: json", "type: messagepack");
    std::fs::write(&config_path, config).expect("write messagepack coflow.yaml");

    let export_output = coflow()
        .args([
            "export",
            "messagepack",
            project_dir.to_str().expect("utf8 temp path"),
            "--out",
            export_dir.to_str().expect("utf8 temp path"),
        ])
        .output()
        .expect("run coflow export");
    assert!(
        export_output.status.success(),
        "export failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&export_output.stdout),
        String::from_utf8_lossy(&export_output.stderr)
    );

    let codegen_output = coflow()
        .args([
            "codegen",
            "csharp",
            project_dir.to_str().expect("utf8 temp path"),
            "--namespace",
            "Game.Config",
            "--out",
            csharp_dir.to_str().expect("utf8 temp path"),
        ])
        .output()
        .expect("run coflow codegen");
    assert!(
        codegen_output.status.success(),
        "codegen failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&codegen_output.stdout),
        String::from_utf8_lossy(&codegen_output.stderr)
    );

    let new_output = Command::new("dotnet")
        .args([
            "new",
            "console",
            "--framework",
            "net8.0",
            "--output",
            dotnet_dir.to_str().expect("utf8 temp path"),
        ])
        .output()
        .expect("run dotnet new");
    assert!(
        new_output.status.success(),
        "dotnet new failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&new_output.stdout),
        String::from_utf8_lossy(&new_output.stderr)
    );

    let add_package_output = Command::new("dotnet")
        .current_dir(&dotnet_dir)
        .args(["add", "package", "MessagePack"])
        .output()
        .expect("run dotnet add package");
    assert!(
        add_package_output.status.success(),
        "dotnet add package failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&add_package_output.stdout),
        String::from_utf8_lossy(&add_package_output.stderr)
    );

    for entry in std::fs::read_dir(&csharp_dir).expect("read generated C# dir") {
        let entry = entry.expect("generated C# entry");
        let path = entry.path();
        if path.extension().is_some_and(|extension| extension == "cs") {
            std::fs::copy(
                &path,
                dotnet_dir.join(path.file_name().expect("generated C# file name")),
            )
            .expect("copy generated C# file");
        }
    }

    std::fs::write(
        dotnet_dir.join("Program.cs"),
        r#"using Game.Config;

var config = GameConfig.Load(args[0]);
if (config.Items.Count == 0)
{
    throw new Exception("expected items");
}
Console.WriteLine("loaded");
"#,
    )
    .expect("write Program.cs");

    let build_output = Command::new("dotnet")
        .current_dir(&dotnet_dir)
        .arg("build")
        .output()
        .expect("run dotnet build");
    assert!(
        build_output.status.success(),
        "dotnet build failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&build_output.stdout),
        String::from_utf8_lossy(&build_output.stderr)
    );

    let run_output = Command::new("dotnet")
        .current_dir(&dotnet_dir)
        .args(["run", "--", export_dir.to_str().expect("utf8 temp path")])
        .output()
        .expect("run dotnet app");
    assert!(
        run_output.status.success(),
        "dotnet run failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&run_output.stdout),
        String::from_utf8_lossy(&run_output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&run_output.stdout).contains("loaded"),
        "dotnet run stdout: {}",
        String::from_utf8_lossy(&run_output.stdout)
    );

    std::fs::remove_dir_all(root_dir).expect("clean output dir");
}
```

- [ ] **Step 4: Run CLI MessagePack tests**

Run:

```powershell
cargo test --test cli messagepack
```

Expected: PASS, including the .NET e2e test if `dotnet` and NuGet access are available.

- [ ] **Step 5: Commit**

Run:

```powershell
git add tests/cli.rs
git commit -m "test: cover messagepack cli and csharp loading"
```

Expected: commit succeeds.

---

### Task 9: Update Specs And Project Documentation

**Files:**

- Create: `docs/spec/08-messagepack-export.md`
- Modify: `docs/spec/05-json-export.md`
- Modify: `docs/spec/06-csharp-codegen.md`
- Modify: `docs/spec/07-project-pipeline.md`
- Modify: `docs/superpowers/specs/2026-06-09-messagepack-support-design.md`

- [ ] **Step 1: Add MessagePack export spec**

Create `docs/spec/08-messagepack-export.md`:

```markdown
# MessagePack 导出格式

**依赖文档**：[02-data-model.md](02-data-model.md)

MessagePack 导出是 JSON 导出的二进制等价格式。输入仍是已经通过 schema、Excel 加载、DataModel 构建和检查的 `CfdDataModel`。

## 文件结构

导出目录中每个 table 写出一个 `<TypeName>.msgpack` 文件：

```text
out/
  Item.msgpack
  Monster.msgpack
  DropTable.msgpack
```

每个文件内容是裸 MessagePack array。array 中每个元素是 record map。

## 编码规则

| CFT 类型 | MessagePack 表示 |
| --- | --- |
| `int` | integer |
| `float` | float64 |
| `bool` | boolean |
| `string` | string |
| nullable null | nil |
| enum | integer 底层值 |
| type | map |
| 多态 type | 带 `$type` string 字段的 map |
| `[T]` | array |
| `{K: V}` | map，key 统一为 string |
| `@ref` 字段 | 原始 ID 值 |

本版本不包含文件头、manifest、schema hash、加密、完整性校验或压缩。

## 字段顺序

record map 的字段顺序按 schema 的继承展开顺序输出：父类字段先于子类字段。多态对象需要 `$type` 时，`$type` 位于 map 的第一个 entry。
```

- [ ] **Step 2: Update existing specs**

In `docs/spec/05-json-export.md`, add a short note:

```markdown
JSON 导出实现位于 `coflow-exporter-json`，与 MessagePack 导出共用 exporter core 的 schema-aware 遍历规则。
```

In `docs/spec/06-csharp-codegen.md`, update loader section:

```markdown
`coflow codegen csharp` 根据 `outputs.data.type` 生成匹配的运行时加载器：

- `json`：生成 Newtonsoft.Json loader，读取 `<TypeName>.json`。
- `messagepack`：生成 MessagePack-CSharp loader，读取 `<TypeName>.msgpack`，使用显式 `MessagePackReader` 读取方法以兼容 Unity/IL2CPP/AOT。

`outputs.code` 不提供独立的 data format override。
```

In `docs/spec/07-project-pipeline.md`, update responsibilities:

```markdown
- Orchestrate CLI commands, including JSON export, MessagePack export, and C# codegen invocation.
```

- [ ] **Step 3: Run doc grep**

Run:

```powershell
rg "coflow-json-export|coflow-excel-loader|export json|MessagePack" docs crates tests Cargo.toml
```

Expected: old crate names appear only in historical design docs or migration notes. New docs mention `coflow-exporter-json`, `coflow-loader-excel`, and MessagePack support.

- [ ] **Step 4: Commit**

Run:

```powershell
git add docs/spec/05-json-export.md docs/spec/06-csharp-codegen.md docs/spec/07-project-pipeline.md docs/spec/08-messagepack-export.md docs/superpowers/specs/2026-06-09-messagepack-support-design.md
git commit -m "docs: document messagepack export support"
```

Expected: commit succeeds.

---

### Task 10: Final Verification

**Files:**

- No planned source edits.

- [ ] **Step 1: Format entire workspace**

Run:

```powershell
cargo fmt
```

Expected: no formatting errors.

- [ ] **Step 2: Run clippy**

Run:

```powershell
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 3: Run full test suite**

Run:

```powershell
cargo test --workspace
```

Expected: PASS. If the MessagePack .NET e2e test fails because NuGet is unreachable, record the exact network error and run all non-network Rust tests plus `cargo test -p coflow-codegen-csharp codegen_messagepack`.

- [ ] **Step 4: Inspect Git state**

Run:

```powershell
git status --short
```

Expected: only intentionally uncommitted user-local files remain, especially `.claude/settings.local.json`.

- [ ] **Step 5: Commit final fixes if verification required source changes**

Run only if Step 1-3 required edits:

```powershell
git add <changed-files>
git commit -m "fix: complete messagepack verification"
```

Expected: commit succeeds when files were changed. If no files changed, skip this step.
