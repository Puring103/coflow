#![allow(
    clippy::expect_used,
    clippy::needless_borrow,
    clippy::needless_raw_string_hashes,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::redundant_field_names,
    clippy::unwrap_used
)]

use coflow_api::{
    ResolvedSource, SourceLoadContext, SourceLocation, SourceLocationSpec, SourceProvider,
};
use coflow_cft::{build_schema, parse_modules, CftDimensionInputs, CftFile, CftSchema, ModuleId};
use coflow_data_model::CfdDataModel;
use coflow_data_model::{CfdValue, LoadedValueDraft};
use coflow_loader_cfd::{
    load_cfd_model, parse_cfd_input_records, CfdLoader, CfdTextErrorCode, CfdTextLoadError,
};
use std::fs;
use std::path::{Path, PathBuf};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn compile_schema(source: &str) -> CftSchema {
    let modules = parse_modules([CftFile::from_source(ModuleId::from("main"), source)]);
    build_schema(&modules, &CftDimensionInputs::default()).expect("schema should compile")
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
fn string_fields_require_quotes() {
    let schema = compile_schema("type Item { name: string; }");
    let error = parse_cfd_input_records(&schema, "item: Item { name: sword, }")
        .expect_err("bare strings must be rejected");

    assert_has_text_code(&error, CfdTextErrorCode::TypeMismatch);
    let CfdTextLoadError::Text(diagnostics) = error else {
        panic!("expected text diagnostics");
    };
    assert!(
        diagnostics
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message == "expected string")
    );
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
    let (ast, diagnostics) = coflow_cfd::parse_cfd(source);
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
fn cfd_object_and_dict_spreads_merge_before_local_overrides() -> TestResult {
    let schema = compile_schema(
        r#"
            enum Element { Fire, Ice, }
            type Stats { hp: int; attack: int; }
            type Monster {
                name: string;
                stats: Stats;
                weights: {Element: int};
            }
        "#,
    );

    let model = load_cfd_model(
        &schema,
        r#"
            base: Monster {
                name: "Base",
                stats: { hp: 100, attack: 20 },
                weights: { Fire: 10, Ice: 5 },
            }

            elite: Monster {
                ...&base,
                name: "Elite",
                stats: {
                    ...{ hp: 100, attack: 15 },
                    hp: 180,
                },
                weights: {
                    ...{ Fire: 10, Ice: 5 },
                    Fire: 20,
                },
            }
        "#,
    )?;

    let elite_id = model
        .lookup_assignable(&schema, "Monster", "elite")
        .expect("elite record");
    let elite = model.record(elite_id).expect("elite");
    assert_eq!(
        elite.field("name"),
        Some(&CfdValue::String("Elite".to_string()))
    );
    let Some(CfdValue::Object(stats)) = elite.field("stats") else {
        panic!("expected stats object");
    };
    assert_eq!(stats.field("hp"), Some(&CfdValue::Int(180)));
    assert_eq!(stats.field("attack"), Some(&CfdValue::Int(15)));
    let Some(CfdValue::Dict(weights)) = elite.field("weights") else {
        panic!("expected weights dict");
    };
    assert_eq!(weights.len(), 2);
    assert!(weights.iter().any(|(_, value)| value == &CfdValue::Int(20)));
    assert!(weights.iter().any(|(_, value)| value == &CfdValue::Int(5)));
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
fn cfd_allows_cyclic_record_references() -> TestResult {
    let schema = compile_schema(
        r#"
            type Node {
                label: string;
                next: &Node? = null;
            }
        "#,
    );

    let model = load_cfd_model(
        &schema,
        r#"
            a: Node { label: "A", next: &b }
            b: Node { label: "B", next: &a }
        "#,
    )?;

    let a_id = model
        .lookup_assignable(&schema, "Node", "a")
        .expect("a record");
    let b_id = model
        .lookup_assignable(&schema, "Node", "b")
        .expect("b record");
    assert_eq!(
        model.record(a_id).and_then(|record| record.field("next")),
        Some(&CfdValue::record_ref("b").unwrap())
    );
    assert_eq!(
        model.record(b_id).and_then(|record| record.field("next")),
        Some(&CfdValue::record_ref("a").unwrap())
    );
    Ok(())
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
    let root = std::env::temp_dir().join("coflow-cfd-loader-origin-spans");
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
            SourceLoadContext {
                project_root: &root,
                schema: schema,
            },
            &ResolvedSource {
                provider_id: "cfd".to_string(),
                location: SourceLocationSpec::new(source_path.clone()),
                options: CfdLoader
                    .decode_options(&serde_json::Value::Null)
                    .expect("decode cfd options"),
                display_name: source_path.display().to_string(),
            },
        )
        .map_err(|diagnostics| format!("{diagnostics:?}"))?;
    let origins = coflow_api::origins_of(&loaded.records);
    let mut builder = CfdDataModel::builder(&schema);
    for record in loaded.records {
        builder.add_loaded_record(record);
    }
    let err = builder.build().expect_err("second record is missing value");
    let mapped = coflow_api::map_diagnostics_with_origins(err, &origins);
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
    let mapped = coflow_api::map_diagnostics_with_origins(diagnostics, &origins);
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
            r#"sword: Item { rarity: Rarity.Rare }"#,
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
fn examples_cfd_files_load_together() -> TestResult {
    let examples_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/cfd");
    let schema = compile_schema(&fs::read_to_string(examples_dir.join("schema.cft"))?);
    let source = [
        "data/01-records.cfd",
        "data/02-polymorphic-and-paths.cfd",
        "data/03-spread.cfd",
    ]
    .into_iter()
    .map(|path| fs::read_to_string(examples_dir.join(path)))
    .collect::<Result<Vec<_>, _>>()?
    .join("\n");

    let model = load_cfd_model(&schema, &source)?;

    let elite_id = model
        .lookup_assignable(&schema, "Monster", "elite_monster")
        .expect("elite monster");
    let elite = model.record(elite_id).expect("elite monster record");
    assert_eq!(
        elite.field("name"),
        Some(&CfdValue::String("Elite Training Dummy".to_string()))
    );

    let Some(CfdValue::Object(stats)) = elite.field("stats") else {
        panic!("expected stats object");
    };
    assert_eq!(stats.field("hp"), Some(&CfdValue::Int(250)));
    assert_eq!(stats.field("attack"), Some(&CfdValue::Int(5)));

    let encounter_id = model
        .lookup_assignable(&schema, "Encounter", "elite_encounter")
        .expect("elite encounter");
    let encounter = model.record(encounter_id).expect("encounter record");
    assert_eq!(
        encounter.field("weakness_hint"),
        Some(&CfdValue::Float(1.5))
    );
    assert!(matches!(
        encounter.field("featured_item"),
        Some(CfdValue::Ref(target_key)) if target_key.as_str() == "sword_fire"
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
