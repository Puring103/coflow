# AI Data CLI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build AI-friendly schema and data CLI commands that let an agent inspect Coflow schema/data, apply batch patches, and receive structured diagnostics.

**Architecture:** Put reusable schema inspection, data read, and data patch semantics in `coflow-engine`; keep the root CLI as a thin parser/output layer. The first version is a short-lived batch CLI, not a daemon, and it reuses existing `ProjectSession::write_field`, `insert_record`, and `delete_record` provider dispatch.

**Tech Stack:** Rust 2021, `serde`/`serde_json`, `clap`, existing `coflow-engine`, `coflow-api`, `coflow-project`, and default provider registry from `coflow-builtins`.

---

## Scope

Build these commands:

```powershell
coflow schema inspect [CONFIG_OR_DIR] [--type TYPE] [--include-derived] [--human]
coflow schema files [CONFIG_OR_DIR] [--human]

coflow data sources [CONFIG_OR_DIR] [--human]
coflow data list [CONFIG_OR_DIR] [--type TYPE] [--file FILE] [--limit N] [--offset N] [--human]
coflow data get [CONFIG_OR_DIR] [TYPE.KEY] [--type TYPE] [--file FILE] [--keys a,b] [--limit N] [--offset N] [--all] [--human]
coflow data patch [CONFIG_OR_DIR] --patch JSON [--human]
```

Default output for the new `schema` and `data` commands is JSON. `--human` opts into text output. `CONFIG_OR_DIR` is optional and follows existing Coflow project resolution.

Out of scope for this first implementation:

- Transaction rollback.
- `data serve --stdio`.
- MCP server.
- Editor migration to the patch API.
- Dict-key field writes.
- Shell sugar commands such as `data set`, `data insert`, and `data delete`.

## File Structure

Create focused engine modules:

- Create `crates/coflow-engine/src/schema_inspect.rs`
  - Engine-owned serializable schema view for AI/tooling.
  - Includes annotations on consts, types, fields, enums, and enum variants where present.
  - Includes raw CFT module source output for `schema files`.

- Create `crates/coflow-engine/src/data_read.rs`
  - Engine-owned source/index/get reports and query structs.
  - Converts `ProjectSession` record/source indexes into stable JSON shapes.
  - Enforces default batch limits for `data get`.

- Create `crates/coflow-engine/src/data_patch.rs`
  - Engine-owned patch request/op/report types.
  - Converts schema-guided `serde_json::Value` patch values into `CfdValue`.
  - Applies ops through existing write APIs.

- Modify `crates/coflow-engine/src/lib.rs`
  - Add the new modules and public re-exports.
  - Add small `ProjectSession` methods that delegate to the modules.

Add CLI wrapper modules:

- Create `src/schema_commands.rs`
  - Open schema-only project sessions.
  - Call engine schema inspection functions.
  - Write JSON or human text.

- Create `src/data_commands.rs`
  - Open full project sessions with the default provider registry.
  - Call engine data read/patch functions.
  - Write JSON or human text.

- Modify `src/lib.rs`
  - Export `schema_commands` and `data_commands`.

- Modify `src/main.rs`
  - Add `schema` and `data` command trees.
  - Route to new command modules.

Tests and docs:

- Create `crates/coflow-engine/tests/schema_inspect.rs`
- Create `crates/coflow-engine/tests/data_read.rs`
- Create `crates/coflow-engine/tests/data_patch.rs`
- Create `tests/cli_ai_data.rs`
- Modify `docs/spec/09-cli.md`
- Modify `README.md`

## Behavioral Decisions

Patch write semantics:

- `insert_record.file` is required.
- `set_field.file` and `delete_record.file` are optional guards.
- Write/preflight errors stop the batch when `stop_on_write_error` is true.
- CFT `check {}` diagnostics do not block writes.
- Data model or load failures that prevent addressing records are write/report failures for that op or final rebuild diagnostics, not silent success.
- The CLI exits non-zero when write fails or final diagnostics contain errors.

Patch JSON:

```json
{
  "check_after_write": true,
  "stop_on_write_error": true,
  "ops": [
    {
      "op": "insert_record",
      "file": "data/items.cfd",
      "type": "Item",
      "key": "steel_sword",
      "fields": {
        "name": "Steel Sword",
        "price": 250
      }
    },
    {
      "op": "set_field",
      "record": { "type": "Item", "key": "steel_sword" },
      "path": ["rarity"],
      "value": "Rare"
    }
  ]
}
```

Special patch value forms:

```json
{ "$ref": "Item.sword_01" }
{ "$ref": { "type": "Item", "key": "sword_01" } }
{ "$type": "ItemReward", "item": { "$ref": "Item.sword_01" }, "count": 1 }
{ "$dict": [{ "key": "Fire", "value": 10 }] }
```

Plain JSON values are converted using the expected CFT type:

- String to `string`.
- String to enum variant when expected type is enum.
- Number to `int` or `float`.
- Object to inline object when expected type is a named type.
- Object to string-key dict when expected type is `{string: T}`.
- `$ref` object to record reference when expected type is named type.

## Task 1: Engine Schema Inspection Types

**Files:**
- Create: `crates/coflow-engine/src/schema_inspect.rs`
- Modify: `crates/coflow-engine/src/lib.rs`
- Test: `crates/coflow-engine/tests/schema_inspect.rs`

- [ ] **Step 1: Write failing schema inspection tests**

Create `crates/coflow-engine/tests/schema_inspect.rs`:

```rust
#![allow(clippy::expect_used, clippy::panic)]

use coflow_engine::{build_project_schema_session, inspect_schema, schema_files};
use coflow_project::Project;

fn write_project(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        r#"
            @display("Item type")
            @idAsEnum(ItemId)
            type Item {
                @display("Display name")
                name: string;
                rarity: Rarity = Rarity.Common;
            }

            @display("Rarity enum")
            enum Rarity {
                @display("Common rarity")
                Common = 0,
                Rare = 10,
            }

            enum ItemId {}
        "#,
    )
    .expect("write schema");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema/\noutputs:\n  data:\n    type: json\n    dir: generated/data\n",
    )
    .expect("write config");
}

#[test]
fn inspect_schema_preserves_annotations_fields_and_enums() {
    let root = std::env::temp_dir().join(format!(
        "coflow-schema-inspect-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    write_project(&root);

    let project = Project::open_schema_only(Some(&root.join("coflow.yaml"))).expect("open");
    let session = build_project_schema_session(project).expect("schema session");
    let report = inspect_schema(&session, None, false);

    let item = report
        .types
        .iter()
        .find(|ty| ty.name == "Item")
        .expect("Item type");
    assert!(item.annotations.iter().any(|a| a.name == "display"));
    assert!(item.annotations.iter().any(|a| a.name == "idAsEnum"));
    assert!(item.fields.iter().any(|field| {
        field.name == "name" && field.annotations.iter().any(|a| a.name == "display")
    }));
    assert!(report.enums.iter().any(|e| {
        e.name == "Rarity"
            && e.annotations.iter().any(|a| a.name == "display")
            && e.variants.iter().any(|v| {
                v.name == "Common" && v.annotations.iter().any(|a| a.name == "display")
            })
    }));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn schema_files_returns_compiled_module_sources() {
    let root = std::env::temp_dir().join(format!(
        "coflow-schema-files-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    write_project(&root);

    let project = Project::open_schema_only(Some(&root.join("coflow.yaml"))).expect("open");
    let session = build_project_schema_session(project).expect("schema session");
    let files = schema_files(&session);

    assert_eq!(files.files.len(), 1);
    assert!(files.files[0].module.contains("schema/main.cft"));
    assert!(files.files[0].source.contains("type Item"));

    let _ = std::fs::remove_dir_all(root);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```powershell
