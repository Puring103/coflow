#![allow(
    clippy::expect_used,
    clippy::needless_borrow,
    clippy::needless_raw_string_hashes,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::redundant_field_names,
    clippy::unwrap_used
)]

use super::{load_cfd_model, parse_cfd_input_records, CfdTextErrorCode, CfdTextLoadError};
use crate as coflow_runtime;
use coflow_language::cft::{build_schema, parse_modules, CftFile, CftSchema, ModuleId};
use coflow_runtime::SourceLocation;
use coflow_runtime::{CfdValue, LoadedValueDraft};
use std::fs;
use std::path::{Path, PathBuf};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn shared_runtime_corpus_preserves_acceptance_and_value_states() {
    let dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/cfd-runtime-parity");
    let mut count = 0;
    for entry in fs::read_dir(&dir).expect("runtime corpus") {
        let path = entry.expect("fixture").path();
        if path.extension().is_none_or(|extension| extension != "cfd") {
            continue;
        }
        let name = path.file_name().expect("name").to_str().expect("UTF-8");
        let schema_name = format!("{}.cft", name.split('.').next().expect("fixture stem"));
        let schema =
            compile_schema(&fs::read_to_string(dir.join(schema_name)).expect("fixture schema"));
        let result = load_cfd_model(&schema, &fs::read_to_string(&path).expect("fixture source"));
        assert_eq!(
            result.is_ok(),
            name.ends_with(".valid.cfd"),
            "{name}: {result:?}"
        );
        count += 1;
        if name == "migration-contract.valid.cfd" {
            let model = result.expect("contract model");
            let record = model
                .record_by_type_id("ContractEntry", "entry")
                .and_then(|id| model.record(id))
                .expect("entry");
            let Some(CfdValue::Object(payload)) = record.field("payload") else {
                panic!("inline payload");
            };
            assert!(
                matches!(payload.field("label"), Some(CfdValue::FormattedString(value)) if value.rendered == "score 2147483648")
            );
            assert!(
                matches!(payload.field("labels"), Some(CfdValue::Array(values)) if matches!(&values[0], CfdValue::FormattedString(value) if value.rendered == "array 2147483648"))
            );
            assert!(
                matches!(payload.field("template"), Some(CfdValue::OptionSome(value)) if matches!(&**value, CfdValue::FormattedString(text) if text.rendered == "optional 2147483648"))
            );
            assert!(
                matches!(record.field("own_id"), Some(CfdValue::FormattedString(value)) if value.rendered == "entry")
            );
            assert_eq!(
                record.field("plain_text"),
                Some(&CfdValue::String("{self.id}".into()))
            );
            assert_eq!(record.field("wide"), Some(&CfdValue::Int(i64::MIN)));
            assert_eq!(
                record.field("precise"),
                Some(&CfdValue::Float(1.0000000000000002))
            );
            assert_eq!(
                record.field("nested"),
                Some(&CfdValue::OptionSome(Box::new(CfdValue::OptionNone)))
            );
            let Some(CfdValue::FormattedString(text)) = record.field("scalar_text") else {
                panic!("evaluated scalar template");
            };
            assert_eq!(
                text.rendered,
                format!(
                    "-9223372036854775808|1.0000000000000002|1{}",
                    "0".repeat(100)
                )
            );
        }
    }
    assert!(
        count >= 7,
        "shared runtime corpus must cover all regression fixtures"
    );
}

fn runtime_parity_fixture(name: &str) -> String {
    fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/cfd-runtime-parity")
            .join(name),
    )
    .expect("shared CFD runtime parity fixture")
}

fn compile_schema(source: &str) -> CftSchema {
    let modules = parse_modules([CftFile::from_source(ModuleId::from("main"), source)]);
    build_schema(&modules).expect("schema should compile")
}

fn compile_schema_files(files: &[(&str, &str)]) -> CftSchema {
    let modules = parse_modules(
        files
            .iter()
            .map(|(name, source)| CftFile::from_source(ModuleId::from(*name), *source)),
    );
    build_schema(&modules).expect("schema should compile")
}

#[test]
fn project_global_names_resolve_types_enums_dict_keys_and_references() -> TestResult {
    let schema = compile_schema_files(&[
        ("common.cft", "enum Rarity { Common, Rare }"),
        (
            "items.cft",
            r#"
table Item {
  rarity: Rarity;
  weights: {Rarity: int};
  backup: Option<&Item> = None;
}
"#,
        ),
    ]);

    let records = parse_cfd_input_records(
        &schema,
        r#"
Item {
  sword {
    rarity: Rarity::Rare,
    weights: { Rarity::Common: 1 },
  }
  shield {
    rarity: Rarity::Common,
    weights: { Rarity::Rare: 2 },
    backup: &Item::sword,
  }
}
"#,
    )?;

    assert_eq!(records.len(), 2);
    assert!(records.iter().all(|record| record.actual_type == "Item"));
    assert_eq!(
        records[1].fields.get("backup"),
        Some(&LoadedValueDraft::OptionSome(Box::new(
            LoadedValueDraft::TypedRecordRef {
                type_name: "Item".to_string(),
                id: "sword".to_string()
            }
        )))
    );
    Ok(())
}

