# Core Crates Review Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the reviewed core-crate/spec inconsistencies while preserving the agreed C# trusted-export loader boundary.

**Architecture:** The work is split by contract boundary: docs/specs first, then CFT language/schema changes, then runtime checker semantics, then exporter/codegen alignment, then Excel loader cell handling. Each task adds failing tests before implementation and keeps changes scoped to the crate that owns the behavior.

**Tech Stack:** Rust workspace crates, Tera C# templates, serde_json, calamine Excel data model, optional .NET SDK verification for generated C#.

---

## Files And Responsibilities

- `docs/spec/01-cft.md`: CFT syntax, reserved identifiers, annotations, type/eval semantics.
- `docs/spec/02-schema-api.md`: New public schema reflection API contract.
- `docs/spec/04-excel-loader.md`: Low-level Excel loader crate contract only.
- `docs/spec/07-project-pipeline.md`: New project/YAML/CLI orchestration contract.
- `docs/spec/06-csharp-codegen.md`: Generated C# trusted-export loader contract.
- `crates/coflow-cft/src/schema/support.rs`: Reserved-name helpers, annotation target definitions, type helper semantics.
- `crates/coflow-cft/src/schema/compiler.rs`: Reserved identifier validation, enum variant annotation validation/conversion.
- `crates/coflow-cft/src/schema/type_checker.rs`: `is null`, built-in calls with nullable element arrays.
- `crates/coflow-cft/tests/schema.rs`: Annotation and reserved-name schema tests.
- `crates/coflow-cft/tests/type_checker.rs`: `is null` and nullable aggregate type tests.
- `crates/coflow-checker/src/check/evaluator.rs`: Nullable element runtime behavior for built-ins.
- `crates/coflow-checker/tests/check.rs`: Runtime checker behavior tests.
- `crates/coflow-json-export/src/lib.rs`: Export complete table set, including empty tables.
- `crates/coflow-json-export/tests/json_export.rs`: Empty table export test.
- `crates/coflow-codegen-csharp/src/emit.rs`: Avoid `@struct` property initializers; align codegen docs/tests.
- `crates/coflow-codegen-csharp/src/lib.rs`: Codegen tests for struct defaults and enum variant annotations.
- `crates/coflow-codegen-csharp/templates/*.tera`: Only adjust templates if tests show rendered output still violates the contract.
- `crates/coflow-excel-loader/src/lib.rs`: Explicit cell type conversion errors and Rustdoc updates.
- `crates/coflow-excel-loader/tests/excel_loader.rs`: Error/date/bool cell tests.
- `tests/cli.rs`: Optional generated C# compile/load integration helper if kept in Rust integration tests.

---

### Task 1: Update Specs For Agreed Contracts

**Files:**
- Modify: `docs/spec/01-cft.md`
- Create: `docs/spec/02-schema-api.md`
- Modify: `docs/spec/04-excel-loader.md`
- Create: `docs/spec/07-project-pipeline.md`
- Modify: `docs/spec/06-csharp-codegen.md`

- [ ] **Step 1: Update `01-cft.md` language rules**

Document:

```markdown
Reserved identifiers include current keywords/literals, primitive names, current built-in function names, future syntax names, and `_`.

`is null` is valid for any nullable operand and invalid for non-nullable operands.

`is TypeName` is an assignable dynamic type predicate: actual type equal to TypeName or any subtype returns true.

Enum variants may carry `@display("text")` and `@deprecated`; other annotations on enum variants are invalid.

`unique`, `min`, `max`, `sum`, `contains`, and `len` have defined nullable element behavior:
- `unique` treats null as a comparable value.
- `min`/`max` skip null and error when no non-null value exists.
- `sum` skips null and returns zero when no non-null value exists.
- `contains([T?], null)` checks for a null element.
- `len` counts null elements.
```

- [ ] **Step 2: Create `02-schema-api.md`**

Include these sections:

