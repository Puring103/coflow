#![allow(
    clippy::expect_used,
    clippy::needless_borrow,
    clippy::needless_raw_string_hashes,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::redundant_field_names,
    clippy::unwrap_used
)]

use coflow_language::{
    build_schema, parse_modules, CftDimensionInputs, CftFile, CftSchema, ModuleId,
};
use coflow_runtime::CfdDataModel;
use coflow_runtime::{
    load_cfd_model, parse_cfd_input_records, CfdLoader, CfdTextErrorCode, CfdTextLoadError,
};
use coflow_runtime::{CfdErrorCode, CfdLoadContext, CfdSource, CfdSourcePath, SourceLocation};
use coflow_runtime::{CfdValue, LoadedValueDraft};
use std::fs;
use std::path::{Path, PathBuf};

type TestResult = Result<(), Box<dyn std::error::Error>>;

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
    build_schema(&modules, &CftDimensionInputs::default()).expect("schema should compile")
}

fn compile_schema_files(files: &[(&str, &str)]) -> CftSchema {
    let modules = parse_modules(
        files
            .iter()
            .map(|(name, source)| CftFile::from_source(ModuleId::from(*name), *source)),
    );
    build_schema(&modules, &CftDimensionInputs::default()).expect("schema should compile")
}

#[test]
fn cfd_namespace_and_uses_resolve_types_enums_dict_keys_and_references() -> TestResult {
    let schema = compile_schema_files(&[
        (
            "common.cft",
            "namespace shared::common; enum Rarity { Common, Rare }",
        ),
        (
            "items.cft",
            r#"
namespace game::items;
use shared::common::Rarity;
type Item {
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
namespace game::items;
use shared::common::Rarity as Quality;

Item {
  sword {
    rarity: Quality::Rare,
    weights: { Quality::Common: 1 },
  }
  shield {
    rarity: shared::common::Rarity::Common,
    weights: { shared::common::Rarity::Rare: 2 },
    backup: &Item::sword,
  }
}
"#,
    )?;

    assert_eq!(records.len(), 2);
    assert!(records
        .iter()
        .all(|record| record.actual_type == "game::items::Item"));
    assert_eq!(
        records[1].fields.get("backup"),
        Some(&LoadedValueDraft::OptionSome(Box::new(
            LoadedValueDraft::record_ref("sword")
        )))
    );
    Ok(())
}

#[test]
fn cfd_rejects_unknown_or_conflicting_uses_without_name_fallback() {
    let schema = compile_schema_files(&[
        ("root.cft", "type Item {}"),
        ("local.cft", "namespace game; type Item {}"),
        ("shared.cft", "namespace shared; type Item {}"),
    ]);

    let unknown = parse_cfd_input_records(&schema, "use missing::Item; Item { value {} }")
        .expect_err("unknown use target");
    assert_has_text_code(&unknown, CfdTextErrorCode::Syntax);

    let conflict = parse_cfd_input_records(
        &schema,
        "namespace game; use shared::Item; Item { value {} }",
    )
    .expect_err("unqualified use path or local declaration conflict");
    assert_has_text_code(&conflict, CfdTextErrorCode::Syntax);
}

#[test]
fn loader_rejects_non_cfd_sources_before_reading() {
    let loader = CfdLoader;
    let source = CfdSource {
        location: CfdSourcePath::new("data/items.json"),
        display_name: "data/items.json".to_string(),
    };

    let diagnostics = loader.resolve(&source).expect_err("only CFD is supported");
    assert!(diagnostics.contains("unsupported extension"));
}

#[test]
fn records_use_colon_blocks_and_do_not_emit_id_fields() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item {
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
    assert_eq!(records[0].key, "sword");
    assert_eq!(records[0].actual_type, "Item");
    assert_eq!(
        records[0].fields.get("name"),
        Some(&LoadedValueDraft::from("Iron Sword"))
    );
    assert!(!records[0].fields.contains_key("id"));
    Ok(())
}

#[test]
fn nested_option_and_result_tags_survive_loading() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item {
                nested: Option<Option<int>>;
                outcome: Result<Option<int>, string>;
            }
        "#,
    );
    let model = load_cfd_model(
        &schema,
        r#"
            item: Item {
                nested: Some(None),
                outcome: Ok(Some(3)),
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
    assert_eq!(
        item.field("outcome"),
        Some(&CfdValue::ResultOk(Box::new(CfdValue::OptionSome(
            Box::new(CfdValue::Int(3))
        ))))
    );
    Ok(())
}

#[test]
fn string_fields_require_quotes() {
    let schema = compile_schema("type Item { name: string; }");
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
    let schema = compile_schema("type Item { enabled: bool; }");
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
            type Stats { hp: int; }
            type Item {
                name: string;
                enabled: bool;
                price: float;
                rarity: Rarity;
                stats: Stats;
                tags: [string];
            }
            type Holder {
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
            message: "<b>{&Item::sword.name}</b> {&Item::sword.enabled} {&Item::sword.price} {&Item::sword.rarity} {&Item::sword.stats} {&Item::sword.tags}",
        }
    "#;

    let model = load_cfd_model(&schema, source)?;
    let holder = model
        .record(
            model
                .record_by_type_key("Holder", "holder")
                .expect("holder"),
        )
        .expect("holder record");
    let CfdValue::FormattedString(message) = holder.field("message").expect("message") else {
        panic!("expected formatted string");
    };
    assert!(message.source.starts_with("\"<b>"));
    assert_eq!(
        message.rendered,
        "<b>Iron Sword</b> true 12.5 Rare Stats{hp: 30} [\"weapon\", \"melee\"]"
    );
    Ok(())
}

#[test]
fn formatted_strings_follow_record_reference_fields() -> TestResult {
    let schema =
        compile_schema("type Item { name: string; } type Holder { item: &Item; message: string; }");
    let model = load_cfd_model(
        &schema,
        r#"
            sword: Item { name: "Iron Sword" }
            holder: Holder { item: &sword, message: "{item.name}" }
        "#,
    )?;
    let holder = model
        .record(
            model
                .record_by_type_key("Holder", "holder")
                .expect("holder"),
        )
        .expect("holder record");
    assert!(matches!(
        holder.field("message"),
        Some(CfdValue::FormattedString(value)) if value.rendered == "Iron Sword"
    ));
    Ok(())
}

#[test]
fn formatted_strings_resolve_fields_on_another_record_of_the_same_type() -> TestResult {
    let schema = compile_schema("type Item { name: string; message: string; }");
    let model = load_cfd_model(
        &schema,
        r#"
            sword: Item { name: "Iron Sword", message: "source" }
            shield: Item { name: "Iron Shield", message: "{&sword.name}" }
        "#,
    )?;
    let shield = model
        .record(model.record_by_type_key("Item", "shield").expect("shield"))
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
            type Item { name: string; }
            type Holder {
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
            type User { access: Access; }
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
            type User { access: Access; rarity: Rarity; }
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
            type Item { name: string; }
            type Holder { item: &Item; }
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
            type Item { name: string; }
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
    assert_eq!(records[0].key, "sword");
    assert_eq!(records[0].actual_type, "Item");
    assert_eq!(records[1].key, "shield");
    assert_eq!(records[1].actual_type, "Item");
    Ok(())
}

#[test]
fn grouped_record_commas_are_optional() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item { name: string; }
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
        .map(|record| (record.actual_type.as_str(), record.key.as_str()))
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
            type Item { name: string; }
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
            type Item {
                name: string;
                tags: [string] = [];
            }
            abstract type Reward {}
            type ItemReward : Reward { item: &Item; count: int; }
            type CurrencyReward : Reward { amount: int; }
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
        .map(|record| (record.actual_type.as_str(), record.key.as_str()))
        .collect::<Vec<_>>();
    let ast_coords = ast
        .records
        .iter()
        .map(|record| (record.type_name.as_str(), record.key.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(ast_coords, loader_coords);
    Ok(())
}

#[test]
fn grouped_polymorphic_records_can_choose_concrete_types() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item { name: string; }
            abstract type Reward {}
            type CurrencyReward : Reward { amount: int; }
            type ItemReward : Reward { item: &Item; count: int; }
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
            type Item { name: string; }

            type Holder {
                ref_item: &Item;
                inline_item: Item;
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
            type Item {
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
fn cfd_rejects_cyclic_record_references() {
    let schema = compile_schema(&runtime_parity_fixture("record-references.cft"));

    let error = load_cfd_model(
        &schema,
        &runtime_parity_fixture("record-reference-cycle.invalid.cfd"),
    )
    .expect_err("record reference cycles must be rejected consistently across runtimes");

    let CfdTextLoadError::DataModel { diagnostics, .. } = error else {
        panic!("expected a data-model reference diagnostic");
    };
    assert!(diagnostics
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == CfdErrorCode::RefCycle));
    assert!(diagnostics
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("Node:a -> Node:b -> Node:a")));
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
            type Item { name: string; }
            type Tables {
                by_name: {string: Item};
                by_element: {Element: Item};
            }
            type Holder {
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
            type Tables {
                resistances: {Element: float};
                labels: {string: string};
            }
            type Holder {
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
            type Item {
                name: string;
            }
        "#,
    );

    let err = parse_cfd_input_records(
        &schema,
        r#"
            sword: Item {
                name: "Iron Sword",
                check { true }
            }
        "#,
    )
    .expect_err("check blocks are not CFD data syntax");

    assert_has_text_code(&err, CfdTextErrorCode::Syntax);
}

#[test]
fn loader_file_origins_preserve_record_text_spans() -> TestResult {
    let schema = compile_schema("type Item { value: int; }");
    let schema = &schema;
    let root = std::env::temp_dir().join("coflow-language-loader-origin-spans");
    if root.exists() {
        fs::remove_dir_all(&root)?;
    }
    fs::create_dir_all(&root)?;
    let source_path = root.join("items.cfd");
    fs::write(
        &source_path,
        "first: Item { value: 1 }\n\nsecond: Item {\n}\n",
    )?;

    let cfd_loader = CfdLoader;
    let loaded = cfd_loader
        .load(
            CfdLoadContext {
                project_root: &root,
                schema: schema,
                source_text: None,
            },
            &CfdSource {
                location: CfdSourcePath::new(source_path.clone()),
                display_name: source_path.display().to_string(),
            },
        )
        .map_err(|diagnostics| format!("{diagnostics:?}"))?;
    let origins = coflow_runtime::origins_of(&loaded.records);
    let mut builder = CfdDataModel::builder(&schema);
    for record in loaded.records {
        builder.add_loaded_record(record);
    }
    let err = builder.build().expect_err("second record is missing value");
    let mapped = coflow_runtime::map_diagnostics_with_origins(err, &origins);
    let primary = mapped
        .diagnostics
        .first()
        .and_then(|diagnostic| diagnostic.primary.as_ref())
        .ok_or("expected mapped primary label")?;

    assert_eq!(
        primary.location,
        SourceLocation::FileSpan {
            path: source_path,
            start_line: 2,
            start_character: 0,
            end_line: 3,
            end_character: 1,
        }
    );
    Ok(())
}

#[test]
fn direct_model_errors_keep_record_text_spans() -> TestResult {
    let schema = compile_schema("type Item { value: int; }");
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
            "type Item { name: string; }",
            r#"sword Item { name: "Sword" }"#,
            r#"sword: Item { name: "Sword" }"#,
        ),
        (
            CfdTextErrorCode::UnknownType,
            "type Item { name: string; }",
            r#"sword: Missing { name: "Sword" }"#,
            r#"sword: Item { name: "Sword" }"#,
        ),
        (
            CfdTextErrorCode::AbstractObjectType,
            "abstract type Reward {} type CoinReward : Reward { amount: int; }",
            r#"reward: Reward {}"#,
            r#"reward: CoinReward { amount: 1 }"#,
        ),
        (
            CfdTextErrorCode::ObjectTypeMismatch,
            "abstract type Reward {} type CoinReward : Reward { amount: int; } type Item { name: string; }",
            r#"Reward { bad: Item { name: "Sword" } }"#,
            r#"Reward { coin: CoinReward { amount: 1 } }"#,
        ),
        (
            CfdTextErrorCode::UnknownField,
            "type Item { name: string; }",
            r#"sword: Item { missing: "Sword" }"#,
            r#"sword: Item { name: "Sword" }"#,
        ),
        (
            CfdTextErrorCode::DuplicateField,
            "type Item { name: string; }",
            r#"sword: Item { name: "Sword", name: "Blade" }"#,
            r#"sword: Item { name: "Sword" }"#,
        ),
        (
            CfdTextErrorCode::ReservedIdField,
            "type Item { name: string; }",
            r#"sword: Item { id: "sword", name: "Sword" }"#,
            r#"sword: Item { name: "Sword" }"#,
        ),
        (
            CfdTextErrorCode::TypeMismatch,
            "type Item { level: int; }",
            r#"sword: Item { level: "high" }"#,
            r#"sword: Item { level: 3 }"#,
        ),
        (
            CfdTextErrorCode::InvalidEnumVariant,
            "enum Rarity { Common, Rare, } type Item { rarity: Rarity; }",
            r#"sword: Item { rarity: Missing }"#,
            r#"sword: Item { rarity: Rarity::Rare }"#,
        ),
        (
            CfdTextErrorCode::Syntax,
            "type Item { name: string; } type Holder { item: &Item; }",
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
            type Item {
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
        "schema/13-functions.cft",
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
        "data/13-functions.cfd",
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
fn function_values_are_retained_and_signature_checked() -> TestResult {
    let schema = compile_schema(
        r#"
namespace app;
type Rule {
  apply: fn(value: int, callback: fn(int) -> int) -> Result<int, string>;
  factories: [fn(int) -> int];
}
"#,
    );
    let source = r#"
namespace app;
item: Rule {
  apply: fn(value: int, callback: fn(int) -> int) -> Result<int, string> {
    Ok(callback(value))
  },
  factories: [fn(value: int) -> int { value + 1 }],
}
"#;
    let model = load_cfd_model(&schema, source)?;
    let record_id = model
        .record_by_type_key("app::Rule", "item")
        .expect("function record");
    let record = model.record(record_id).expect("function record value");
    let Some(CfdValue::Function(function)) = record.field("apply") else {
        panic!("expected retained function");
    };
    assert!(function.source.contains("Ok(callback(value))"));
    let Some(CfdValue::Array(factories)) = record.field("factories") else {
        panic!("expected function array");
    };
    assert!(matches!(factories.as_slice(), [CfdValue::Function(_)]));

    for invalid in [
        "item: app::Rule { apply: fn(value: float, callback: fn(int) -> int) -> Result<int, string> { Ok(1) }, factories: [] }",
        "item: app::Rule { apply: fn(value: int, callback: fn(int) -> int) -> int { 1 }, factories: [] }",
        "item: app::Rule { apply: fn(value: int, value: fn(int) -> int) -> Result<int, string> { Ok(1) }, factories: [] }",
    ] {
        let error = load_cfd_model(&schema, invalid).expect_err("invalid function signature");
        assert_has_text_code(&error, CfdTextErrorCode::TypeMismatch);
    }
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