#[test]
fn cfd_loading_rejects_removed_result_constructors_and_unit_values() {
    let schema =
        compile_schema("table Item { value: int; maybe: Option<int> = None; items: [int] = []; }");
    for fields in [
        "value: Ok(1)",
        "value: Err(\"bad\")",
        "value: ()",
        "value: 1, maybe: Some(Ok(2))",
        "value: 1, items: [Ok(2)]",
    ] {
        let source = format!("item: Item {{ {fields} }}");
        assert!(
            load_cfd_model(&schema, &source).is_err(),
            "removed constructor or value must fail: {source}"
        );
    }
}

#[test]
fn cfd_rejects_removed_namespace_and_use_headers() {
    let schema = compile_schema("table Item {}");

    let unknown = parse_cfd_input_records(&schema, "use missing::Item; Item { value {} }")
        .expect_err("use is not syntax");
    assert_has_text_code(&unknown, CfdTextErrorCode::Syntax);

    let conflict = parse_cfd_input_records(&schema, "namespace game; Item { value {} }")
        .expect_err("namespace is not syntax");
    assert_has_text_code(&conflict, CfdTextErrorCode::Syntax);
}

#[test]
fn records_use_colon_blocks_and_do_not_emit_id_fields() -> TestResult {
    let schema = compile_schema(
        r#"
            table Item {
                name: string;
            }
        "#,
    );

    let records = parse_cfd_input_records(
        &schema,
        r#"
            sword: Item {
                name: "Iron Sword",
            }
        "#,
    )?;

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, "sword");
    assert_eq!(records[0].actual_type, "Item");
    assert_eq!(
        records[0].fields.get("name"),
        Some(&LoadedValueDraft::from("Iron Sword"))
    );
    assert!(!records[0].fields.contains_key("id"));
    Ok(())
}

#[test]
fn nested_option_tags_survive_loading() -> TestResult {
    let schema = compile_schema(
        r#"
            table Item {
                nested: Option<Option<int>>;
            }
        "#,
    );
    let model = load_cfd_model(
        &schema,
        r#"
            item: Item {
                nested: Some(None),
            }
        "#,
    )?;
    let item_id = model
        .lookup_assignable(&schema, "Item", "item")
        .expect("item record");
    let item = model.record(item_id).expect("item record");
    assert_eq!(
        item.field("nested"),
        Some(&CfdValue::OptionSome(Box::new(CfdValue::OptionNone)))
    );
    Ok(())
}

#[test]
fn string_fields_require_quotes() {
    let schema = compile_schema("table Item { name: string; }");
    let error = parse_cfd_input_records(&schema, "item: Item { name: sword, }")
        .expect_err("bare strings must be rejected");

    assert_has_text_code(&error, CfdTextErrorCode::TypeMismatch);
    let CfdTextLoadError::Text(diagnostics) = error else {
        panic!("expected text diagnostics");
    };
    assert!(diagnostics
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message == "expected string"));
}

#[test]
fn bool_fields_accept_only_lowercase_cfd_literals() {
    let schema = compile_schema("table Item { enabled: bool; }");
    for value in ["TRUE", "True", "FALSE", "False", "1", "0", "yes", "no"] {
        let source = format!("item: Item {{ enabled: {value}, }}");
        let error = parse_cfd_input_records(&schema, &source)
            .expect_err("non-canonical bool literal must be rejected");
        assert_has_text_code(&error, CfdTextErrorCode::TypeMismatch);
    }
}

#[test]
fn formatted_strings_resolve_cross_record_fields_and_preserve_source() -> TestResult {
    let schema = compile_schema(
        r#"
            enum Rarity { Common, Rare, }
            data Stats { hp: int; }
            table Item {
                name: string;
                enabled: bool;
                price: float;
                rarity: Rarity;
                stats: Stats;
                tags: [string];
            }
            table Holder {
                item: &Item;
                message: string;
            }
        "#,
    );
    let source = r#"
        sword: Item {
            name: "Iron Sword",
            enabled: true,
            price: 12.5,
            rarity: Rare,
            stats: { hp: 30 },
            tags: ["weapon", "melee"],
        }
        holder: Holder {
            item: &sword,
            message: f"<b>{&Item::sword.name}</b> {&Item::sword.enabled} {&Item::sword.price} {&Item::sword.rarity} {&Item::sword.stats.hp}",
        }
    "#;

    let model = load_cfd_model(&schema, source)?;
    let holder = model
        .record(model.record_by_type_id("Holder", "holder").expect("holder"))
        .expect("holder record");
    let CfdValue::FormattedString(message) = holder.field("message").expect("message") else {
        panic!("expected formatted string");
    };
    assert!(message.source.starts_with("f\"<b>"));
    assert_eq!(message.rendered, "<b>Iron Sword</b> true 12.5 Rare 30");
    Ok(())
}