```markdown
# Schema API

## Scope
This document describes the public Rust schema reflection model produced by `coflow-cft`.

## Stable Concepts
- `CftContainer` owns compiled modules and exposes all consts, enums, and types.
- `CftSchemaModule` groups definitions by module.
- `CftSchemaType.fields` are declared fields.
- `CftSchemaType.all_fields` are inherited fields in effective order.
- `CftSchemaField.ty_ref` is the resolved public type reference.
- `span` and `module` identify source locations for diagnostics.
- `check` holds the compiled check expression block.
- `default` holds compiled default values.
- `CftSchemaEnumVariant.annotations` carries variant-level `@display` and `@deprecated`.

## Consumers
Codegen crates should use this schema API rather than re-reading AST.
```

- [ ] **Step 3: Split Excel/project docs**

In `04-excel-loader.md`, keep only low-level crate behavior:

```markdown
`coflow-excel-loader` takes an already compiled `CftContainer` and already parsed `ExcelSource` values.
It does not discover project files, parse `coflow.yaml`, or own CLI orchestration.
`load_excel_model` builds a data model and does not run CFT checks.
`load_excel` builds a data model and runs CFT checks.
```

Create `07-project-pipeline.md` for:

```markdown
# Project Pipeline

Project loading owns `coflow.yaml`, schema discovery, Excel source definitions, CLI command orchestration, JSON export, and C# codegen invocation.
```

- [ ] **Step 4: Update C# codegen spec**

In `06-csharp-codegen.md`, replace strict arbitrary JSON validation language with:

```markdown
Generated C# is a trusted artifact loader. It supports JSON produced by the official Coflow exporter from data already accepted by the Rust pipeline. It deserializes, builds runtime lookups, and resolves generated object references. It does not promise stable diagnostics for arbitrary hand-written or corrupted JSON and does not run CFT checks.
```

Also remove nullable `@index` behavior and keep `@index` non-nullable.

- [ ] **Step 5: Verify docs for contradictions**

Run:

```powershell
rg -n "nullable.*@index|enum variant|trusted artifact|load_excel_model|is null|Reserved" docs/spec
```

Expected: references match the new contracts and no doc still says nullable `@index` is supported or enum variant annotations are forbidden.

---

### Task 2: Reserved Identifiers

**Files:**
- Modify: `crates/coflow-cft/src/schema/support.rs`
- Modify: `crates/coflow-cft/src/schema/compiler.rs`
- Modify: `crates/coflow-cft/tests/schema.rs`
- Optional modify: `crates/coflow-cft/src/error.rs`

- [ ] **Step 1: Write failing reserved-name tests**

Add tests to `crates/coflow-cft/tests/schema.rs`:

```rust
#[test]
fn schema_rejects_reserved_identifiers() {
    let cases = [
        "type int { value: string; }",
        "enum len { A, }",
        "const match = 1;",
        "type Item { from: string; }",
        "enum E { _, }",
    ];

    for source in cases {
        let err = compile_one(source).expect_err(source);
        assert_has_code(&err, CftErrorCode::ReservedIdentifier);
    }
}

#[test]
fn schema_allows_underscore_prefixed_identifiers() {
    compile_one("type _Internal { _value: int; }").expect("underscore-prefixed names are valid");
}
```

If adding `ReservedIdentifier` is deferred, use the final chosen error code consistently in the test.

- [ ] **Step 2: Verify tests fail**

Run:

```powershell
cargo test -p coflow-cft --test schema schema_rejects_reserved_identifiers schema_allows_underscore_prefixed_identifiers
```

Expected: compile error if `ReservedIdentifier` is new and not defined, or test failure because names are currently accepted.

- [ ] **Step 3: Add error code if needed**

In `crates/coflow-cft/src/error.rs`, add:

```rust
ReservedIdentifier,
```

Map it to the schema stage and a stable code near other schema identifier diagnostics.

- [ ] **Step 4: Add reserved helper**

In `support.rs`, add:

```rust
pub(super) fn is_reserved_identifier(name: &str) -> bool {
    matches!(
        name,
        "_"
            | "const" | "enum" | "type" | "abstract" | "sealed" | "check" | "when"
            | "all" | "any" | "none" | "in" | "is" | "true" | "false" | "null"
            | "int" | "float" | "bool" | "string"
            | "len" | "contains" | "unique" | "min" | "max" | "sum" | "keys" | "values" | "matches"
            | "if" | "else" | "match" | "case" | "for" | "while" | "let"
            | "module" | "import" | "export" | "from" | "as" | "use"
    )
}
```

- [ ] **Step 5: Validate all user-defined identifiers**

