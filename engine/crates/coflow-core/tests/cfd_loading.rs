#![allow(
    clippy::expect_used,
    clippy::needless_borrow,
    clippy::needless_raw_string_hashes,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::redundant_field_names,
    clippy::unwrap_used
)]

use coflow_core::schema::{build_schema, parse_modules, CftFile, CftSchema, ModuleId};
use coflow_core::loading::{self, CfdTextErrorCode, CfdTextLoadError, SourceInput};
use coflow_core::{CfdDataModel, CfdValue, LoadedRecordDraft, LoadedValueDraft};
use std::fs;
use std::path::PathBuf;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn parse_cfd_input_records(
    schema: &CftSchema,
    source: &str,
) -> Result<Vec<LoadedRecordDraft>, CfdTextLoadError> {
    loading::parse_records(schema, source, Default::default())
        .map(|records| records.into_iter().map(|record| record.record).collect())
        .map_err(CfdTextLoadError::Text)
}

fn load_cfd_model(schema: &CftSchema, source: &str) -> Result<CfdDataModel, CfdTextLoadError> {
    loading::load(schema, [SourceInput::new("test.cfd", source)]).1
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
  backup: Item? = None;
}
"#,
        ),
    ]);

    let records = parse_cfd_input_records(
        &schema,
        r#"
sword: Item {
  rarity: Rarity::Rare,
  weights: { Rarity::Common: 1 },
}
shield: Item {
  rarity: Rarity::Common,
  weights: { Rarity::Rare: 2 },
  backup: &Item::sword,
}
"#,
    )?;

    assert_eq!(records.len(), 2);
    assert!(records.iter().all(|record| record.actual_type == "Item"));
    assert_eq!(
        records[1].fields.get("backup"),
        Some(&LoadedValueDraft::OptionSome(Box::new(
            LoadedValueDraft::record_ref("Item::sword")
        )))
    );
    Ok(())
}

#[test]
fn cfd_resolves_use_but_rejects_namespace_headers() {
    let schema = compile_schema("table Item {}");

    let unknown = parse_cfd_input_records(&schema, "use missing::Item;  value: Item {}")
        .expect_err("use is not syntax");
    assert_has_text_code(&unknown, CfdTextErrorCode::UnknownType);

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
fn nested_optional_declarations_are_rejected() {
    let modules = parse_modules([CftFile::from_source(
        ModuleId::from("main"),
        "table Item { nested: int??; }",
    )]);
    assert!(build_schema(&modules).is_err());
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
fn templates_preserve_source_without_evaluating_record_reads() -> TestResult {
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
                item: Item;
                message: fstring;
            }
        "#,
    );
    let source = r#"
        sword: Item {
            name: "Iron Sword",
            enabled: true,
            price: 12.5,
            rarity: Rare,
            stats: Stats { hp: 30 },
            tags: ["weapon", "melee"],
        }
        holder: Holder {
            item: &Item::sword,
            message: f"<b>{&Item::sword.name}</b> {&Item::sword.enabled} {&Item::sword.price} {&Item::sword.rarity} {&Item::sword.stats} {&Item::sword.tags}",
        }
    "#;

    let model = load_cfd_model(&schema, &source)?;
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
    assert!(message.source.starts_with("f\"<b>"));
    assert!(message.source.contains("{&Item::sword.name}"));
    Ok(())
}

#[test]
fn ordinary_strings_keep_braces_literal() -> TestResult {
    let schema = compile_schema(
        "table Item { name: string; } table Holder { item: Item; message: string; }",
    );
    let model = load_cfd_model(
        &schema,
        r#"
            sword: Item { name: "Iron Sword" }
            holder: Holder { item: &Item::sword, message: "{item.name}" }
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
        Some(CfdValue::String(value)) if value == "{item.name}"
    ));
    Ok(())
}

#[test]
fn ordinary_strings_do_not_resolve_reference_expressions() -> TestResult {
    let schema = compile_schema("table Item { name: string; message: string; }");
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
        Some(CfdValue::String(value)) if value == "{&sword.name}"
    ));
    Ok(())
}