#[test]
fn formatted_strings_follow_record_reference_fields() -> TestResult {
    let schema = compile_schema(
        "table Item { name: string; } table Holder { item: &Item; message: string; }",
    );
    let model = load_cfd_model(
        &schema,
        r#"
            sword: Item { name: "Iron Sword" }
            holder: Holder { item: &sword, message: f"{self.item.name}" }
        "#,
    )?;
    let holder = model
        .record(model.record_by_type_id("Holder", "holder").expect("holder"))
        .expect("holder record");
    assert!(matches!(
        holder.field("message"),
        Some(CfdValue::FormattedString(value)) if value.rendered == "Iron Sword"
    ));
    Ok(())
}

#[test]
fn formatted_strings_resolve_fields_on_another_record_of_the_same_type() -> TestResult {
    let schema = compile_schema("table Item { name: string; message: string; }");
    let model = load_cfd_model(
        &schema,
        r#"
            sword: Item { name: "Iron Sword", message: f"source" }
            shield: Item { name: "Iron Shield", message: f"{&Item::sword.name}" }
        "#,
    )?;
    let shield = model
        .record(model.record_by_type_id("Item", "shield").expect("shield"))
        .expect("shield record");
    assert!(matches!(
        shield.field("message"),
        Some(CfdValue::FormattedString(value)) if value.rendered == "Iron Sword"
    ));
    Ok(())
}

#[test]
fn ref_type_fields_parse_key_only_refs() -> TestResult {
    let schema = compile_schema(
        r#"
            table Item { name: string; }
            table Holder {
                item: &Item;
            }
        "#,
    );

    let records = parse_cfd_input_records(
        &schema,
        r#"
            sword: Item { name: "Iron Sword" }

            holder: Holder {
                item: &sword,
            }
        "#,
    )?;

    assert_eq!(
        records[1].fields.get("item"),
        Some(&LoadedValueDraft::record_ref("sword"))
    );

    let model = load_cfd_model(
        &schema,
        r#"
            sword: Item { name: "Iron Sword" }
            holder: Holder {
                item: &sword,
            }
        "#,
    )?;

    let _item_id = model
        .lookup_assignable(&schema, "Item", "sword")
        .expect("item record");
    let holder_id = model
        .lookup_assignable(&schema, "Holder", "holder")
        .expect("holder record");
    let holder = model.record(holder_id).expect("holder");
    assert_eq!(
        holder.field("item"),
        Some(&CfdValue::record_ref("sword").unwrap())
    );
    Ok(())
}

#[test]
fn flag_enum_fields_accept_expressions_and_integer_masks() -> TestResult {
    let schema = compile_schema(
        r#"
            @flag enum Access { Empty = 0, Read = 1, Write = 2, Execute = 4, Admin = 8 }
            table User { access: Access; }
        "#,
    );
    let records = parse_cfd_input_records(
        &schema,
        r#"
            alice: User { access: Read | Write & (Execute | Access::Admin) }
            bob: User { access: 5 }
        "#,
    )?;
    assert_eq!(
        records[0].fields.get("access"),
        Some(&LoadedValueDraft::enum_value("Access", 1))
    );
    assert_eq!(
        records[1].fields.get("access"),
        Some(&LoadedValueDraft::enum_value("Access", 5))
    );

    let model = load_cfd_model(&schema, "alice: User { access: Read | Write }")?;
    let alice = model
        .lookup_assignable(&schema, "User", "alice")
        .and_then(|id| model.record(id))
        .expect("alice record");
    let Some(CfdValue::Enum(access)) = alice.field("access") else {
        panic!("expected enum value");
    };
    assert_eq!(access.value, 3);
    assert_eq!(access.variant, None);
    Ok(())
}

#[test]
fn flag_enum_fields_reject_invalid_operands_and_non_flag_expressions() {
    let schema = compile_schema(
        r#"
            @flag enum Access { Read = 1, Write = 2 }
            enum Rarity { Common, Rare }
            table User { access: Access; rarity: Rarity; }
        "#,
    );
    for source in [
        "alice: User { access: Missing | Read, rarity: Common }",
        "alice: User { access: Other::Read | Write, rarity: Common }",
        "alice: User { access: -1, rarity: Common }",
        "alice: User { access: 4, rarity: Common }",
        "alice: User { access: Read, rarity: Common | Rare }",
    ] {
        let error = parse_cfd_input_records(&schema, source).expect_err(source);
        assert!(matches!(error, CfdTextLoadError::Text(_)));
    }
}