In `compiler.rs`, call a local helper for:

- const names in `collect_symbols`;
- enum names in `collect_symbols`;
- type names in `collect_symbols`;
- enum variant names in `validate_enums`;
- field names while validating type fields.

Use diagnostic message:

```rust
format!("`{name}` is a reserved identifier")
```

- [ ] **Step 6: Verify**

Run:

```powershell
cargo test -p coflow-cft --test schema schema_rejects_reserved_identifiers schema_allows_underscore_prefixed_identifiers
cargo test -p coflow-cft
```

Expected: both targeted tests pass and full crate tests pass.

---

### Task 3: Enum Variant Annotations

**Files:**
- Modify: `crates/coflow-cft/src/schema/support.rs`
- Modify: `crates/coflow-cft/src/schema/compiler.rs`
- Modify: `crates/coflow-cft/tests/schema.rs`
- Modify: `crates/coflow-codegen-csharp/src/lib.rs`

- [ ] **Step 1: Add failing CFT tests**

Add to `crates/coflow-cft/tests/schema.rs`:

```rust
#[test]
fn schema_accepts_display_and_deprecated_on_enum_variants() {
    let schema = compile_one(
        r#"
            enum Rarity {
                @display("Common display")
                Common,
                @deprecated
                Old,
            }
        "#,
    )
    .expect("variant annotations should compile");

    let rarity = schema.resolve_enum("Rarity").expect("enum");
    assert_eq!(rarity.variants[0].annotations[0].name, "display");
    assert_eq!(rarity.variants[1].annotations[0].name, "deprecated");
}

#[test]
fn schema_rejects_invalid_enum_variant_annotations() {
    let err = compile_one(
        r#"
            enum Rarity {
                @index
                Common,
            }
        "#,
    )
    .expect_err("invalid variant annotation should fail");
    assert_has_code(&err, CftErrorCode::InvalidAnnotationTarget);
}
```

- [ ] **Step 2: Verify tests fail**

Run:

```powershell
cargo test -p coflow-cft --test schema schema_accepts_display_and_deprecated_on_enum_variants schema_rejects_invalid_enum_variant_annotations
```

Expected: first test fails because compiler reports invalid variant annotations or schema annotations are empty.

- [ ] **Step 3: Add enum variant annotation target**

In `support.rs`:

```rust
pub(super) enum AnnotationTarget {
    Type,
    Enum,
    EnumVariant,
    Field,
}
```

Add `AnnotationTarget::EnumVariant` to `display` and `deprecated` targets only.

- [ ] **Step 4: Remove blanket rejection and validate variants**

In `compiler.rs`:

- remove the loop in `report_dangling_annotations` that says annotations cannot be applied to enum variants;
- in `validate_annotations`, iterate `info.def.variants` and call `validate_annotation_list(..., AnnotationTarget::EnumVariant, &variant.annotations)`;
- in schema conversion around `CftSchemaEnumVariant`, replace `annotations: Vec::new()` with `annotations: convert_annotations(&variant.annotations)`.

- [ ] **Step 5: Add C# codegen test**

Add to `crates/coflow-codegen-csharp/src/lib.rs` tests:

```rust
#[test]
fn codegen_emits_enum_variant_annotations() -> Result<(), String> {
    let schema = compile_schema(
        r#"
            enum Rarity {
                @display("Common display")
                Common,
                @deprecated
                Old,
            }
        "#,
    )?;

    let files = generate_csharp_json(&schema, &CsharpCodegenOptions::new("Game.Config"))
        .map_err(|err| err.to_string())?;
    let rarity = generated_file(&files, "Rarity.cs")?;
    require_contains(rarity, "/// <summary>Common display</summary>")?;
    require_contains(rarity, "[Obsolete]")?;
    Ok(())
}
```

The existing `enum.cs.tera` and `emit.rs` already read variant annotations, so this test should pass after CFT schema conversion is fixed.

- [ ] **Step 6: Verify**

Run:

```powershell
cargo test -p coflow-cft --test schema schema_accepts_display_and_deprecated_on_enum_variants schema_rejects_invalid_enum_variant_annotations
cargo test -p coflow-codegen-csharp codegen_emits_enum_variant_annotations
```

Expected: targeted tests pass.

---