cargo test -p coflow-engine --test schema_inspect
```

Expected: compile failure for missing `inspect_schema` and `schema_files`.

- [ ] **Step 3: Implement schema inspection module**

Create `crates/coflow-engine/src/schema_inspect.rs`:

```rust
use coflow_api::FlatDiagnostic;
use coflow_cft::{
    CftAnnotation, CftAnnotationValue, CftConstValue, CftSchemaDefaultValue, CftSchemaTypeRef,
};
use serde::Serialize;

use crate::ProjectSchemaSession;

#[derive(Debug, Clone, Serialize)]
pub struct SchemaInspectReport {
    pub types: Vec<SchemaTypeInfo>,
    pub enums: Vec<SchemaEnumInfo>,
    pub consts: Vec<SchemaConstInfo>,
    pub diagnostics: Vec<FlatDiagnostic>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaFilesReport {
    pub files: Vec<SchemaFileInfo>,
    pub diagnostics: Vec<FlatDiagnostic>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaFileInfo {
    pub module: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaTypeInfo {
    pub module: String,
    pub name: String,
    pub parent: Option<String>,
    pub is_abstract: bool,
    pub is_sealed: bool,
    pub is_singleton: bool,
    pub annotations: Vec<SchemaAnnotation>,
    pub fields: Vec<SchemaFieldInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaFieldInfo {
    pub name: String,
    pub ty: SchemaTypeRefInfo,
    pub raw_type: String,
    pub has_default: bool,
    pub default: Option<SchemaDefaultValueInfo>,
    pub annotations: Vec<SchemaAnnotation>,
    pub dimension: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaEnumInfo {
    pub module: String,
    pub name: String,
    pub annotations: Vec<SchemaAnnotation>,
    pub variants: Vec<SchemaEnumVariantInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaEnumVariantInfo {
    pub name: String,
    pub value: i64,
    pub annotations: Vec<SchemaAnnotation>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaConstInfo {
    pub module: String,
    pub name: String,
    pub value: SchemaConstValueInfo,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SchemaTypeRefInfo {
    Int,
    Float,
    Bool,
    String,
    Named { name: String, target_kind: String },
    Array { item: Box<SchemaTypeRefInfo> },
    Dict {
        key: Box<SchemaTypeRefInfo>,
        value: Box<SchemaTypeRefInfo>,
    },
    Nullable { inner: Box<SchemaTypeRefInfo> },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum SchemaAnnotationValueInfo {
    Name(String),
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Null,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaAnnotation {
    pub name: String,
    pub args: Vec<SchemaAnnotationValueInfo>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum SchemaDefaultValueInfo {
    Null,
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    Enum {
        enum_name: String,
        variant: String,
        value: i64,
    },
    EmptyArray,
    EmptyObject,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum SchemaConstValueInfo {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
}

pub fn inspect_schema(
    session: &ProjectSchemaSession,
    type_filter: Option<&str>,
    include_derived: bool,
) -> SchemaInspectReport {
    let mut type_names = session
        .schema
        .all_types()
        .map(|ty| ty.name.clone())
        .collect::<Vec<_>>();
    type_names.sort();
    if let Some(filter) = type_filter {
        type_names.retain(|name| {
            name == filter
                || (include_derived && session.schema.is_assignable(name, filter) && name != filter)
        });
    }

    let types = type_names
        .into_iter()
        .filter_map(|name| session.schema.resolve_type(&name))
        .map(|ty| SchemaTypeInfo {
            module: ty.module.to_string(),
            name: ty.name.clone(),
            parent: ty.parent.clone(),
            is_abstract: ty.is_abstract,
            is_sealed: ty.is_sealed,
            is_singleton: ty.is_singleton,
            annotations: annotations(&ty.annotations),
            fields: ty
                .all_fields
                .iter()
                .map(|field| SchemaFieldInfo {
                    name: field.name.clone(),
                    ty: type_ref_info(&session.schema, &field.ty_ref),
                    raw_type: field.ty.clone(),
                    has_default: field.has_default,
                    default: field.default.as_ref().map(default_value_info),
                    annotations: annotations(&field.annotations),
                    dimension: field.dimension.as_ref().map(|d| format!("{:?}", d.kind)),
                })
                .collect(),
        })
        .collect();

    let mut enums = session
        .schema
        .all_enums()
        .map(|schema_enum| SchemaEnumInfo {
            module: schema_enum.module.to_string(),
            name: schema_enum.name.clone(),
            annotations: annotations(&schema_enum.annotations),
            variants: schema_enum
                .variants
                .iter()
                .map(|variant| SchemaEnumVariantInfo {
                    name: variant.name.clone(),
                    value: variant.value,
                    annotations: annotations(&variant.annotations),
                })
                .collect(),
        })
        .collect::<Vec<_>>();
    enums.sort_by(|left, right| left.name.cmp(&right.name));

    let mut consts = Vec::new();
    for module_id in session.schema.module_ids() {
        if let Some(module) = session.schema.schema(module_id) {
            for schema_const in &module.consts {
                consts.push(SchemaConstInfo {
                    module: schema_const.module.to_string(),
                    name: schema_const.name.clone(),
                    value: const_value_info(&schema_const.value),
                });
            }
        }
    }
    consts.sort_by(|left, right| left.name.cmp(&right.name));

    SchemaInspectReport {
        types,
        enums,
        consts,
        diagnostics: flat_schema_diagnostics(session),
    }
}

pub fn schema_files(session: &ProjectSchemaSession) -> SchemaFilesReport {
    let files = session
        .schema
        .module_ids()
        .filter_map(|id| {
            session.schema.source(id).map(|source| SchemaFileInfo {
                module: id.to_string(),
                source: source.to_string(),
            })
        })
        .collect();
    SchemaFilesReport {
        files,
        diagnostics: flat_schema_diagnostics(session),
    }
}

fn type_ref_info(schema: &coflow_cft::CftContainer, ty: &CftSchemaTypeRef) -> SchemaTypeRefInfo {
    match ty {
        CftSchemaTypeRef::Int => SchemaTypeRefInfo::Int,
        CftSchemaTypeRef::Float => SchemaTypeRefInfo::Float,
        CftSchemaTypeRef::Bool => SchemaTypeRefInfo::Bool,
        CftSchemaTypeRef::String => SchemaTypeRefInfo::String,
        CftSchemaTypeRef::Named(name) => {
            let target_kind = if schema.has_enum(name) {
                "enum"
            } else if schema.has_type(name) {
                "type"
            } else {
                "unknown"
            };
            SchemaTypeRefInfo::Named {
                name: name.clone(),
                target_kind: target_kind.to_string(),
            }
        }
        CftSchemaTypeRef::Array(inner) => SchemaTypeRefInfo::Array {
            item: Box::new(type_ref_info(schema, inner)),
        },
        CftSchemaTypeRef::Dict(key, value) => SchemaTypeRefInfo::Dict {
            key: Box::new(type_ref_info(schema, key)),
            value: Box::new(type_ref_info(schema, value)),
        },
        CftSchemaTypeRef::Nullable(inner) => SchemaTypeRefInfo::Nullable {
            inner: Box::new(type_ref_info(schema, inner)),
        },
    }
}

fn annotations(items: &[CftAnnotation]) -> Vec<SchemaAnnotation> {
    items
        .iter()
        .map(|annotation| SchemaAnnotation {
            name: annotation.name.clone(),
            args: annotation.args.iter().map(annotation_value_info).collect(),
        })
        .collect()
}

fn annotation_value_info(value: &CftAnnotationValue) -> SchemaAnnotationValueInfo {
    match value {
        CftAnnotationValue::Name(v) => SchemaAnnotationValueInfo::Name(v.clone()),
        CftAnnotationValue::String(v) => SchemaAnnotationValueInfo::String(v.clone()),
        CftAnnotationValue::Int(v) => SchemaAnnotationValueInfo::Int(*v),
        CftAnnotationValue::Float(v) => SchemaAnnotationValueInfo::Float(*v),
        CftAnnotationValue::Bool(v) => SchemaAnnotationValueInfo::Bool(*v),
        CftAnnotationValue::Null => SchemaAnnotationValueInfo::Null,
    }
}

fn default_value_info(value: &CftSchemaDefaultValue) -> SchemaDefaultValueInfo {
    match value {
        CftSchemaDefaultValue::Null => SchemaDefaultValueInfo::Null,
        CftSchemaDefaultValue::Int(v) => SchemaDefaultValueInfo::Int(*v),
        CftSchemaDefaultValue::Float(v) => SchemaDefaultValueInfo::Float(*v),
        CftSchemaDefaultValue::Bool(v) => SchemaDefaultValueInfo::Bool(*v),
        CftSchemaDefaultValue::String(v) => SchemaDefaultValueInfo::String(v.clone()),
        CftSchemaDefaultValue::Enum {
            enum_name,
            variant,
            value,
        } => SchemaDefaultValueInfo::Enum {
            enum_name: enum_name.clone(),
            variant: variant.clone(),
            value: *value,
        },
        CftSchemaDefaultValue::EmptyArray => SchemaDefaultValueInfo::EmptyArray,
        CftSchemaDefaultValue::EmptyObject => SchemaDefaultValueInfo::EmptyObject,
    }
}

fn const_value_info(value: &CftConstValue) -> SchemaConstValueInfo {
    match value {
        CftConstValue::Int(v) => SchemaConstValueInfo::Int(*v),
        CftConstValue::Float(v) => SchemaConstValueInfo::Float(*v),
        CftConstValue::Bool(v) => SchemaConstValueInfo::Bool(*v),
        CftConstValue::String(v) => SchemaConstValueInfo::String(v.clone()),
    }
}

fn flat_schema_diagnostics(session: &ProjectSchemaSession) -> Vec<FlatDiagnostic> {
    session
        .diagnostics
        .as_set()
        .diagnostics
        .iter()
        .map(|d| d.flat_view(None, None, None))
        .collect()
}
```

Modify `crates/coflow-engine/src/lib.rs`:

```rust
mod schema_inspect;

pub use schema_inspect::{
    inspect_schema, schema_files, SchemaAnnotation, SchemaAnnotationValueInfo,
    SchemaConstInfo, SchemaConstValueInfo, SchemaDefaultValueInfo, SchemaEnumInfo,
    SchemaEnumVariantInfo, SchemaFieldInfo, SchemaFileInfo, SchemaFilesReport,
    SchemaInspectReport, SchemaTypeInfo, SchemaTypeRefInfo,
};
```

- [ ] **Step 4: Run schema inspection tests**

Run:

```powershell
cargo test -p coflow-engine --test schema_inspect
```

Expected: PASS.

- [ ] **Step 5: Commit schema inspection**

```powershell
git add crates/coflow-engine/src/lib.rs crates/coflow-engine/src/schema_inspect.rs crates/coflow-engine/tests/schema_inspect.rs
git commit -m "feat(engine): expose schema inspection reports"
```

## Task 2: Engine Data Sources, List, and Get

**Files:**
- Create: `crates/coflow-engine/src/data_read.rs`
- Modify: `crates/coflow-engine/src/lib.rs`
- Test: `crates/coflow-engine/tests/data_read.rs`

- [ ] **Step 1: Write failing data read tests**

Create `crates/coflow-engine/tests/data_read.rs`:

```rust
#![allow(clippy::expect_used, clippy::panic)]

use coflow_engine::{
    build_project_session, data_get, data_list, data_sources, DataGetQuery, DataListQuery,
    RecordCoordinate,
};
use coflow_project::Project;

fn write_project(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        r#"
            type Item {
                name: string;
                price: int;
            }
        "#,
    )
    .expect("write schema");
    std::fs::write(
        root.join("data").join("items.cfd"),
        r#"
            sword: Item { name: "Sword", price: 100 }
            shield: Item { name: "Shield", price: 80 }
        "#,
    )
    .expect("write cfd");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\nsources:\n  - path: data\noutputs:\n  data:\n    type: json\n    dir: generated/data\n",
    )
    .expect("write config");
}

#[test]
fn data_sources_report_provider_capabilities_and_types() {
    let root = std::env::temp_dir().join(format!("coflow-data-sources-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    write_project(&root);
    let project = Project::open_schema_only(Some(&root.join("coflow.yaml"))).expect("open");
    let registry = coflow_builtins::default_provider_registry().expect("registry");
    let session = build_project_session(project, &registry).expect("session");

    let report = data_sources(&session, &registry);
    let source = report
        .sources
        .iter()
        .find(|source| source.file == "data/items.cfd")
        .expect("items source");
    assert_eq!(source.provider, "cfd");
    assert!(source.capabilities.can_edit_field);
    assert!(source.capabilities.can_insert_record);
    assert_eq!(source.types, vec!["Item"]);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn data_list_and_get_support_file_type_and_key_filters() {
    let root = std::env::temp_dir().join(format!("coflow-data-get-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    write_project(&root);
    let project = Project::open_schema_only(Some(&root.join("coflow.yaml"))).expect("open");
    let registry = coflow_builtins::default_provider_registry().expect("registry");
    let session = build_project_session(project, &registry).expect("session");

    let list = data_list(
        &session,
        &DataListQuery {
            actual_type: Some("Item".to_string()),
            file: Some("data/items.cfd".to_string()),
            limit: None,
            offset: 0,
        },
    );
    assert_eq!(list.records.len(), 2);
    assert_eq!(list.records[0].record.key, "sword");

    let get = data_get(
        &session,
        &DataGetQuery {
            selector: Some(RecordCoordinate::new("Item", "sword")),
            actual_type: None,
            file: None,
            keys: Vec::new(),
            limit: None,
            offset: 0,
            all: false,
        },
    )
    .expect("get");
    assert_eq!(get.records.len(), 1);
    assert_eq!(get.records[0].record.key, "sword");
    assert!(get.records[0].fields.contains_key("price"));

    let _ = std::fs::remove_dir_all(root);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```powershell
cargo test -p coflow-engine --test data_read
```

Expected: compile failure for missing data read APIs.

- [ ] **Step 3: Implement data read module**

Create `crates/coflow-engine/src/data_read.rs`:

```rust
use std::collections::BTreeSet;

use coflow_api::{Diagnostic, DiagnosticSet, ProviderRegistry, WriterCapabilities};
use coflow_data_model::CfdValue;
use serde::{Deserialize, Serialize};

use crate::{ProjectSession, RecordCoordinate};

const DEFAULT_GET_LIMIT: usize = 100;

#[derive(Debug, Clone, Serialize)]
pub struct DataSourcesReport {
    pub sources: Vec<DataSourceInfo>,
    pub diagnostics: Vec<coflow_api::FlatDiagnostic>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DataSourceInfo {
    pub file: String,
    pub provider: String,
    pub capabilities: WriterCapabilities,
    pub types: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DataListQuery {
    pub actual_type: Option<String>,
    pub file: Option<String>,
    pub limit: Option<usize>,
    pub offset: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct DataListReport {
    pub records: Vec<DataRecordSummary>,
    pub diagnostics: Vec<coflow_api::FlatDiagnostic>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DataRecordSummary {
    pub record: RecordCoordinate,
    pub file: String,
    pub provider: String,
}

#[derive(Debug, Clone, Default)]
pub struct DataGetQuery {
    pub selector: Option<RecordCoordinate>,
    pub actual_type: Option<String>,
    pub file: Option<String>,
    pub keys: Vec<String>,
    pub limit: Option<usize>,
    pub offset: usize,
    pub all: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DataGetReport {
    pub records: Vec<DataRecordInfo>,
    pub diagnostics: Vec<coflow_api::FlatDiagnostic>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DataRecordInfo {
    pub record: RecordCoordinate,
    pub file: String,
    pub provider: String,
    pub fields: std::collections::BTreeMap<String, CfdValue>,
}

pub fn data_sources(session: &ProjectSession, registry: &ProviderRegistry) -> DataSourcesReport {
    let mut sources = Vec::new();
    for entry in session.sources.entries() {
        let mut types = BTreeSet::new();
        for id in session.records.ids_in_file(&entry.display_path) {
            if let Some(record_ref) = session.records.get(*id) {
                types.insert(record_ref.coordinate.actual_type.clone());
            }
        }
        let capabilities = registry
            .writer(&entry.provider_id)
            .map_or_else(WriterCapabilities::read_only, |writer| {
                writer
                    .descriptor()
                    .capabilities
                    .clone()
                    .with_provider_id(entry.provider_id.clone())
            });
        sources.push(DataSourceInfo {
            file: entry.display_path.clone(),
            provider: entry.provider_id.clone(),
            capabilities,
            types: types.into_iter().collect(),
        });
    }
    DataSourcesReport {
        sources,
        diagnostics: flat_diagnostics(session),
    }
}

pub fn data_list(session: &ProjectSession, query: &DataListQuery) -> DataListReport {
    let mut records = record_summaries(session, query.file.as_deref(), query.actual_type.as_deref());
    records.sort_by(|left, right| {
        left.file
            .cmp(&right.file)
            .then_with(|| left.record.actual_type.cmp(&right.record.actual_type))
            .then_with(|| left.record.key.cmp(&right.record.key))
    });
    let end = query
        .limit
        .map_or(records.len(), |limit| query.offset.saturating_add(limit))
        .min(records.len());
    let records = if query.offset >= records.len() {
        Vec::new()
    } else {
        records[query.offset..end].to_vec()
    };
    DataListReport {
        records,
        diagnostics: flat_diagnostics(session),
    }
}

pub fn data_get(
    session: &ProjectSession,
    query: &DataGetQuery,
) -> Result<DataGetReport, DiagnosticSet> {
    let mut summaries = if let Some(selector) = &query.selector {
        let view = session
            .record_view(&selector.actual_type, &selector.key)
            .ok_or_else(|| DiagnosticSet::one(not_found(selector)))?;
        vec![DataRecordSummary {
            record: view.coordinate,
            file: view.display_path.to_string(),
            provider: view.provider_id.to_string(),
        }]
    } else {
        record_summaries(session, query.file.as_deref(), query.actual_type.as_deref())
    };

    if !query.keys.is_empty() {
        let keys: BTreeSet<&str> = query.keys.iter().map(String::as_str).collect();
        summaries.retain(|summary| keys.contains(summary.record.key.as_str()));
    }
    summaries.sort_by(|left, right| {
        left.file
            .cmp(&right.file)
            .then_with(|| left.record.actual_type.cmp(&right.record.actual_type))
            .then_with(|| left.record.key.cmp(&right.record.key))
    });

    let requested_limit = query.limit.unwrap_or(DEFAULT_GET_LIMIT);
    if !query.all && query.limit.is_none() && summaries.len() > DEFAULT_GET_LIMIT {
        return Err(DiagnosticSet::one(Diagnostic::error(
            "DATA-GET-LIMIT",
            "DATA",
            format!(
                "data get matched {} records; pass --limit or --all to fetch this many records",
                summaries.len()
            ),
        )));
    }
    let end = query
        .offset
        .saturating_add(if query.all { summaries.len() } else { requested_limit })
        .min(summaries.len());
    let summaries = if query.offset >= summaries.len() {
        Vec::new()
    } else {
        summaries[query.offset..end].to_vec()
    };

    let mut records = Vec::new();
    for summary in summaries {
        let Some(view) = session.record_view(&summary.record.actual_type, &summary.record.key)
        else {
            return Err(DiagnosticSet::one(not_found(&summary.record)));
        };
        records.push(DataRecordInfo {
            record: summary.record,
            file: summary.file,
            provider: summary.provider,
            fields: view.record.fields.clone(),
        });
    }
    Ok(DataGetReport {
        records,
        diagnostics: flat_diagnostics(session),
    })
}

fn record_summaries(
    session: &ProjectSession,
    file: Option<&str>,
    actual_type: Option<&str>,
) -> Vec<DataRecordSummary> {
    let mut out = Vec::new();
    let files: Vec<String> = file
        .map(|file| vec![file.to_string()])
        .unwrap_or_else(|| session.files.source_files().iter().cloned().collect());
    for file in files {
        for id in session.records.ids_in_file(&file) {
            let Some(record_ref) = session.records.get(*id) else {
                continue;
            };
            if actual_type.is_some_and(|ty| record_ref.coordinate.actual_type != ty) {
                continue;
            }
            out.push(DataRecordSummary {
                record: record_ref.coordinate.clone(),
                file: file.clone(),
                provider: record_ref.provider_id.clone(),
            });
        }
    }
    out
}

fn not_found(coordinate: &RecordCoordinate) -> Diagnostic {
    Diagnostic::error(
        "DATA-NOT-FOUND",
        "DATA",
        format!(
            "record `{}.{}` was not found",
            coordinate.actual_type, coordinate.key
        ),
    )
}

fn flat_diagnostics(session: &ProjectSession) -> Vec<coflow_api::FlatDiagnostic> {
    session
        .diagnostics
        .as_set()
        .diagnostics
        .iter()
        .map(|d| d.flat_view(None, None, None))
        .collect()
}
```

Modify `crates/coflow-engine/src/lib.rs`:

```rust
mod data_read;

pub use data_read::{
    data_get, data_list, data_sources, DataGetQuery, DataGetReport, DataListQuery,
    DataListReport, DataRecordInfo, DataRecordSummary, DataSourceInfo, DataSourcesReport,
};
```

- [ ] **Step 4: Run data read tests**

Run:

```powershell
cargo test -p coflow-engine --test data_read
```

Expected: PASS.

- [ ] **Step 5: Commit data read engine API**

```powershell
git add crates/coflow-engine/src/lib.rs crates/coflow-engine/src/data_read.rs crates/coflow-engine/tests/data_read.rs
git commit -m "feat(engine): expose data source and record reads"
```

## Task 3: Engine Batch Patch API

**Files:**
- Create: `crates/coflow-engine/src/data_patch.rs`
- Modify: `crates/coflow-engine/src/lib.rs`
- Test: `crates/coflow-engine/tests/data_patch.rs`

- [ ] **Step 1: Write failing patch tests**

Create `crates/coflow-engine/tests/data_patch.rs`:

```rust
#![allow(clippy::expect_used, clippy::panic)]

use coflow_engine::{
    build_project_session, DataPatchOp, DataPatchRequest, PatchPathSegment, PatchRecordSelector,
};
use coflow_project::Project;
use serde_json::json;

fn write_project(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        r#"
            enum Rarity { Common = 0, Rare = 10 }

            type Item {
                name: string;
                price: int;
                rarity: Rarity = Rarity.Common;
                check { price > 0; }
            }
        "#,
    )
    .expect("write schema");
    std::fs::write(
        root.join("data").join("items.cfd"),
        r#"sword: Item { name: "Sword", price: 100 }"#,
    )
    .expect("write cfd");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\nsources:\n  - path: data\noutputs:\n  data:\n    type: json\n    dir: generated/data\n",
    )
    .expect("write config");
}

#[test]
fn patch_inserts_and_edits_cfd_records_then_reports_check_diagnostics() {
    let root = std::env::temp_dir().join(format!("coflow-data-patch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    write_project(&root);
    let project = Project::open_schema_only(Some(&root.join("coflow.yaml"))).expect("open");
    let registry = coflow_builtins::default_provider_registry().expect("registry");
    let mut session = build_project_session(project, &registry).expect("session");

    let report = session
        .apply_data_patch(
            &registry,
            DataPatchRequest {
                check_after_write: true,
                stop_on_write_error: true,
                ops: vec![
                    DataPatchOp::InsertRecord {
                        file: "data/items.cfd".to_string(),
                        actual_type: "Item".to_string(),
                        key: "bad_sword".to_string(),
                        fields: serde_json::from_value(json!({
                            "name": "Bad Sword",
                            "price": -1
                        }))
                        .expect("fields map"),
                    },
                    DataPatchOp::SetField {
                        record: PatchRecordSelector {
                            actual_type: "Item".to_string(),
                            key: "bad_sword".to_string(),
                        },
                        file: None,
                        path: vec![PatchPathSegment::Field("rarity".to_string())],
                        value: json!("Rare"),
                    },
                ],
            },
        )
        .expect("patch should write");

    assert!(report.write_ok);
    assert!(!report.check_ok);
    assert_eq!(report.applied.len(), 2);
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.stage == "CHECK"));

    let text = std::fs::read_to_string(root.join("data").join("items.cfd")).expect("read cfd");
    assert!(text.contains("bad_sword"));
    assert!(text.contains("Rare"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn patch_file_guard_rejects_wrong_record_location() {
    let root = std::env::temp_dir().join(format!("coflow-data-patch-guard-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    write_project(&root);
    let project = Project::open_schema_only(Some(&root.join("coflow.yaml"))).expect("open");
    let registry = coflow_builtins::default_provider_registry().expect("registry");
    let mut session = build_project_session(project, &registry).expect("session");

    let err = session
        .apply_data_patch(
            &registry,
            DataPatchRequest {
                check_after_write: true,
                stop_on_write_error: true,
                ops: vec![DataPatchOp::SetField {
                    record: PatchRecordSelector {
                        actual_type: "Item".to_string(),
                        key: "sword".to_string(),
                    },
                    file: Some("data/other.cfd".to_string()),
                    path: vec![PatchPathSegment::Field("price".to_string())],
                    value: json!(200),
                }],
            },
        )
        .expect_err("guard should fail");

    assert!(err
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "PATCH-FILE-GUARD"));

    let _ = std::fs::remove_dir_all(root);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```powershell
cargo test -p coflow-engine --test data_patch
```

Expected: compile failure for missing patch API.

- [ ] **Step 3: Implement patch request/report structs and dispatch**

Create `crates/coflow-engine/src/data_patch.rs` with this public surface:

```rust
use std::collections::BTreeMap;

use coflow_api::{Diagnostic, DiagnosticSet, ProviderRegistry, Severity, WriteFieldPathSegment};
use coflow_cft::CftSchemaTypeRef;
use coflow_data_model::{CfdDictKey, CfdEnumValue, CfdRecord, CfdValue};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{ProjectSession, RecordCoordinate, WriteOutcome};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataPatchRequest {
    #[serde(default = "default_true")]
    pub check_after_write: bool,
    #[serde(default = "default_true")]
    pub stop_on_write_error: bool,
    pub ops: Vec<DataPatchOp>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum DataPatchOp {
    InsertRecord {
        file: String,
        #[serde(rename = "type")]
        actual_type: String,
        key: String,
        #[serde(default)]
        fields: BTreeMap<String, Value>,
    },
    SetField {
        record: PatchRecordSelector,
        #[serde(default)]
        file: Option<String>,
        path: Vec<PatchPathSegment>,
        value: Value,
    },
    DeleteRecord {
        record: PatchRecordSelector,
        #[serde(default)]
        file: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatchRecordSelector {
    #[serde(rename = "type")]
    pub actual_type: String,
    pub key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PatchPathSegment {
    Field(String),
    Index(usize),
}

#[derive(Debug, Clone, Serialize)]
pub struct DataPatchReport {
    pub write_ok: bool,
    pub check_ok: bool,
    pub applied: Vec<DataPatchAppliedOp>,
    pub failed: Option<DataPatchFailedOp>,
    pub diagnostics: Vec<coflow_api::FlatDiagnostic>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DataPatchAppliedOp {
    pub index: usize,
    pub op: String,
    pub record: Option<RecordCoordinate>,
    pub file: Option<String>,
    pub outcome: WriteOutcome,
}

#[derive(Debug, Clone, Serialize)]
pub struct DataPatchFailedOp {
    pub index: usize,
    pub op: String,
    pub diagnostics: Vec<coflow_api::FlatDiagnostic>,
}

const fn default_true() -> bool {
    true
}
```

Add an impl block:

```rust
impl ProjectSession {
    pub fn apply_data_patch(
        &mut self,
        registry: &ProviderRegistry,
        request: DataPatchRequest,
    ) -> Result<DataPatchReport, DiagnosticSet> {
        let mut applied = Vec::new();
        for (index, op) in request.ops.iter().enumerate() {
            match apply_one(self, registry, op) {
                Ok(applied_op) => applied.push(DataPatchAppliedOp {
                    index,
                    ..applied_op
                }),
                Err(diagnostics) => {
                    let failed = DataPatchFailedOp {
                        index,
                        op: op_name(op).to_string(),
                        diagnostics: diagnostics
                            .diagnostics
                            .iter()
                            .map(|d| d.flat_view(None, None, None))
                            .collect(),
                    };
                    if request.stop_on_write_error {
                        return Ok(DataPatchReport {
                            write_ok: false,
                            check_ok: false,
                            applied,
                            failed: Some(failed),
                            diagnostics: self
                                .diagnostics
                                .as_set()
                                .diagnostics
                                .iter()
                                .map(|d| d.flat_view(None, None, None))
                                .collect(),
                        });
                    }
                }
            }
        }
        let diagnostics = self
            .diagnostics
            .as_set()
            .diagnostics
            .iter()
            .map(|d| d.flat_view(None, None, None))
            .collect::<Vec<_>>();
        let check_ok = if request.check_after_write {
            diagnostics.iter().all(|d| d.severity != "error")
        } else {
            true
        };
        Ok(DataPatchReport {
            write_ok: true,
            check_ok,
            applied,
            failed: None,
            diagnostics,
        })
    }
}
```

Implement private helpers in the same file:

```rust
fn apply_one(
    session: &mut ProjectSession,
    registry: &ProviderRegistry,
    op: &DataPatchOp,
) -> Result<DataPatchAppliedOp, DiagnosticSet> {
    match op {
        DataPatchOp::InsertRecord {
            file,
            actual_type,
            key,
            fields,
        } => {
            ensure_source_file(session, file)?;
            ensure_type_can_insert(session, actual_type)?;
            let values = coerce_insert_fields(session, actual_type, fields)?;
            let outcome = session.insert_record(registry, file, key, actual_type, &values)?;
            Ok(DataPatchAppliedOp {
                index: 0,
                op: "insert_record".to_string(),
                record: Some(RecordCoordinate::new(actual_type, key)),
                file: Some(file.clone()),
                outcome,
            })
        }
        DataPatchOp::SetField {
            record,
            file,
            path,
            value,
        } => {
            let coordinate = RecordCoordinate::new(&record.actual_type, &record.key);
            ensure_file_guard(session, &coordinate, file.as_deref())?;
            let write_path = patch_path_to_write_path(path)?;
            let expected = expected_type_for_path(session, &coordinate, path)?;
            let new_value = coerce_value(session, &expected, value)?;
            let outcome = session.write_field(
                registry,
                &coordinate.actual_type,
                &coordinate.key,
                &write_path,
                &new_value,
            )?;
            Ok(DataPatchAppliedOp {
                index: 0,
                op: "set_field".to_string(),
                record: Some(coordinate),
                file: file.clone(),
                outcome,
            })
        }
        DataPatchOp::DeleteRecord { record, file } => {
            let coordinate = RecordCoordinate::new(&record.actual_type, &record.key);
            ensure_file_guard(session, &coordinate, file.as_deref())?;
            let outcome =
                session.delete_record(registry, &coordinate.actual_type, &coordinate.key)?;
            Ok(DataPatchAppliedOp {
                index: 0,
                op: "delete_record".to_string(),
                record: Some(coordinate),
                file: file.clone(),
                outcome,
            })
        }
    }
}
```

Use these exact diagnostic codes:

```rust
fn patch_diag(code: &'static str, message: impl Into<String>) -> Diagnostic {
    Diagnostic {
        code: code.to_string(),
        stage: "PATCH".to_string(),
        severity: Severity::Error,
        message: message.into(),
        primary: None,
        related: Vec::new(),
    }
}
```

Required helper behavior:

- `PATCH-FILE`: file is not a loaded source.
- `PATCH-FILE-GUARD`: optional file guard does not match record origin.
- `PATCH-TYPE`: unknown, abstract, or singleton insert target.
- `PATCH-PATH`: unsupported or unknown path.
- `PATCH-VALUE`: value cannot be coerced to expected CFT type.
- `PATCH-REF`: `$ref` target cannot be parsed.

Value coercion rules:

```rust
fn coerce_value(
    session: &ProjectSession,
    expected: &CftSchemaTypeRef,
    value: &Value,
) -> Result<CfdValue, DiagnosticSet> {
    match expected {
        CftSchemaTypeRef::Int => value
            .as_i64()
            .map(CfdValue::Int)
            .ok_or_else(|| one_value_error("expected int")),
        CftSchemaTypeRef::Float => value
            .as_f64()
            .map(CfdValue::Float)
            .ok_or_else(|| one_value_error("expected float")),
        CftSchemaTypeRef::Bool => value
            .as_bool()
            .map(CfdValue::Bool)
            .ok_or_else(|| one_value_error("expected bool")),
        CftSchemaTypeRef::String => value
            .as_str()
            .map(|v| CfdValue::String(v.to_string()))
            .ok_or_else(|| one_value_error("expected string")),
        CftSchemaTypeRef::Nullable(inner) if value.is_null() => Ok(CfdValue::Null),
        CftSchemaTypeRef::Nullable(inner) => coerce_value(session, inner, value),
        CftSchemaTypeRef::Array(inner) => {
            let items = value
                .as_array()
                .ok_or_else(|| one_value_error("expected array"))?;
            items
                .iter()
                .map(|item| coerce_value(session, inner, item))
                .collect::<Result<Vec<_>, _>>()
                .map(CfdValue::Array)
        }
        CftSchemaTypeRef::Dict(key, item) => coerce_dict_value(session, key, item, value),
        CftSchemaTypeRef::Named(name) if session.schema.has_enum(name) => {
            let variant = value
                .as_str()
                .ok_or_else(|| one_value_error(format!("expected enum variant for `{name}`")))?;
            let int_value = session
                .schema
                .enum_variant_value(name, variant)
                .ok_or_else(|| one_value_error(format!("unknown enum variant `{name}.{variant}`")))?;
            Ok(CfdValue::Enum(CfdEnumValue {
                enum_name: name.clone(),
                variant: Some(variant.to_string()),
                value: int_value,
            }))
        }
        CftSchemaTypeRef::Named(name) => coerce_named_value(session, name, value),
    }
}
```

When implementing the helper bodies, avoid `unwrap`/`expect`, keep errors as `DiagnosticSet`, and keep path support to fields and array indexes.

Modify `crates/coflow-engine/src/lib.rs`:

```rust
mod data_patch;

pub use data_patch::{
    DataPatchAppliedOp, DataPatchFailedOp, DataPatchOp, DataPatchReport, DataPatchRequest,
    PatchPathSegment, PatchRecordSelector,
};
```

- [ ] **Step 4: Run patch tests**

Run:

```powershell
cargo test -p coflow-engine --test data_patch
```

Expected: PASS.

- [ ] **Step 5: Commit data patch engine API**

```powershell
git add crates/coflow-engine/src/lib.rs crates/coflow-engine/src/data_patch.rs crates/coflow-engine/tests/data_patch.rs
git commit -m "feat(engine): apply batch data patches"
```

## Task 4: CLI Command Modules

**Files:**
- Create: `src/schema_commands.rs`
- Create: `src/data_commands.rs`
- Modify: `src/lib.rs`
- Modify: `src/main.rs`
- Test: `tests/cli_ai_data.rs`

- [ ] **Step 1: Write failing CLI tests**

Create `tests/cli_ai_data.rs`:

```rust
#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::unwrap_used
)]

mod common;
use common::*;
use serde_json::json;

fn write_project(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        r#"
            @display("Item")
            enum Rarity { Common = 0, Rare = 10 }

            type Item {
                name: string;
                price: int;
                rarity: Rarity = Rarity.Common;
                check { price > 0; }
            }
        "#,
    )
    .expect("write schema");
    std::fs::write(
        root.join("data").join("items.cfd"),
        r#"sword: Item { name: "Sword", price: 100 }"#,
    )
    .expect("write cfd");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\nsources:\n  - path: data\noutputs:\n  data:\n    type: json\n    dir: generated/data\n",
    )
    .expect("write config");
}

#[test]
fn schema_inspect_outputs_json_by_default() {
    let root = temp_project_dir("cli-schema-inspect");
    let _cleanup = TempDirCleanup(root.clone());
    write_project(&root);

    let output = coflow()
        .args(["schema", "inspect", root.to_str().expect("utf8")])
        .output()
        .expect("run schema inspect");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let json: Value = serde_json::from_slice(&output.stdout).expect("json");
    assert!(json["types"].as_array().expect("types").iter().any(|ty| {
        ty["name"] == "Item"
            && ty["annotations"]
                .as_array()
                .is_some_and(|items| items.iter().any(|a| a["name"] == "display"))
    }));
}

#[test]
fn data_get_can_fetch_single_complete_record() {
    let root = temp_project_dir("cli-data-get");
    let _cleanup = TempDirCleanup(root.clone());
    write_project(&root);

    let output = coflow()
        .args([
            "data",
            "get",
            root.to_str().expect("utf8"),
            "Item.sword",
        ])
        .output()
        .expect("run data get");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let json: Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(json["records"][0]["record"]["key"], "sword");
    assert_eq!(json["records"][0]["file"], "data/items.cfd");
}

#[test]
fn data_patch_writes_then_returns_check_diagnostics() {
    let root = temp_project_dir("cli-data-patch");
    let _cleanup = TempDirCleanup(root.clone());
    write_project(&root);
    let patch_path = root.join("patch.json");
    std::fs::write(
        &patch_path,
        serde_json::to_string(&json!({
            "ops": [{
                "op": "insert_record",
                "file": "data/items.cfd",
                "type": "Item",
                "key": "bad_sword",
                "fields": { "name": "Bad Sword", "price": -1 }
            }]
        }))
        .expect("patch json"),
    )
    .expect("write patch");

    let output = coflow()
        .args([
            "data",
            "patch",
            root.to_str().expect("utf8"),
            "--patch",
            patch_path.to_str().expect("utf8 patch"),
        ])
        .output()
        .expect("run data patch");

    assert!(!output.status.success(), "check diagnostics should produce non-zero exit");
    let json: Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(json["write_ok"], true);
    assert_eq!(json["check_ok"], false);
    assert!(json["diagnostics"].as_array().expect("diagnostics").iter().any(|d| {
        d["stage"] == "CHECK"
    }));
    let text = std::fs::read_to_string(root.join("data").join("items.cfd")).expect("read cfd");
    assert!(text.contains("bad_sword"));
}
```

- [ ] **Step 2: Run CLI test to verify it fails**

Run:

```powershell
cargo test --test cli_ai_data
```

Expected: command/subcommand failures.

- [ ] **Step 3: Implement `src/schema_commands.rs`**

Create `src/schema_commands.rs`:

```rust
use coflow_engine::{build_project_schema_session, inspect_schema, schema_files};
use coflow_project::Project;
use serde::Serialize;
use std::io::{self, Write};
use std::path::Path;

pub fn inspect(
    config_or_dir: Option<&Path>,
    type_filter: Option<&str>,
    include_derived: bool,
    human: bool,
) -> Result<bool, String> {
    let project = Project::open_schema_only(config_or_dir)?;
    let session = build_project_schema_session(project)?;
    let report = inspect_schema(&session, type_filter, include_derived);
    if human {
        write_schema_inspect_human(&report)?;
    } else {
        write_json(&report)?;
    }
    Ok(report.diagnostics.is_empty())
}

pub fn files(config_or_dir: Option<&Path>, human: bool) -> Result<bool, String> {
    let project = Project::open_schema_only(config_or_dir)?;
    let session = build_project_schema_session(project)?;
    let report = schema_files(&session);
    if human {
        for file in &report.files {
            println!("{}\n{}\n", file.module, file.source);
        }
    } else {
        write_json(&report)?;
    }
    Ok(report.diagnostics.is_empty())
}

fn write_json(value: &impl Serialize) -> Result<(), String> {
    serde_json::to_writer(io::stdout().lock(), value)
        .map_err(|err| format!("failed to write JSON: {err}"))?;
    println!();
    Ok(())
}

fn write_schema_inspect_human(report: &coflow_engine::SchemaInspectReport) -> Result<(), String> {
    let mut stdout = io::stdout().lock();
    for ty in &report.types {
        writeln!(stdout, "type {}", ty.name).map_err(|err| err.to_string())?;
        for field in &ty.fields {
            writeln!(stdout, "  {}: {}", field.name, field.raw_type)
                .map_err(|err| err.to_string())?;
        }
    }
    for schema_enum in &report.enums {
        writeln!(stdout, "enum {}", schema_enum.name).map_err(|err| err.to_string())?;
        for variant in &schema_enum.variants {
            writeln!(stdout, "  {} = {}", variant.name, variant.value)
                .map_err(|err| err.to_string())?;
        }
    }
    Ok(())
}
```

- [ ] **Step 4: Implement `src/data_commands.rs`**

Create `src/data_commands.rs`:

```rust
use coflow_engine::{
    build_project_session, data_get, data_list, data_sources, DataGetQuery, DataListQuery,
    DataPatchRequest, RecordCoordinate,
};
use coflow_project::Project;
use serde::Serialize;
use std::io::{self, Write};
use std::path::Path;

pub fn sources(config_or_dir: Option<&Path>, human: bool) -> Result<bool, String> {
    let (session, registry) = open_session(config_or_dir)?;
    let report = data_sources(&session, &registry);
    if human {
        for source in &report.sources {
            println!("{}\t{}\t{}", source.file, source.provider, source.types.join(","));
        }
    } else {
        write_json(&report)?;
    }
    Ok(report.diagnostics.is_empty())
}

pub fn list(
    config_or_dir: Option<&Path>,
    actual_type: Option<String>,
    file: Option<String>,
    limit: Option<usize>,
    offset: usize,
    human: bool,
) -> Result<bool, String> {
    let (session, _registry) = open_session(config_or_dir)?;
    let report = data_list(
        &session,
        &DataListQuery {
            actual_type,
            file,
            limit,
            offset,
        },
    );
    if human {
        for record in &report.records {
            println!(
                "{}\t{}\t{}",
                record.record.actual_type, record.record.key, record.file
            );
        }
    } else {
        write_json(&report)?;
    }
    Ok(report.diagnostics.is_empty())
}

pub fn get(
    config_or_dir: Option<&Path>,
    selector: Option<RecordCoordinate>,
    actual_type: Option<String>,
    file: Option<String>,
    keys: Vec<String>,
    limit: Option<usize>,
    offset: usize,
    all: bool,
    human: bool,
) -> Result<bool, String> {
    let (session, _registry) = open_session(config_or_dir)?;
    let report = data_get(
        &session,
        &DataGetQuery {
            selector,
            actual_type,
            file,
            keys,
            limit,
            offset,
            all,
        },
    )
    .map_err(|diagnostics| {
        diagnostics
            .diagnostics
            .iter()
            .map(|d| d.message.as_str())
            .collect::<Vec<_>>()
            .join("; ")
    })?;
    if human {
        for record in &report.records {
            println!(
                "{}.{}\t{}",
                record.record.actual_type, record.record.key, record.file
            );
            for (name, value) in &record.fields {
                println!("  {name}\t{value:?}");
            }
        }
    } else {
        write_json(&report)?;
    }
    Ok(report.diagnostics.is_empty())
}

pub fn patch(config_or_dir: Option<&Path>, patch_path: &Path, human: bool) -> Result<bool, String> {
    let patch_text = std::fs::read_to_string(patch_path)
        .map_err(|err| format!("failed to read `{}`: {err}", patch_path.display()))?;
    let request: DataPatchRequest = serde_json::from_str(&patch_text)
        .map_err(|err| format!("failed to parse `{}`: {err}", patch_path.display()))?;
    let (mut session, registry) = open_session(config_or_dir)?;
    let report = session
        .apply_data_patch(&registry, request)
        .map_err(|diagnostics| {
            diagnostics
                .diagnostics
                .iter()
                .map(|d| d.message.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        })?;
    let ok = report.write_ok && report.check_ok;
    if human {
        println!("Applied {} operation(s).", report.applied.len());
        for diagnostic in &report.diagnostics {
            println!("[{}] [{}] {}", diagnostic.code, diagnostic.stage, diagnostic.message);
        }
    } else {
        write_json(&report)?;
    }
    Ok(ok)
}

fn open_session(
    config_or_dir: Option<&Path>,
) -> Result<(coflow_engine::ProjectSession, coflow_api::ProviderRegistry), String> {
    let project = Project::open_schema_only(config_or_dir)?;
    let registry = coflow_builtins::default_provider_registry().map_err(|err| err.to_string())?;
    let session = build_project_session(project, &registry)?;
    Ok((session, registry))
}

fn write_json(value: &impl Serialize) -> Result<(), String> {
    serde_json::to_writer(io::stdout().lock(), value)
        .map_err(|err| format!("failed to write JSON: {err}"))?;
    println!();
    Ok(())
}
```

- [ ] **Step 5: Wire modules into `src/lib.rs`**

Modify `src/lib.rs`:

```rust
mod artifacts;
pub mod commands;
pub mod data_commands;
pub mod diagnostics;
pub mod schema_commands;
```

- [ ] **Step 6: Wire `schema` and `data` into `src/main.rs`**

Add imports:

```rust
use coflow::data_commands;
use coflow::schema_commands;
```

Add to `Command`:

```rust
    /// Schema inspection tools for automation and AI agents.
    Schema(SchemaArgs),
    /// Data inspection and patch tools for automation and AI agents.
    Data(DataArgs),
```

Add argument structs:

```rust
#[derive(Debug, Args)]
struct SchemaArgs {
    #[command(subcommand)]
    command: SchemaCommand,
}

#[derive(Debug, Subcommand)]
enum SchemaCommand {
    Inspect(SchemaInspectArgs),
    Files(SchemaFilesArgs),
}

#[derive(Debug, Args)]
struct SchemaInspectArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    config_or_dir: Option<PathBuf>,
    #[arg(long = "type", value_name = "TYPE")]
    type_filter: Option<String>,
    #[arg(long)]
    include_derived: bool,
    #[arg(long)]
    human: bool,
}

#[derive(Debug, Args)]
struct SchemaFilesArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    config_or_dir: Option<PathBuf>,
    #[arg(long)]
    human: bool,
}

#[derive(Debug, Args)]
struct DataArgs {
    #[command(subcommand)]
    command: DataCommand,
}

#[derive(Debug, Subcommand)]
enum DataCommand {
    Sources(DataSourcesArgs),
    List(DataListArgs),
    Get(DataGetArgs),
    Patch(DataPatchArgs),
}

#[derive(Debug, Args)]
struct DataSourcesArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    config_or_dir: Option<PathBuf>,
    #[arg(long)]
    human: bool,
}

#[derive(Debug, Args)]
struct DataListArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    config_or_dir: Option<PathBuf>,
    #[arg(long = "type", value_name = "TYPE")]
    actual_type: Option<String>,
    #[arg(long, value_name = "FILE")]
    file: Option<String>,
    #[arg(long)]
    limit: Option<usize>,
    #[arg(long, default_value_t = 0)]
    offset: usize,
    #[arg(long)]
    human: bool,
}

#[derive(Debug, Args)]
struct DataGetArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    config_or_dir: Option<PathBuf>,
    #[arg(value_name = "TYPE.KEY")]
    selector: Option<String>,
    #[arg(long = "type", value_name = "TYPE")]
    actual_type: Option<String>,
    #[arg(long, value_name = "FILE")]
    file: Option<String>,
    #[arg(long, value_delimiter = ',')]
    keys: Vec<String>,
    #[arg(long)]
    limit: Option<usize>,
    #[arg(long, default_value_t = 0)]
    offset: usize,
    #[arg(long)]
    all: bool,
    #[arg(long)]
    human: bool,
}

#[derive(Debug, Args)]
struct DataPatchArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    config_or_dir: Option<PathBuf>,
    #[arg(long, value_name = "PATCH_FILE")]
    patch: PathBuf,
    #[arg(long)]
    human: bool,
}
```

Add match arms in `run()`:

```rust
        Command::Schema(args) => run_schema(&args),
        Command::Data(args) => run_data(&args),
```

Add functions:

```rust
fn run_schema(args: &SchemaArgs) -> Result<bool, String> {
    match &args.command {
        SchemaCommand::Inspect(args) => schema_commands::inspect(
            args.config_or_dir.as_deref(),
            args.type_filter.as_deref(),
            args.include_derived,
            args.human,
        ),
        SchemaCommand::Files(args) => {
            schema_commands::files(args.config_or_dir.as_deref(), args.human)
        }
    }
}

fn run_data(args: &DataArgs) -> Result<bool, String> {
    match &args.command {
        DataCommand::Sources(args) => {
            data_commands::sources(args.config_or_dir.as_deref(), args.human)
        }
        DataCommand::List(args) => data_commands::list(
            args.config_or_dir.as_deref(),
            args.actual_type.clone(),
            args.file.clone(),
            args.limit,
            args.offset,
            args.human,
        ),
        DataCommand::Get(args) => data_commands::get(
            args.config_or_dir.as_deref(),
            parse_record_selector(args.selector.as_deref())?,
            args.actual_type.clone(),
            args.file.clone(),
            args.keys.clone(),
            args.limit,
            args.offset,
            args.all,
            args.human,
        ),
        DataCommand::Patch(args) => {
            data_commands::patch(args.config_or_dir.as_deref(), &args.patch, args.human)
        }
    }
}

fn parse_record_selector(value: Option<&str>) -> Result<Option<RecordCoordinate>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    let Some((actual_type, key)) = value.split_once('.') else {
        return Err(format!("record selector `{value}` must be written as TYPE.KEY"));
    };
    if actual_type.is_empty() || key.is_empty() {
        return Err(format!("record selector `{value}` must be written as TYPE.KEY"));
    }
    Ok(Some(RecordCoordinate::new(actual_type, key)))
}
```

Import `RecordCoordinate` in `src/main.rs`:

```rust
use coflow_engine::RecordCoordinate;
```

- [ ] **Step 7: Run CLI tests**

Run:

```powershell
cargo test --test cli_ai_data
```

Expected: PASS.

- [ ] **Step 8: Commit CLI command surface**

```powershell
git add src/lib.rs src/main.rs src/schema_commands.rs src/data_commands.rs tests/cli_ai_data.rs
git commit -m "feat(cli): add schema and data automation commands"
```

## Task 5: Documentation

**Files:**
- Modify: `docs/spec/09-cli.md`
- Modify: `README.md`

- [ ] **Step 1: Document commands in CLI spec**

Add sections to `docs/spec/09-cli.md` after `coflow lsp` and before `coflow check`:

```markdown
### `coflow schema inspect [CONFIG_OR_DIR] [--type TYPE] [--include-derived] [--human]`

Outputs a compiled schema view for automation. JSON is the default output format.
The report includes types, inherited fields, defaults, enum variants, and annotations.
Use `--type TYPE` to limit output to one type. Use `--include-derived` to include
types assignable to the requested type.

### `coflow schema files [CONFIG_OR_DIR] [--human]`

Outputs the CFT module sources that participated in schema compilation. This is
intended for agents that need comments, check blocks, or source-level context.

### `coflow data sources [CONFIG_OR_DIR] [--human]`

Outputs resolved data sources, provider ids, writer capabilities, and record
types found in each source.

### `coflow data list [CONFIG_OR_DIR] [--type TYPE] [--file FILE] [--limit N] [--offset N] [--human]`

Outputs a lightweight record index. Records include `type`, `key`, `file`, and
provider, but not full field values.

### `coflow data get [CONFIG_OR_DIR] [TYPE.KEY] [--type TYPE] [--file FILE] [--keys a,b] [--limit N] [--offset N] [--all] [--human]`

Outputs complete record values. By default, large result sets require `--limit`
or `--all` to avoid accidental full-project dumps.

### `coflow data patch [CONFIG_OR_DIR] --patch JSON [--human]`

Applies a batch data patch through provider writers. Write errors stop the batch
when `stop_on_write_error` is true. CFT check diagnostics do not block writes;
the project is rebuilt and diagnostics are returned after writing.
```

- [ ] **Step 2: Add README quick examples**

Add a short automation section to `README.md` after the common commands:

```markdown
AI/data automation commands default to JSON:

```powershell
cargo run -- schema inspect examples/rpg
cargo run -- data sources examples/rpg
cargo run -- data list examples/rpg --type Item
cargo run -- data get examples/rpg Item.potion
cargo run -- data patch examples/rpg --patch-file patch.json
```

`data patch` writes through the same provider writer layer used by the editor.
It runs project checks after writing and returns diagnostics so agents can make
follow-up fixes.
```
```

- [ ] **Step 3: Commit docs**

```powershell
git add docs/spec/09-cli.md README.md
git commit -m "docs: describe schema and data automation commands"
```

## Task 6: Final Verification

**Files:**
- No new files.
- Validates the whole workspace.

- [ ] **Step 1: Run focused tests**

Run:

```powershell
cargo test -p coflow-engine --test schema_inspect
cargo test -p coflow-engine --test data_read
cargo test -p coflow-engine --test data_patch
cargo test --test cli_ai_data
```

Expected: all PASS.

- [ ] **Step 2: Run repository checks required by `AGENTS.md`**

Run from the repository root:

```powershell
cargo check --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: all PASS.

- [ ] **Step 3: Fix any verification failures**

If formatting fails, run:

```powershell
cargo fmt --all
```

Then rerun:

```powershell
cargo fmt --all -- --check
```

If clippy fails, fix the reported warnings in the smallest relevant module and rerun:

```powershell
cargo clippy --workspace --all-targets -- -D warnings
```

If tests fail, fix only the failing behavior and rerun the focused test before rerunning the workspace suite.

- [ ] **Step 4: Commit verification fixes if any**

Only if Step 3 changed files:

```powershell
git add .
git commit -m "fix: satisfy automation command checks"
```

## Self-Review

- Spec coverage: The plan covers schema context, annotations, raw CFT files, data sources, list/get, batch patch, optional `CONFIG_OR_DIR`, JSON default, human output, write-after-check behavior, file requirement for inserts, and future editor/MCP reuse through engine APIs.
- Scope check: The plan avoids editor migration, MCP, daemon mode, transactions, dict-key writes, and sugar commands. Those are separate follow-up projects.
- Type consistency: Public names are consistently `DataPatchRequest`, `DataPatchOp`, `DataPatchReport`, `PatchRecordSelector`, `PatchPathSegment`, `DataGetQuery`, `DataListQuery`, `SchemaInspectReport`, and `SchemaFilesReport`.
- Risk: The highest-risk area is schema-guided coercion from `serde_json::Value` to `CfdValue`. Keep this code isolated in `data_patch.rs` and add new focused tests before widening supported value forms.