#[test]
fn cfd_rejects_invalid_reference_syntax_and_bare_object_keys() {
    let schema = compile_schema(
        r#"
            table Item { name: string; }
            table Holder { item: &Item; }
        "#,
    );

    let invalid_at = parse_cfd_input_records(
        &schema,
        r#"
            sword: Item { name: "Iron Sword" }
            holder: Holder { item: @sword }
        "#,
    )
    .expect_err("@key is invalid");
    assert_has_text_code(&invalid_at, CfdTextErrorCode::Syntax);

    let direct_path = parse_cfd_input_records(
        &schema,
        r#"
            sword: Item { name: "Iron Sword" }
            holder: Holder { item: &sword.name }
        "#,
    )
    .expect_err("&key must not support paths");
    assert_has_text_code(&direct_path, CfdTextErrorCode::Syntax);

    let bare = parse_cfd_input_records(
        &schema,
        r#"
            sword: Item { name: "Iron Sword" }
            holder: Holder { item: sword }
        "#,
    )
    .expect_err("object references must use markers");
    assert_has_text_code(&bare, CfdTextErrorCode::Syntax);
}

#[test]
fn grouped_records_expand_to_records_of_the_same_type() -> TestResult {
    let schema = compile_schema(
        r#"
            table Item { name: string; }
        "#,
    );

    let records = parse_cfd_input_records(
        &schema,
        r#"
            Item {
                sword { name: "Sword" }
                shield { name: "Shield" }
            }
        "#,
    )?;

    assert_eq!(records.len(), 2);
    assert_eq!(records[0].id, "sword");
    assert_eq!(records[0].actual_type, "Item");
    assert_eq!(records[1].id, "shield");
    assert_eq!(records[1].actual_type, "Item");
    Ok(())
}