### Task 4: `is null` Type Semantics

**Files:**
- Modify: `crates/coflow-cft/src/schema/type_checker.rs`
- Modify: `crates/coflow-cft/tests/type_checker.rs`

- [ ] **Step 1: Add failing tests**

Add to `crates/coflow-cft/tests/type_checker.rs`:

```rust
#[test]
fn type_checker_allows_is_null_for_nullable_operands() {
    compile_one(
        r#"
            type Child { id: string; }
            type Holder {
                maybe_int: int? = null;
                maybe_child: Child? = null;
                check {
                    maybe_int is null;
                    maybe_child is null;
                }
            }
        "#,
    )
    .expect("nullable operands may use is null");
}

#[test]
fn type_checker_rejects_is_null_for_non_nullable_operands() {
    let err = compile_one(
        r#"
            type Holder {
                value: int;
                check { value is null; }
            }
        "#,
    )
    .expect_err("non-nullable operand should fail");
    assert_has_code(&err, CftErrorCode::OperatorTypeMismatch);
}
```

- [ ] **Step 2: Verify failing behavior**

Run:

```powershell
cargo test -p coflow-cft --test type_checker type_checker_allows_is_null_for_nullable_operands type_checker_rejects_is_null_for_non_nullable_operands
```

Expected: reject test fails because current checker allows `value is null`.

- [ ] **Step 3: Implement `is null` check**

In `check_is`, replace the empty null branch with:

```rust
TypePredicate::Null(_) => {
    if !matches!(lhs, Ty::Nullable(_) | Ty::Unknown) {
        self.diag(
            CftErrorCode::OperatorTypeMismatch,
            span,
            "`is null` requires a nullable operand",
        );
    }
}
```

Keep the existing `TypePredicate::Type` behavior.

- [ ] **Step 4: Verify**

Run:

```powershell
cargo test -p coflow-cft --test type_checker
```

Expected: type checker tests pass.

---

### Task 5: Nullable Element Built-In Semantics

**Files:**
- Modify: `crates/coflow-cft/src/schema/type_checker.rs`
- Modify: `crates/coflow-cft/src/schema/support.rs`
- Modify: `crates/coflow-cft/tests/type_checker.rs`
- Modify: `crates/coflow-checker/src/check/evaluator.rs`
- Modify: `crates/coflow-checker/tests/check.rs`

- [ ] **Step 1: Add type checker tests**

Add to `crates/coflow-cft/tests/type_checker.rs`:

```rust
#[test]
fn type_checker_accepts_nullable_element_builtins() {
    compile_one(
        r#"
            type Holder {
                values: [int?] = [];
                check {
                    unique(values);
                    min(values) >= 0;
                    max(values) >= 0;
                    sum(values) >= 0;
                    contains(values, null);
                }
            }
        "#,
    )
    .expect("nullable element arrays are supported by built-ins");
}
```

- [ ] **Step 2: Add runtime checker tests**

Add to `crates/coflow-checker/tests/check.rs`:

```rust
#[test]
fn check_runner_handles_nullable_element_builtins() {
    let schema = compile_schema(
        r#"
            type Holder {
                values: [int?] = [];
                check {
                    unique(values);
                    min(values) == 1;
                    max(values) == 3;
                    sum(values) == 4;
                    contains(values, null);
                    len(values) == 3;
                }
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Holder",
        [(
            "values",
            CfdInputValue::Array(vec![
                CfdInputValue::from(1_i64),
                CfdInputValue::Null,
                CfdInputValue::from(3_i64),
            ]),
        )],
    );
    let model = builder.build().expect("data model should build");
    model.run_checks(&schema).expect("checks should pass");
}

#[test]
fn check_runner_reports_min_max_when_nullable_array_has_no_values() {
    let schema = compile_schema(
        r#"
            type Holder {
                values: [int?] = [];
                check { min(values) >= 0; }
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Holder",
        [("values", CfdInputValue::Array(vec![CfdInputValue::Null]))],
    );
    let model = builder.build().expect("data model should build");
    let err = model.run_checks(&schema).expect_err("min over all-null values should fail");
    assert_has_code(&err, CfdErrorCode::CheckEvalTypeError);
}

#[test]
fn check_runner_unique_counts_multiple_nulls_as_duplicates() {
    let schema = compile_schema(
        r#"
            type Holder {
                values: [int?] = [];
                check { unique(values); }
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Holder",
        [("values", CfdInputValue::Array(vec![CfdInputValue::Null, CfdInputValue::Null]))],
    );
    let model = builder.build().expect("data model should build");
    let err = model.run_checks(&schema).expect_err("multiple nulls are not unique");
    assert_has_code(&err, CfdErrorCode::CheckFailed);
}
```