#[test]
fn ref_type_fields_parse_key_only_refs() -> TestResult {
    let schema = compile_schema(
        r#"
            table Item { name: string; }
            table Holder {
                item: Item;
            }
        "#,
    );

    let records = parse_cfd_input_records(
        &schema,
        r#"
            sword: Item { name: "Iron Sword" }

            holder: Holder {
                item: &Item::sword,
            }
        "#,
    )?;

    assert_eq!(
        records[1].fields.get("item"),
        Some(&LoadedValueDraft::record_ref("Item::sword"))
    );

    let model = load_cfd_model(
        &schema,
        r#"
            sword: Item { name: "Iron Sword" }
            holder: Holder {
                item: &Item::sword,
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
            table Holder { item: Item; }
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
fn standalone_records_keep_source_order() -> TestResult {
    let schema = compile_schema(
        r#"
            table Item { name: string; }
        "#,
    );

    let records = parse_cfd_input_records(
        &schema,
        r#"
                          sword: Item { name: "Sword" }
              shield: Item { name: "Shield" }
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
fn standalone_records_allow_trailing_field_commas() -> TestResult {
    let schema = compile_schema(
        r#"
            table Item { name: string; }
        "#,
    );

    let records = parse_cfd_input_records(
        &schema,
        r#"
                          sword: Item { name: "Sword" }
              shield: Item { name: "Shield" }
              bow: Item { name: "Bow" }
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
            table ItemReward : Reward { item: Item; count: int; }
            table CurrencyReward : Reward { amount: int; }
        "#,
    );
    let source = r#"
        # group commas are optional
                  sword: Item { name: "Sword", tags: ["weapon", "melee"] }
          shield: Item { name: "Shield", tags: ["armor"], }

                  item_reward: ItemReward {
              item: &Item::sword,
              count: 1,
          }
          coin_reward: CurrencyReward { amount: 50 }
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
fn standalone_polymorphic_records_keep_concrete_types() -> TestResult {
    let schema = compile_schema(
        r#"
            table Item { name: string; }
            abstract table Reward {}
            table CurrencyReward : Reward { amount: int; }
            table ItemReward : Reward { item: Item; count: int; }
        "#,
    );

    let model = load_cfd_model(
        &schema,
        r#"
            sword: Item { name: "Sword" }

                          coin: CurrencyReward { amount: 100 }
              item: ItemReward { item: &Item::sword, count: 1 }
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
            table Item { name: string; } data ItemData { name: string; }

            table Holder {
                ref_item: Item;
                inline_item: ItemData;
            }
        "#,
    );

    load_cfd_model(
        &schema,
        r#"
            sword: Item { name: "Sword" }
            holder: Holder {
                ref_item: &Item::sword,
                inline_item: ItemData { name: "Inline" },
            }
        "#,
    )?;

    let mode_err = load_cfd_model(
        &schema,
        r#"
            sword: Item { name: "Sword" }
            holder: Holder {
                ref_item: { name: "Bad" },
                inline_item: ItemData { name: "Inline" },
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
a: Node { next: &b }
b: Node { next: &a }
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
                check { true }
            }
        "#,
    )
    .expect_err("check blocks are not CFD data syntax");

    assert_has_text_code(&err, CfdTextErrorCode::Syntax);
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
            "abstract data Reward {} data CoinReward : Reward { amount: int; } data Item { name: string; } table Holder { value: Reward; }",
            r#"holder: Holder { value: Item { name: "Sword" } }"#,
            r#"holder: Holder { value: CoinReward { amount: 1 } }"#,
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
            "table Item { name: string; } table Holder { item: Item; }",
            r#"sword: Item { name: "Sword" } holder: Holder { item: sword }"#,
            r#"sword: Item { name: "Sword" } holder: Holder { item: &Item::sword }"#,
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
fn function_values_are_retained_and_signature_checked() -> TestResult {
    let schema = compile_schema(
        r#"
table Rule {
  apply: fn(value: int, callback: fn(int) -> int) -> int?;
  factories: [fn(int) -> int];
}
"#,
    );
    let source = r#"
item: Rule {
  apply: fn(value: int, callback: fn(int) -> int) -> int? {
    Ok(callback(value))
  },
  factories: [fn(value: int) -> int { value + 1 }],
}
"#;
    let model = load_cfd_model(&schema, &source)?;
    let record_id = model
        .record_by_type_key("Rule", "item")
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
        "item: Rule { apply: fn(value: float, callback: fn(int) -> int) -> int? { Ok(1) }, factories: [] }",
        "item: Rule { apply: fn(value: int, callback: fn(int) -> int) -> int { 1 }, factories: [] }",
        "item: Rule { apply: fn(value: int, value: fn(int) -> int) -> int? { Ok(1) }, factories: [] }",
    ] {
        let error = load_cfd_model(&schema, invalid).expect_err("invalid function signature");
        assert_has_text_code(&error, CfdTextErrorCode::TypeMismatch);
    }
    Ok(())
}

#[test]
fn cft_formatted_string_and_function_defaults_materialize_as_cfd_values() -> TestResult {
    let schema = compile_schema(
        r#"
table Rule {
  name: string;
  label: fstring = f"rule {self.name}";
  apply: fn(value: int) -> int = fn(value: int) -> int { value + 1 };
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
        .record_by_type_key("Rule", "item")
        .expect("defaulted function record");
    let record = model.record(record_id).expect("defaulted function value");
    assert!(matches!(
        record.field("label"),
        Some(CfdValue::FormattedString(value)) if value.source == r#"f"rule {self.name}""#
    ));
    assert!(matches!(
        record.field("apply"),
        Some(CfdValue::Function(value)) if value.source.contains("value + 1")
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