#[test]
fn grouped_record_commas_are_optional() -> TestResult {
    let schema = compile_schema(
        r#"
            table Item { name: string; }
        "#,
    );

    let records = parse_cfd_input_records(
        &schema,
        r#"
            Item {
                sword { name: "Sword" },
                shield { name: "Shield" }
                bow { name: "Bow" },
            }
        "#,
    )?;

    let coords = records
        .iter()
        .map(|record| (record.actual_type.as_str(), record.id.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        coords,
        vec![("Item", "sword"), ("Item", "shield"), ("Item", "bow")]
    );
    Ok(())
}

#[test]
fn cfd_rejects_slash_slash_comments() {
    let schema = compile_schema(
        r#"
            table Item { name: string; }
        "#,
    );

    let err = parse_cfd_input_records(
        &schema,
        r#"
            // not a CFD comment
            sword: Item { name: "Sword" }
        "#,
    )
    .expect_err("only # comments should be accepted");

    assert_has_text_code(&err, CfdTextErrorCode::Syntax);
}

#[test]
fn schema_free_ast_matches_loader_record_coordinates_for_supported_syntax() -> TestResult {
    let schema = compile_schema(
        r#"
            table Item {
                name: string;
                tags: [string] = [];
            }
            abstract table Reward {}
            table ItemReward : Reward { item: &Item; count: int; }
            table CurrencyReward : Reward { amount: int; }
        "#,
    );
    let source = r#"
        # group commas are optional
        Item {
            sword { name: "Sword", tags: ["weapon", "melee"] }
            shield { name: "Shield", tags: ["armor"], },
        }

        Reward {
            item_reward: ItemReward {
                item: &sword,
                count: 1,
            }
            coin_reward: CurrencyReward { amount: 50 },
        }
    "#;

    let loader_records = parse_cfd_input_records(&schema, source)?;
    let (ast, diagnostics) = coflow_language::cfd::parse_cfd(source);
    assert!(
        diagnostics.is_empty(),
        "schema-free parser diagnostics: {diagnostics:?}"
    );

    let loader_coords = loader_records
        .iter()
        .map(|record| (record.actual_type.as_str(), record.id.as_str()))
        .collect::<Vec<_>>();
    let ast_coords = ast
        .records
        .iter()
        .map(|record| (record.type_name.as_str(), record.id.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(ast_coords, loader_coords);
    Ok(())
}

#[test]
fn grouped_polymorphic_records_can_choose_concrete_types() -> TestResult {
    let schema = compile_schema(
        r#"
            table Item { name: string; }
            abstract table Reward {}
            table CurrencyReward : Reward { amount: int; }
            table ItemReward : Reward { item: &Item; count: int; }
        "#,
    );

    let model = load_cfd_model(
        &schema,
        r#"
            sword: Item { name: "Sword" }

            Reward {
                coin: CurrencyReward { amount: 100 }
                item: ItemReward { item: &sword, count: 1 }
            }
        "#,
    )?;

    let coin_id = model
        .lookup_assignable(&schema, "CurrencyReward", "coin")
        .expect("currency reward");
    let item_id = model
        .lookup_assignable(&schema, "ItemReward", "item")
        .expect("item reward");
    assert_eq!(
        model.lookup_assignable(&schema, "Reward", "coin"),
        Some(coin_id)
    );
    assert_eq!(
        model.lookup_assignable(&schema, "Reward", "item"),
        Some(item_id)
    );
    Ok(())
}

#[test]
fn cfd_enforces_ref_and_inline_types() -> TestResult {
    let schema = compile_schema(
        r#"
            table Item { name: string; }
            data InlineItem { name: string; }

            table Holder {
                ref_item: &Item;
                inline_item: InlineItem;
            }
        "#,
    );

    load_cfd_model(
        &schema,
        r#"
            sword: Item { name: "Sword" }
            holder: Holder {
                ref_item: &sword,
                inline_item: { name: "Inline" },
            }
        "#,
    )?;

    let mode_err = load_cfd_model(
        &schema,
        r#"
            sword: Item { name: "Sword" }
            holder: Holder {
                ref_item: { name: "Bad" },
                inline_item: { name: "Inline" },
            }
        "#,
    )
    .expect_err("CFD should enforce schema ref/inline types");
    assert_has_text_code(&mode_err, CfdTextErrorCode::Syntax);

    Ok(())
}

#[test]
fn cfd_rejects_reserved_id_fields() {
    let schema = compile_schema(
        r#"
            table Item {
                name: string;
            }
        "#,
    );

    let err = parse_cfd_input_records(
        &schema,
        r#"
            sword: Item {
                id: "sword",
                name: "Iron Sword",
            }
        "#,
    )
    .expect_err("id must be reserved");

    assert_has_text_code(&err, CfdTextErrorCode::ReservedIdField);
}

#[test]
fn cfd_allows_cyclic_record_references() -> TestResult {
    let schema = compile_schema(&runtime_parity_fixture("record-references.cft"));

    let model = load_cfd_model(
        &schema,
        r#"
Node {
  a { next: Some(&b) }
  b { next: Some(&a) }
}
"#,
    )?;

    assert_eq!(model.record_count(), 2);
    assert_eq!(model.ref_edges().count(), 2);
    Ok(())
}

#[test]
fn cfd_allows_acyclic_record_reference_chains() -> TestResult {
    let schema = compile_schema(&runtime_parity_fixture("record-references.cft"));

    let model = load_cfd_model(
        &schema,
        &runtime_parity_fixture("record-references.valid.cfd"),
    )?;
    assert_eq!(model.record_count(), 2);
    Ok(())
}

#[test]
fn cfd_loads_shared_complex_runtime_values() -> TestResult {
    let schema = compile_schema(&runtime_parity_fixture("complex-values.cft"));

    let model = load_cfd_model(&schema, &runtime_parity_fixture("complex-values.valid.cfd"))?;

    assert_eq!(model.record_count(), 1);
    Ok(())
}

#[test]
fn cfd_rejects_shared_complex_runtime_unknown_fields() {
    let schema = compile_schema(&runtime_parity_fixture("complex-values.cft"));

    load_cfd_model(
        &schema,
        &runtime_parity_fixture("complex-values.invalid.cfd"),
    )
    .expect_err("shared runtime fixture contains an unknown field");
}

#[test]
fn cfd_rejects_invalid_record_reference_forms() {
    let schema = compile_schema(
        r#"
            enum Element { Fire, Ice, }
            data Item { name: string; }
            table Tables {
                by_name: {string: Item};
                by_element: {Element: Item};
            }
            table Holder {
                named: Item;
                elemental: Item;
            }
        "#,
    );

    let err = parse_cfd_input_records(
        &schema,
        r#"
            tables: Tables {
                by_name: { "main": { name: "Main" } },
                by_element: { Fire: { name: "Fire" } },
            }
            holder: Holder {
                named: @Tables.tables.by_name["main"],
                elemental: @Tables.tables.by_element[Element.Fire],
            }
        "#,
    )
    .expect_err("invalid record reference should be rejected");
    assert_has_text_code(&err, CfdTextErrorCode::Syntax);
}

#[test]
fn cfd_rejects_invalid_record_reference_in_scalar_field() {
    let schema = compile_schema(
        r#"
            enum Element { Fire, Ice, }
            table Tables {
                resistances: {Element: float};
                labels: {string: string};
            }
            table Holder {
                fire_resistance: float;
                label: string;
            }
        "#,
    );

    let source = r#"
        tables: Tables {
            resistances: { Fire: 0.5 },
            labels: { "main": "primary" },
        }
        holder: Holder {
            fire_resistance: @Tables.tables.resistances[Fire],
            label: @Tables.tables.labels["main"],
        }
    "#;

    let err = parse_cfd_input_records(&schema, source)
        .expect_err("invalid record reference should be rejected");
    assert_has_text_code(&err, CfdTextErrorCode::Syntax);
}

#[test]
fn cfd_rejects_check_blocks_as_data_syntax() {
    let schema = compile_schema(
        r#"
            table Item {
                name: string;
            }
        "#,
    );

    let err = parse_cfd_input_records(
        &schema,
        r#"
            sword: Item {
                name: "Iron Sword",
                check Valid { true }
            }
        "#,
    )
    .expect_err("check blocks are not CFD data syntax");

    assert_has_text_code(&err, CfdTextErrorCode::Syntax);
}

#[test]
fn direct_model_errors_keep_record_text_spans() -> TestResult {
    let schema = compile_schema("table Item { value: int; }");
    let err = load_cfd_model(&schema, "first: Item { value: 1 }\n\nsecond: Item {\n}\n")
        .expect_err("second record is missing value");
    let CfdTextLoadError::DataModel {
        diagnostics,
        origins,
    } = err
    else {
        return Err("expected data-model diagnostics".into());
    };
    let mapped = coflow_runtime::map_diagnostics_with_origins(diagnostics, &origins);
    let primary = mapped
        .diagnostics
        .first()
        .and_then(|diagnostic| diagnostic.primary.as_ref())
        .ok_or("expected mapped primary label")?;

    assert_eq!(
        primary.location,
        SourceLocation::FileSpan {
            path: PathBuf::new(),
            start_line: 2,
            start_character: 0,
            end_line: 3,
            end_character: 1,
        }
    );
    Ok(())
}

#[test]
fn cfd_text_error_codes_have_negative_and_adjacent_valid_cases() {
    let cases = [
        (
            CfdTextErrorCode::Syntax,
            "table Item { name: string; }",
            r#"sword Item { name: "Sword" }"#,
            r#"sword: Item { name: "Sword" }"#,
        ),
        (
            CfdTextErrorCode::UnknownType,
            "table Item { name: string; }",
            r#"sword: Missing { name: "Sword" }"#,
            r#"sword: Item { name: "Sword" }"#,
        ),
        (
            CfdTextErrorCode::AbstractObjectType,
            "abstract table Reward {} table CoinReward : Reward { amount: int; }",
            r#"reward: Reward {}"#,
            r#"reward: CoinReward { amount: 1 }"#,
        ),
        (
            CfdTextErrorCode::ObjectTypeMismatch,
            "abstract table Reward {} table CoinReward : Reward { amount: int; } table Item { name: string; }",
            r#"Reward { bad: Item { name: "Sword" } }"#,
            r#"Reward { coin: CoinReward { amount: 1 } }"#,
        ),
        (
            CfdTextErrorCode::UnknownField,
            "table Item { name: string; }",
            r#"sword: Item { missing: "Sword" }"#,
            r#"sword: Item { name: "Sword" }"#,
        ),
        (
            CfdTextErrorCode::DuplicateField,
            "table Item { name: string; }",
            r#"sword: Item { name: "Sword", name: "Blade" }"#,
            r#"sword: Item { name: "Sword" }"#,
        ),
        (
            CfdTextErrorCode::ReservedIdField,
            "table Item { name: string; }",
            r#"sword: Item { id: "sword", name: "Sword" }"#,
            r#"sword: Item { name: "Sword" }"#,
        ),
        (
            CfdTextErrorCode::TypeMismatch,
            "table Item { level: int; }",
            r#"sword: Item { level: "high" }"#,
            r#"sword: Item { level: 3 }"#,
        ),
        (
            CfdTextErrorCode::InvalidEnumVariant,
            "enum Rarity { Common, Rare, } table Item { rarity: Rarity; }",
            r#"sword: Item { rarity: Missing }"#,
            r#"sword: Item { rarity: Rarity::Rare }"#,
        ),
        (
            CfdTextErrorCode::Syntax,
            "table Item { name: string; } table Holder { item: &Item; }",
            r#"sword: Item { name: "Sword" } holder: Holder { item: sword }"#,
            r#"sword: Item { name: "Sword" } holder: Holder { item: &sword }"#,
        ),
    ];

    for (code, schema_source, invalid_source, adjacent_valid_source) in cases {
        let schema = compile_schema(schema_source);
        let err = match parse_cfd_input_records(&schema, invalid_source) {
            Ok(records) => panic!("{code:?} case should fail, got {records:?}"),
            Err(err) => err,
        };
        assert_has_text_code(&err, code);
        parse_cfd_input_records(&schema, adjacent_valid_source)
            .unwrap_or_else(|err| panic!("{code:?} adjacent-valid case should parse: {err:?}"));
    }
}

#[test]
fn lowering_collects_independent_errors_across_fields_and_records() {
    let schema = compile_schema(
        r#"
            table Item {
                count: int;
                enabled: bool;
            }
        "#,
    );
    let error = parse_cfd_input_records(
        &schema,
        r#"
            first: Item { count: nope, enabled: maybe }
            second: Item { count: still_nope, enabled: perhaps }
        "#,
    )
    .expect_err("all four values are invalid");
    let CfdTextLoadError::Text(diagnostics) = error else {
        panic!("expected text diagnostics");
    };
    assert_eq!(diagnostics.diagnostics.len(), 4, "{diagnostics:?}");
    assert!(diagnostics
        .diagnostics
        .iter()
        .all(|diagnostic| diagnostic.code == CfdTextErrorCode::TypeMismatch));
}

#[test]
fn showcase_files_load_together() -> TestResult {
    let examples_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/showcase");
    let schema_source = [
        "schema/01-records.cft",
        "schema/02-defaults.cft",
        "schema/03-enums.cft",
        "schema/04-flags.cft",
        "schema/05-arrays.cft",
        "schema/06-dictionaries.cft",
        "schema/07-inheritance.cft",
        "schema/08-references.cft",
        "schema/09-options.cft",
        "schema/10-checks.cft",
        "schema/11-conditional-checks.cft",
        "schema/12-quantifiers.cft",
    ]
    .into_iter()
    .map(|path| fs::read_to_string(examples_dir.join(path)))
    .collect::<Result<Vec<_>, _>>()?
    .join("\n");
    let schema = compile_schema(&schema_source);
    let source = [
        "data/01-records.cfd",
        "data/02-defaults.cfd",
        "data/03-enums.cfd",
        "data/04-flags.cfd",
        "data/05-arrays.cfd",
        "data/06-dictionaries.cfd",
        "data/07-inheritance.cfd",
        "data/08-references.cfd",
        "data/09-options.cfd",
        "data/10-checks.cfd",
        "data/11-conditional-checks.cfd",
        "data/12-quantifiers.cfd",
    ]
    .into_iter()
    .map(|path| fs::read_to_string(examples_dir.join(path)))
    .collect::<Result<Vec<_>, _>>()?
    .join("\n");

    let model = load_cfd_model(&schema, &source)?;

    let product_id = model
        .lookup_assignable(&schema, "Product", "notebook")
        .expect("notebook product");
    let product = model.record(product_id).expect("notebook product record");
    assert_eq!(
        product.field("name"),
        Some(&CfdValue::String("Notebook".to_string()))
    );

    let bundle_id = model
        .lookup_assignable(&schema, "EffectBundle", "starter_effects")
        .expect("effect bundle");
    let bundle = model.record(bundle_id).expect("effect bundle record");
    let Some(CfdValue::Object(primary)) = bundle.field("primary") else {
        panic!("expected polymorphic primary effect");
    };
    assert_eq!(primary.actual_type.as_str(), "HealEffect");
    assert_eq!(primary.field("amount"), Some(&CfdValue::Int(0)));
    Ok(())
}

#[test]
fn cft_data_defaults_materialize_as_cfd_values() -> TestResult {
    let schema = compile_schema(
        r#"
table Rule {
  name: string;
  label: string = f"rule {self.name}";
  effect: Effect = Damage { amount: 2 };
  permissions: Permission = Permission::Read | Permission::Write;
}
abstract data Effect { amount: int; }
data Damage: Effect {}
@flag enum Permission { Read = 1, Write = 2 }
"#,
    );
    let model = load_cfd_model(&schema, "item: Rule { name: \"primary\" }")?;
    let record_id = model
        .record_by_type_id("Rule", "item")
        .expect("defaulted data record");
    let record = model.record(record_id).expect("defaulted data value");
    assert!(matches!(
        record.field("label"),
        Some(CfdValue::FormattedString(value)) if value.rendered == "rule primary"
    ));
    assert!(matches!(
        record.field("effect"),
        Some(CfdValue::Object(value)) if value.actual_type.as_str() == "Damage"
            && value.field("amount") == Some(&CfdValue::Int(2))
    ));
    assert!(matches!(
        record.field("permissions"),
        Some(CfdValue::Enum(value)) if value.value == 3
    ));
    Ok(())
}

fn assert_has_text_code(err: &CfdTextLoadError, code: CfdTextErrorCode) {
    let CfdTextLoadError::Text(diagnostics) = err else {
        panic!("expected text diagnostics, got {err:?}");
    };
    assert!(
        diagnostics
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == code),
        "expected {code:?}, got {:?}",
        diagnostics
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code)
            .collect::<Vec<_>>()
    );
}

#[test]
fn dimension_blocks_replace_selected_fields_without_configured_variants() -> TestResult {
    let schema = compile_schema(
        r#"
        table Item {
            @dimension("language") name: string;
            @dimension("language") note: Option<string>;
            price: int;
        }
    "#,
    );
    let model = load_cfd_model(
        &schema,
        r#"
        sword: Item { name: "Sword", note: Some("note"), price: 10 }
        language[ja] { sword: Item { name: "剣", note: None } }
        language[en] {}
    "#,
    )?;
    let id = model.record_by_type_id("Item", "sword").expect("sword");
    assert!(
        matches!(model.dimension_field_value(&schema, id, "name", "language", "ja"),
        Ok(coflow_runtime::DimensionValueLookup::Value { value: CfdValue::String(value), .. }) if value == "剣")
    );
    assert!(matches!(
        model.dimension_field_value(&schema, id, "note", "language", "ja"),
        Ok(coflow_runtime::DimensionValueLookup::ExplicitNone { .. })
    ));
    assert!(matches!(
        model.dimension_field_value(&schema, id, "name", "language", "unknown"),
        Ok(coflow_runtime::DimensionValueLookup::Missing)
    ));
    assert_eq!(
        model.record(id).expect("record").field("price"),
        Some(&CfdValue::Int(10))
    );
    assert_eq!(
        model
            .dimension_variants("language")
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        ["en", "ja"]
    );
    for overlay in [
        "language[ja] { missing: Item { name: \"剣\" } }",
        "language[ja] { sword: Item { price: 20 } }",
        "language[ja] { sword: Item { name: None } }",
        "language[ja] { sword: Item { name: \"剣\" } } language[ja] { sword: Item { name: \"刀\" } }",
    ] {
        let source = format!("sword: Item {{ name: \"Sword\", note: None, price: 10 }} {overlay}");
        assert!(load_cfd_model(&schema, &source).is_err(), "{overlay}");
    }
    Ok(())
}

#[test]
fn formatted_string_terminals_reject_composite_and_optional_numbers() {
    let schema = compile_schema("data Stats { hp: int; } table Item { numbers: [int]; stats: Stats; optional: Option<int>; target: &Item; message: string; }");
    for terminal in ["numbers", "stats", "optional", "target"] {
        let source = format!("item: Item {{ numbers: [1], stats: {{ hp: 1 }}, optional: 1, target: &item, message: f\"{{self.{terminal}}}\" }}");
        let diagnostics =
            load_cfd_model(&schema, &source).expect_err("interpolation must end at a scalar");
        assert!(
            diagnostics.to_string().contains("interpolation must end"),
            "{diagnostics:?}"
        );
    }
}

#[test]
fn explicit_reference_type_errors_remain_repairable_in_partial_models() {
    let schema =
        compile_schema("table Parent {} table Child: Parent {} table Holder { target: &Parent; }");
    let records = parse_cfd_input_records(
        &schema,
        "base: Parent {} holder: Holder { target: &Child::base }",
    )
    .expect("syntax and declared reference type");
    let mut builder = crate::CfdDataModel::builder(&schema);
    for record in records {
        builder.add_loaded_record(record);
    }
    let output = builder
        .build_partial()
        .expect("partial model retains invalid reference");
    assert!(output.model.record_by_type_id("Holder", "holder").is_some());
    assert!(output
        .diagnostics
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == crate::CfdErrorCode::RefTargetTypeMismatch));
    assert!(load_cfd_model(
        &schema,
        "base: Parent {} holder: Holder { target: &Child::base }"
    )
    .is_err());
}