- [ ] **Step 3: Verify failing runtime behavior**

Run:

```powershell
cargo test -p coflow-cft --test type_checker type_checker_accepts_nullable_element_builtins
cargo test -p coflow-checker --test check nullable_element
```

Expected: type test may already pass; runtime tests should fail until evaluator skips nulls for min/max/sum and compares null for unique/contains.

- [ ] **Step 4: Type checker adjustments**

Keep helper functions accepting nullable element arrays:

```rust
pub(super) fn unique_supported(ty: &Ty) -> bool {
    matches!(unwrap_nullable(ty), Ty::Int | Ty::Bool | Ty::String | Ty::Enum(_))
}

pub(super) fn min_max_supported(ty: &Ty) -> bool {
    matches!(unwrap_nullable(ty), Ty::Int | Ty::Float | Ty::Enum(_))
}
```

For `sum`, accept `Ty::Nullable(inner)` when `inner` is `Int` or `Float`, and return the non-null numeric type.

For `contains`, allow `null` as the value when array element type is nullable.

- [ ] **Step 5: Evaluator implementation**

In `eval_call` for `unique`, allow `CheckValue::Null` as comparable. If `values_equal` already handles null equality, remove the diagnostic for null elements.

In `eval_min_max`, skip `CheckValue::Null` items:

```rust
let mut out: Option<CheckValue> = None;
for item in items {
    if matches!(item, CheckValue::Null) {
        continue;
    }
    // existing comparable min/max logic against out
}
let Some(out) = out else {
    self.diag_at(CfdErrorCode::CheckEvalTypeError, arg_value.path, "min/max called with no non-null values");
    return Err(());
};
```

In `eval_sum`, ignore `CheckValue::Null` items and keep empty/all-null returning zero based on the declared `element_type`.

In `contains_value`, no special case is needed if `values_equal(CheckValue::Null, CheckValue::Null)` returns true; otherwise add it.

- [ ] **Step 6: Verify**

Run:

```powershell
cargo test -p coflow-cft --test type_checker type_checker_accepts_nullable_element_builtins
cargo test -p coflow-checker --test check
```

Expected: targeted and checker tests pass.

---

### Task 6: JSON Export Complete Empty Tables

**Files:**
- Modify: `crates/coflow-json-export/src/lib.rs`
- Modify: `crates/coflow-json-export/tests/json_export.rs`

- [ ] **Step 1: Add failing export test**

Add to `crates/coflow-json-export/tests/json_export.rs`:

```rust
#[test]
fn exports_empty_tables_for_concrete_id_types() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item { @id id: string; }
            type Monster { @id id: string; }
        "#,
    )?;

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("Item", [("id", CfdInputValue::from("item_1"))]);
    let model = build_model(builder)?;
    let tables = export_tables(&schema, &model)?;

    assert_eq!(tables["Item"], json!([{ "id": "item_1" }]));
    assert_eq!(tables["Monster"], json!([]));
    Ok(())
}
```

- [ ] **Step 2: Verify failing behavior**

Run:

```powershell
cargo test -p coflow-json-export --test json_export exports_empty_tables_for_concrete_id_types
```

Expected: fails because `Monster` is missing.

- [ ] **Step 3: Implement schema-driven table enumeration**

In `coflow-json-export/src/lib.rs`, change `export()` to iterate schema types:

```rust
for schema_type in self.schema.all_types() {
    if schema_type.is_abstract {
        continue;
    }
    if !schema_type.all_fields.iter().any(|field| has_annotation(&field.annotations, "id")) {
        continue;
    }
    let value = if let Some(table) = self.model.table(&schema_type.name) {
        self.encode_table(table)?
    } else {
        Value::Array(Vec::new())
    };
    out.insert(schema_type.name.clone(), value);
}
```