#[test]
fn numeric_widths_are_enforced_for_source_constants_and_nested_values() {
    let schema = compile_schema("const WIDE: long = 1; const PRECISE: double = 0.1; @flag enum Bits { A = 1, B = 8 } table T { i: int; l: long; f: float; d: double; nested: [Option<int>] = []; keys: {int: string} = {}; bits: Bits = Bits::Everything; }");
    let model = load_cfd_model(
        &schema,
        "a: T { i: 2147483647, l: 9223372036854775807, f: 0.1, d: 0.1 }",
    )
    .unwrap();
    let record = model
        .record(model.record_by_type_id("T", "a").unwrap())
        .unwrap();
    assert_eq!(
        record.field("f"),
        Some(&CfdValue::Float(f64::from(0.1_f32)))
    );
    assert_eq!(record.field("d"), Some(&CfdValue::Float(0.1)));
    assert!(matches!(record.field("bits"), Some(CfdValue::Enum(v)) if v.value == 9));
    for fields in [
        "i: 2147483648, l: 0, f: 0.0, d: 0.0",
        "i: WIDE, l: 0, f: 0.0, d: 0.0",
        "i: 0, l: 0, f: PRECISE, d: 0.0",
        "i: 0, l: 0, f: 1e100, d: 0.0",
        "i: 0, l: 0, f: 0.0, d: 0.0, nested: [Some(2147483648)]",
        "i: 0, l: 0, f: 0.0, d: 0.0, keys: {2147483648: \"bad\"}",
    ] {
        assert!(
            load_cfd_model(&schema, &format!("a: T {{ {fields} }}")).is_err(),
            "{fields}"
        );
    }
}