Use or add a local helper equivalent to the existing annotation lookup rather than importing codegen internals.

- [ ] **Step 4: Verify**

Run:

```powershell
cargo test -p coflow-json-export --test json_export
```

Expected: JSON export tests pass.

---

### Task 7: C# Codegen Trusted Loader And Struct Defaults

**Files:**
- Modify: `crates/coflow-codegen-csharp/src/emit.rs`
- Modify: `crates/coflow-codegen-csharp/src/lib.rs`
- Modify: `docs/spec/06-csharp-codegen.md` if not already completed in Task 1
- Optional modify: `tests/cli.rs`

- [ ] **Step 1: Add failing codegen unit test for struct defaults**

Add to `crates/coflow-codegen-csharp/src/lib.rs` tests:

```rust
#[test]
fn codegen_does_not_emit_struct_property_initializers() -> Result<(), String> {
    let schema = compile_schema(
        r#"
            @struct
            sealed type StatBlock {
                speed: float = 1.0;
                crit: int = 5;
            }

            type Item {
                @id id: string;
                stats: StatBlock = {};
            }
        "#,
    )?;

    let files = generate_csharp_json(&schema, &CsharpCodegenOptions::new("Game.Config"))
        .map_err(|err| err.to_string())?;
    let stat_block = generated_file(&files, "StatBlock.cs")?;
    require_contains(stat_block, "public partial struct StatBlock")?;
    require_not_contains(stat_block, "= 1.0f;")?;
    require_not_contains(stat_block, "= 5;")?;
    let database = generated_file(&files, "GameConfig.cs")?;
    require_contains(database, "Speed = ReadWithDefault")?;
    require_contains(database, "Crit = ReadWithDefault")?;
    Ok(())
}
```

Add helper:

```rust
fn require_not_contains(contents: &str, needle: &str) -> Result<(), String> {
    if contents.contains(needle) {
        Err(format!("expected generated output not to contain `{needle}`"))
    } else {
        Ok(())
    }
}
```

- [ ] **Step 2: Verify failing behavior**

Run:

```powershell
cargo test -p coflow-codegen-csharp codegen_does_not_emit_struct_property_initializers
```

Expected: fails because struct property initializers are currently emitted.

- [ ] **Step 3: Implement struct initializer suppression**

In `emit.rs`, determine whether `schema_type` has `@struct` once in `build_csharp_type`. For struct fields, pass `initializer: None`. For class fields, keep the current initializer behavior.

Example approach:

```rust
let is_struct = has_annotation(&schema_type.annotations, "struct");
...
initializer: if is_struct { None } else { default_initializer(field, &field_ty, view) },
```

Do not change loader default reading logic.

- [ ] **Step 4: Align trusted-loader docs/comments**

Update Rustdoc in `crates/coflow-codegen-csharp/src/lib.rs`:

```rust
/// The emitted loader is a trusted artifact loader for JSON produced by
/// `coflow export json`; it is not a validator for arbitrary JSON.
```

Only simplify `CftLoadException` or template validation code if it is straightforward and tests remain focused. Do not remove lookup construction needed by generated APIs.

- [ ] **Step 5: Optional generated C# compile verification**

If `.NET SDK` is available, run the manual verification:

```powershell
cargo run --quiet -- export json examples/rpg --out target/review-json
cargo run --quiet -- codegen csharp examples/rpg --out target/review-csharp
```

Then compile generated files in a temporary .NET project with Newtonsoft.Json. Expected: no CS8983 struct initializer error.

- [ ] **Step 6: Verify**

Run:

```powershell
cargo test -p coflow-codegen-csharp
cargo test --test cli codegen_csharp_writes_newtonsoft_json_loader
```

Expected: codegen tests and CLI codegen smoke test pass.

---

### Task 8: Excel Loader Cell Conversion Errors

**Files:**
- Modify: `crates/coflow-excel-loader/src/lib.rs`
- Modify: `crates/coflow-excel-loader/tests/excel_loader.rs`

- [ ] **Step 1: Add failing tests**

Add tests to `crates/coflow-excel-loader/tests/excel_loader.rs` using the existing workbook helper patterns in that file:

```rust
use rust_xlsxwriter::{ExcelDateTime, Formula};

#[test]
fn rejects_excel_error_cells() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item {
                id: string;
                value: string;
            }
        "#,
    )?;
    let path = temp_xlsx_path("error-cell");
    let mut workbook = Workbook::new();
    let sheet = workbook
        .add_worksheet()
        .set_name("Item")
        .map_err(|err| format!("{err:?}"))?;
    sheet.write_string(0, 0, "id").map_err(|err| format!("{err:?}"))?;
    sheet.write_string(0, 1, "value").map_err(|err| format!("{err:?}"))?;
    sheet.write_string(1, 0, "item_1").map_err(|err| format!("{err:?}"))?;
    sheet
        .write_formula(1, 1, Formula::new("=1/0").set_result("#DIV/0!"))
        .map_err(|err| format!("{err:?}"))?;
    workbook.save(&path).map_err(|err| format!("{err:?}"))?;

    let source = ExcelSource::new(&path, vec![ExcelSheet::new("Item")]);
    let Err(err) = load_excel_model(&schema, &[source]) else {
        return Err("expected unsupported cell error".to_string());
    };

    let ExcelLoadError::UnsupportedCellValue { location, kind } = err else {
        return Err(format!("expected unsupported cell error, got {err:?}"));
    };
    assert_eq!(location.sheet.as_deref(), Some("Item"));
    assert_eq!(location.row, Some(2));
    assert_eq!(location.column, Some(2));
    assert!(kind.contains("Error"), "kind: {kind}");
    Ok(())
}

#[test]
fn rejects_native_excel_datetime_cells() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item {
                id: string;
                value: string;
            }
        "#,
    )?;
    let path = temp_xlsx_path("datetime-cell");
    let mut workbook = Workbook::new();
    let sheet = workbook
        .add_worksheet()
        .set_name("Item")
        .map_err(|err| format!("{err:?}"))?;
    sheet.write_string(0, 0, "id").map_err(|err| format!("{err:?}"))?;
    sheet.write_string(0, 1, "value").map_err(|err| format!("{err:?}"))?;
    sheet.write_string(1, 0, "item_1").map_err(|err| format!("{err:?}"))?;
    let date = ExcelDateTime::from_ymd(2026, 6, 9).map_err(|err| format!("{err:?}"))?;
    sheet
        .write_datetime(1, 1, &date)
        .map_err(|err| format!("{err:?}"))?;
    workbook.save(&path).map_err(|err| format!("{err:?}"))?;

    let source = ExcelSource::new(&path, vec![ExcelSheet::new("Item")]);
    let Err(err) = load_excel_model(&schema, &[source]) else {
        return Err("expected unsupported cell error".to_string());
    };

    let ExcelLoadError::UnsupportedCellValue { location, kind } = err else {
        return Err(format!("expected unsupported cell error, got {err:?}"));
    };
    assert_eq!(location.sheet.as_deref(), Some("Item"));
    assert_eq!(location.row, Some(2));
    assert_eq!(location.column, Some(2));
    assert!(kind.contains("DateTime"), "kind: {kind}");
    Ok(())
}

#[test]
fn accepts_boolean_cells_for_bool_fields() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item {
                id: string;
                enabled: bool;
            }
        "#,
    )?;
    let path = temp_xlsx_path("bool-cell");
    let mut workbook = Workbook::new();
    let sheet = workbook
        .add_worksheet()
        .set_name("Item")
        .map_err(|err| format!("{err:?}"))?;
    sheet.write_string(0, 0, "id").map_err(|err| format!("{err:?}"))?;
    sheet.write_string(0, 1, "enabled").map_err(|err| format!("{err:?}"))?;
    sheet.write_string(1, 0, "item_1").map_err(|err| format!("{err:?}"))?;
    sheet.write_boolean(1, 1, true).map_err(|err| format!("{err:?}"))?;
    workbook.save(&path).map_err(|err| format!("{err:?}"))?;

    let source = ExcelSource::new(&path, vec![ExcelSheet::new("Item")]);
    let model = load_excel_model(&schema, &[source]).map_err(|err| format!("{err:?}"))?;
    let record = model
        .records_of_type("Item")
        .next()
        .map(|(_, record)| record)
        .ok_or_else(|| "expected item record".to_string())?;
    assert_eq!(record.field("enabled"), Some(&CfdValue::Bool(true)));
    Ok(())
}
```

Add `Formula` and `ExcelDateTime` to the existing `rust_xlsxwriter` imports. Use the existing workbook writer utilities rather than introducing a new Excel library.

- [ ] **Step 2: Verify failing behavior**

Run:

```powershell
cargo test -p coflow-excel-loader --test excel_loader rejects_excel_error_cells rejects_native_excel_datetime_cells accepts_boolean_cells_for_bool_fields
```

Expected: first two fail because `cell_text` currently stringifies these values; bool may already pass.

- [ ] **Step 3: Add explicit error type**

In `ExcelLoadError`, add a variant such as:

```rust
UnsupportedCellValue {
    location: ExcelLocation,
    kind: String,
}
```

Use the crate's existing `ExcelLocation` type. Report one-based row and column values, matching existing `CellParse` diagnostics.

- [ ] **Step 4: Convert `cell_text` to fallible conversion**

Change:

```rust
fn cell_text(cell: Option<&Data>) -> String
```

to a fallible helper for contexts where source location is known:

```rust
fn cell_text(cell: Option<&Data>, location: ExcelLocation) -> Result<String, ExcelLoadError>
```

Behavior:

- `None` and `Data::Empty` -> empty string;
- `Data::String`, `Data::DateTimeIso`, `Data::DurationIso` -> string value;
- `Data::Float` / `Data::Int` -> current numeric formatting;
- `Data::Bool` -> `"true"` or `"false"`;
- `Data::DateTime` -> `UnsupportedCellValue`;
- `Data::Error` -> `UnsupportedCellValue`.

Update header parsing, row parsing, and empty-row detection carefully. Empty-row detection may keep a non-fallible local helper that treats only `None`, `Empty`, and empty strings as empty.

- [ ] **Step 5: Rustdoc updates**

Update `load_excel_model` Rustdoc to say it does not run CFT checks. Update `load_excel` Rustdoc to say it runs checks.

- [ ] **Step 6: Verify**

Run:

```powershell
cargo test -p coflow-excel-loader --test excel_loader
```

Expected: Excel loader tests pass.

---

### Task 9: Generated C# End-To-End Verification

**Files:**
- Modify: `tests/cli.rs` or add a new ignored/manual integration test if .NET dependency should not be required by default.

- [ ] **Step 1: Decide default vs ignored**

If CI has .NET SDK and network-restored Newtonsoft.Json available, make this a normal test. Otherwise mark it `#[ignore]` and document manual execution.

- [ ] **Step 2: Add a C# compile/load test**

Use `tests/cli.rs` patterns to:

1. export JSON for `examples/rpg` to a temp directory;
2. generate C# to a temp directory;
3. create a temporary .NET console project;
4. copy generated files into it;
5. add `Newtonsoft.Json`;
6. run a program that calls `GameConfig.Load(exportDir)`;
7. assert `dotnet build` and `dotnet run` succeed.

The generated program should be minimal:

```csharp
using Game.Config;

var config = GameConfig.Load(args[0]);
if (config.Items.Count == 0)
{
    throw new Exception("expected items");
}
Console.WriteLine("loaded");
```

- [ ] **Step 3: Verify locally**

Run, depending on ignore choice:

```powershell
cargo test --test cli generated_csharp_compiles_and_loads_exported_json -- --ignored
```

or:

```powershell
cargo test --test cli generated_csharp_compiles_and_loads_exported_json
```

Expected: generated C# compiles and loads official export output.

---

### Task 10: Full Workspace Verification

**Files:**
- No source changes beyond previous tasks.

- [ ] **Step 1: Format**

Run:

```powershell
cargo fmt --all
```

Expected: completes successfully.

- [ ] **Step 2: Test workspace**

Run:

```powershell
cargo test --workspace
```

Expected: all Rust tests pass.

- [ ] **Step 3: Clippy**

Run:

```powershell
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: clippy passes with no warnings.

- [ ] **Step 4: Optional generated C# verification**

Run the ignored/manual C# integration test if it is not part of default tests.

Expected: generated C# compile/load verification passes.

- [ ] **Step 5: Review git status**

Run:

```powershell
git status --short
```

Expected: only intentional docs/source/test changes are present. Exclude temporary generated output under `examples/rpg/target/` and workspace `target/`.
