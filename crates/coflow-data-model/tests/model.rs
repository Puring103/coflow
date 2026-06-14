#![allow(
    clippy::expect_used,
    clippy::needless_raw_string_hashes,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::too_many_lines,
    clippy::unwrap_used
)]

mod common;
use common::*;

#[test]
fn data_model_applies_defaults_and_builds_indexes_without_running_check() {
    let schema = compile_schema(
        r#"
            const DEFAULT_NAME = "unknown";
            enum Rarity { Common = 0, Rare = 10, }
            type Item {
                @id
                id: string;
                name: string = DEFAULT_NAME;
                @index
                rarity: Rarity = Rarity.Common;
                tags: [string] = [];
                attrs: {string: int} = {};
                check { id != ""; }
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("Item", [("id", CfdInputValue::from(""))]);
    let model = builder.build().expect("data model should build");

    let item_id = record_id_at(&model, 0);
    let table = model.table("Item").expect("item table");
    assert_eq!(table.records, vec![item_id]);
    assert!(table.primary_index.contains_key(&CfdIdValue::from("")));
    assert!(
        table.secondary_indexes["rarity"].contains_key(&CfdIndexKey::Enum(CfdEnumValue {
            enum_name: "Rarity".to_string(),
            variant: Some("Common".to_string()),
            value: 0,
        }))
    );

    let record = model.record(item_id).expect("record");
    assert_eq!(
        record.field("name"),
        Some(&CfdValue::String("unknown".to_string()))
    );
    assert_eq!(record.field("tags"), Some(&CfdValue::Array(Vec::new())));
    assert_eq!(record.field("attrs"), Some(&CfdValue::Dict(Vec::new())));
}

#[test]
fn polymorphic_refs_resolve_against_the_data_model() {
    let schema = compile_schema(
        r#"
            abstract type Reward { @id id: string; }
            type ItemReward : Reward { count: int; }
            type CurrencyReward : Reward { amount: int; }
            type Drop {
                @ref(Reward)
                reward_id: string;
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "ItemReward",
        [
            ("id", CfdInputValue::from("reward_1")),
            ("count", CfdInputValue::from(1_i64)),
        ],
    );
    builder.add_record(
        "Drop",
        [(
            "reward_id",
            CfdInputValue::Ref(CfdIdValue::from("reward_1")),
        )],
    );
    let model = builder.build().expect("data model should build");
    let item_reward_id = record_id_at(&model, 0);
    let drop_id = record_id_at(&model, 1);

    assert!(model
        .polymorphic_index("Reward")
        .unwrap()
        .records
        .contains_key(&CfdIdValue::from("reward_1")));
    assert_eq!(
        model
            .record(drop_id)
            .and_then(|record| record.field("reward_id")),
        Some(&CfdValue::Ref {
            id: CfdIdValue::from("reward_1"),
            target: item_reward_id,
        })
    );
}

#[test]
fn ref_field_defaults_are_resolved_as_references() {
    let schema = compile_schema(
        r#"
            type Item { @id id: string; }
            type Drop {
                @ref(Item)
                item_id: string = "default_item";
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("Item", [("id", CfdInputValue::from("default_item"))]);
    builder.add_input_record(CfdInputRecord::new(
        "Drop",
        std::iter::empty::<(&str, CfdInputValue)>(),
    ));
    let model = builder.build().expect("data model should build");
    let item_id = record_id_at(&model, 0);
    let drop_id = record_id_at(&model, 1);

    assert_eq!(
        model
            .record(drop_id)
            .and_then(|record| record.field("item_id")),
        Some(&CfdValue::Ref {
            id: CfdIdValue::from("default_item"),
            target: item_id,
        })
    );
}

#[test]
fn nullable_ref_defaults_accept_null_and_target_id_enum_names_are_validated() {
    let null_schema = compile_schema(
        r#"
            type Item { @id id: string; }
            type Drop {
                @ref(Item)
                item_id: string? = null;
            }
        "#,
    );
    let mut null_builder = CfdDataModel::builder(&null_schema);
    null_builder.add_input_record(CfdInputRecord::new(
        "Drop",
        std::iter::empty::<(&str, CfdInputValue)>(),
    ));
    let model = null_builder
        .build()
        .expect("nullable @ref default null should build");
    let drop_id = record_id_at(&model, 0);
    assert_eq!(
        model
            .record(drop_id)
            .and_then(|record| record.field("item_id")),
        Some(&CfdValue::Null)
    );

    let key_enum_schema = compile_schema(
        r#"
            type Gene {
                @IdAsEnum("GeneId")
                @id
                id: string;
            }
            type UseGene {
                @ref(Gene)
                gene_id: string = "bad-value";
            }
        "#,
    );
    let mut key_enum_builder = CfdDataModel::builder(&key_enum_schema);
    key_enum_builder.add_record("Gene", [("id", CfdInputValue::from("ValidGene"))]);
    key_enum_builder.add_input_record(CfdInputRecord::new(
        "UseGene",
        std::iter::empty::<(&str, CfdInputValue)>(),
    ));
    let err = key_enum_builder
        .build()
        .expect_err("@ref default into @IdAsEnum id should validate enum variant shape");
    assert_has_code(&err, CfdErrorCode::InvalidEnumVariant);
}

#[test]
fn scalar_defaults_cover_nullable_null_int_float_and_bool() {
    let schema = compile_schema(
        r#"
            type Settings {
                maybe: int? = null;
                count: int = 7;
                ratio: float = 2.5;
                enabled: bool = true;
            }
        "#,
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_input_record(CfdInputRecord::new(
        "Settings",
        std::iter::empty::<(&str, CfdInputValue)>(),
    ));

    let model = builder.build().expect("scalar defaults should build");
    let record_id = record_id_at(&model, 0);
    let record = model.record(record_id).expect("settings record");
    assert_eq!(record.field("maybe"), Some(&CfdValue::Null));
    assert_eq!(record.field("count"), Some(&CfdValue::Int(7)));
    assert_eq!(record.field("ratio"), Some(&CfdValue::Float(2.5)));
    assert_eq!(record.field("enabled"), Some(&CfdValue::Bool(true)));
}

#[test]
fn duplicate_ids_are_checked_inside_polymorphic_ranges() {
    let schema = compile_schema(
        r#"
            abstract type Reward { @id id: string; }
            type ItemReward : Reward { count: int; }
            type CurrencyReward : Reward { amount: int; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "ItemReward",
        [
            ("id", CfdInputValue::from("same")),
            ("count", CfdInputValue::from(1_i64)),
        ],
    );
    builder.add_record(
        "CurrencyReward",
        [
            ("id", CfdInputValue::from("same")),
            ("amount", CfdInputValue::from(10_i64)),
        ],
    );

    let err = builder.build().expect_err("duplicate polymorphic id");
    assert_has_code(&err, CfdErrorCode::DuplicatePolymorphicId);
    let diag = err
        .diagnostics
        .iter()
        .find(|diag| diag.code == CfdErrorCode::DuplicatePolymorphicId)
        .expect("diag");
    assert!(!diag.related.is_empty());
}

#[test]
fn duplicate_ids_in_concrete_table_report_related_label() {
    let schema = compile_schema(
        r#"
            type Item { @id id: string; value: int; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Item",
        [
            ("id", CfdInputValue::from("same")),
            ("value", CfdInputValue::from(1_i64)),
        ],
    );
    builder.add_record(
        "Item",
        [
            ("id", CfdInputValue::from("same")),
            ("value", CfdInputValue::from(2_i64)),
        ],
    );

    let err = builder.build().expect_err("duplicate concrete id");
    let diag = err
        .diagnostics
        .iter()
        .find(|diag| diag.code == CfdErrorCode::DuplicateId)
        .expect("duplicate id diagnostic");
    assert_eq!(
        diag.primary.as_ref().map(|label| label.path.clone()),
        Some(CfdPath::root().field("id"))
    );
    assert_eq!(diag.related.len(), 1);
}

#[test]
fn inline_objects_use_declared_type_when_not_polymorphic() {
    let schema = compile_schema(
        r#"
            type Stats { hp: int; speed: float = 1.0; }
            type Monster { stats: Stats; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Monster",
        [(
            "stats",
            CfdInputValue::object_with_declared_type([("hp", CfdInputValue::from(100_i64))]),
        )],
    );
    let model = builder.build().expect("data model should build");
    let monster_id = record_id_at(&model, 0);
    let Some(CfdValue::Object(stats)) = model
        .record(monster_id)
        .and_then(|record| record.field("stats"))
    else {
        panic!("expected stats object");
    };
    assert_eq!(stats.actual_type, "Stats");
    assert_eq!(stats.field("speed"), Some(&CfdValue::Float(1.0)));
}

#[test]
fn record_types_without_ids_build_tables_without_primary_indexes() {
    let schema = compile_schema(
        r#"
            type LogEntry { message: string; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("LogEntry", [("message", CfdInputValue::from("started"))]);
    let model = builder.build().expect("records without @id are allowed");
    let table = model.table("LogEntry").expect("LogEntry table");

    assert_eq!(table.records.len(), 1);
    assert!(table.primary_index.is_empty());
    assert!(model
        .lookup("LogEntry", &CfdIdValue::from("started"))
        .is_none());
}

#[test]
fn semantic_edges_report_data_model_diagnostics() {
    let schema = compile_schema(
        r#"
            enum Rarity { Common, Rare, }
            type Item {
                @id
                id: string;
                rarity: Rarity;
                maybe: int?;
                attrs: {string: int};
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Item",
        [
            ("id", CfdInputValue::from("item_1")),
            ("unknown", CfdInputValue::from(1_i64)),
            ("rarity", CfdInputValue::enum_variant("Rarity", "Missing")),
            (
                "attrs",
                CfdInputValue::dict([
                    (CfdInputDictKey::from("x"), CfdInputValue::from(1_i64)),
                    (CfdInputDictKey::from("x"), CfdInputValue::from(2_i64)),
                ]),
            ),
        ],
    );

    let err = builder.build().expect_err("data errors");
    assert_has_code(&err, CfdErrorCode::UnknownField);
    assert_has_code(&err, CfdErrorCode::MissingRequiredField);
    assert_has_code(&err, CfdErrorCode::InvalidEnumVariant);
    assert_has_code(&err, CfdErrorCode::DuplicateDictKey);
}

#[test]
fn build_collects_diagnostics_across_multiple_records() {
    let schema = compile_schema(
        r#"
            type Item { id: string; value: int; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Item",
        [
            ("id", CfdInputValue::from("a")),
            ("value", CfdInputValue::from("not_int")),
        ],
    );
    builder.add_record("MissingType", [("id", CfdInputValue::from("b"))]);

    let err = builder.build().expect_err("data errors");
    assert_has_code(&err, CfdErrorCode::TypeMismatch);
    assert_has_code(&err, CfdErrorCode::UnknownType);
}

#[test]
fn polymorphic_object_fields_need_actual_type_markers() {
    let schema = compile_schema(
        r#"
            abstract type Reward { id: string; }
            type CurrencyReward : Reward { amount: int; }
            type Drop { reward: Reward; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Drop",
        [(
            "reward",
            CfdInputValue::object_with_declared_type([
                ("id", CfdInputValue::from("r1")),
                ("amount", CfdInputValue::from(10_i64)),
            ]),
        )],
    );

    let err = builder.build().expect_err("missing object type");
    assert_has_code(&err, CfdErrorCode::MissingObjectType);
}

#[test]
fn default_empty_object_reports_missing_required_nested_fields() {
    let schema = compile_schema(
        r#"
            type Stats { hp: int; }
            type Monster { stats: Stats = {}; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_input_record(CfdInputRecord::new(
        "Monster",
        std::iter::empty::<(&str, CfdInputValue)>(),
    ));
    let err = builder
        .build()
        .expect_err("empty object default should still validate nested required fields");
    let diag = diagnostic_with_code(&err, CfdErrorCode::MissingRequiredField);
    assert_eq!(
        diag.primary.as_ref().map(|label| label.path.clone()),
        Some(CfdPath::root().field("stats").field("hp"))
    );
}

#[test]
fn ref_resolution_reports_missing_targets() {
    let missing_schema = compile_schema(
        r#"
            type Item { @id id: string; }
            type Drop { @ref(Item) item_id: string; }
        "#,
    );
    let mut missing_builder = CfdDataModel::builder(&missing_schema);
    missing_builder.add_record(
        "Drop",
        [("item_id", CfdInputValue::Ref(CfdIdValue::from("missing")))],
    );
    let missing = missing_builder.build().expect_err("missing ref target");
    assert_has_code(&missing, CfdErrorCode::RefTargetNotFound);
}

#[test]
fn nested_array_and_dict_refs_report_resolution_paths() {
    let schema = compile_schema(
        r#"
            type Item { @id id: string; }
            type Drop { @ref(Item) item_id: string; }
            type Loot {
                drops: [Drop];
                keyed: {string: Drop};
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Loot",
        [
            (
                "drops",
                CfdInputValue::Array(vec![CfdInputValue::object_with_declared_type([(
                    "item_id",
                    CfdInputValue::from("missing_array"),
                )])]),
            ),
            (
                "keyed",
                CfdInputValue::dict([(
                    CfdInputDictKey::from("slot"),
                    CfdInputValue::object_with_declared_type([(
                        "item_id",
                        CfdInputValue::from("missing_dict"),
                    )]),
                )]),
            ),
        ],
    );

    let err = builder
        .build()
        .expect_err("nested pending refs should fail during resolution");
    let paths = err
        .diagnostics
        .iter()
        .filter(|diag| diag.code == CfdErrorCode::RefTargetNotFound)
        .filter_map(|diag| diag.primary.as_ref().map(|label| label.path.clone()))
        .collect::<Vec<_>>();

    assert!(paths.contains(&CfdPath::root().field("drops").index(0).field("item_id")));
    assert!(paths.contains(
        &CfdPath::root()
            .field("keyed")
            .dict_key("\"slot\"")
            .field("item_id")
    ));
}

#[test]
fn non_nullable_ref_rejects_null_before_resolution() {
    let schema = compile_schema(
        r#"
            type Item { @id id: string; }
            type Drop { @ref(Item) item_id: string; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("Drop", [("item_id", CfdInputValue::Null)]);

    let err = builder
        .build()
        .expect_err("non-null ref should reject explicit null");
    let diag = err
        .diagnostics
        .iter()
        .find(|diag| diag.code == CfdErrorCode::TypeMismatch)
        .expect("type mismatch diagnostic");
    assert_eq!(
        diag.primary.as_ref().map(|label| label.path.clone()),
        Some(CfdPath::root().field("item_id"))
    );
}

#[test]
fn child_ref_range_remains_addressable_when_child_declares_id() {
    let schema = compile_schema(
        r#"
            type Base { name: string; }
            type Child : Base { @id id: string; }
            type ChildHolder {
                @ref(Child)
                child_id: string;
            }
        "#,
    );

    let mut child_ref_builder = CfdDataModel::builder(&schema);
    child_ref_builder.add_record(
        "Child",
        [
            ("name", CfdInputValue::from("child")),
            ("id", CfdInputValue::from("child_1")),
        ],
    );
    child_ref_builder.add_record(
        "ChildHolder",
        [("child_id", CfdInputValue::from("child_1"))],
    );
    let child_model = child_ref_builder
        .build()
        .expect("child ref range should remain addressable");
    assert!(child_model.polymorphic_index("Base").is_none());
}

#[test]
fn failed_input_diagnostics_keep_original_record_ordinals() {
    let schema = compile_schema(
        r#"
            type Item { id: string; value: int; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Item",
        [
            ("id", CfdInputValue::from("a")),
            ("value", CfdInputValue::from("not_int")),
        ],
    );
    builder.add_record("MissingType", [("id", CfdInputValue::from("b"))]);

    let err = builder.build().expect_err("data errors");
    let records = err
        .diagnostics
        .iter()
        .filter_map(|diag| diag.primary.as_ref().and_then(|label| label.record))
        .map(CfdRecordId::index)
        .collect::<Vec<_>>();
    assert!(
        records.contains(&0),
        "first invalid input record should be labelled as record 0: {records:?}"
    );
    assert!(
        records.contains(&1),
        "second invalid input record should be labelled as record 1: {records:?}"
    );
}

#[test]
fn abstract_records_and_refs_to_idless_targets_are_rejected() {
    let abstract_schema = compile_schema(
        r#"
            abstract type Reward { id: string; }
            type CoinReward : Reward { amount: int; }
        "#,
    );
    let mut abstract_builder = CfdDataModel::builder(&abstract_schema);
    abstract_builder.add_record("Reward", [("id", CfdInputValue::from("base"))]);
    let abstract_err = abstract_builder
        .build()
        .expect_err("abstract top-level record should fail");
    assert_has_code(&abstract_err, CfdErrorCode::AbstractRecordType);
}

#[test]
fn non_finite_float_inputs_are_rejected() {
    let input_schema = compile_schema(
        r#"
            type Item { value: float; }
        "#,
    );
    let mut input_builder = CfdDataModel::builder(&input_schema);
    input_builder.add_record("Item", [("value", CfdInputValue::from(f64::NAN))]);
    let input_err = input_builder
        .build()
        .expect_err("non-finite float input should fail");
    assert_has_code(&input_err, CfdErrorCode::TypeMismatch);
}

#[test]
fn dict_key_enum_type_mismatch_and_ref_id_type_mismatch_are_rejected() {
    let dict_schema = compile_schema(
        r#"
            enum Rarity { Common, Rare, }
            enum Element { Fire, Ice, }
            type Item {
                attrs: {Rarity: int};
            }
        "#,
    );
    let mut dict_builder = CfdDataModel::builder(&dict_schema);
    dict_builder.add_record(
        "Item",
        [(
            "attrs",
            CfdInputValue::dict([(
                CfdInputDictKey::enum_variant("Element", "Fire"),
                CfdInputValue::from(1_i64),
            )]),
        )],
    );
    let dict_err = dict_builder
        .build()
        .expect_err("wrong enum key type should fail");
    assert_has_code(&dict_err, CfdErrorCode::TypeMismatch);

    let ref_schema = compile_schema(
        r#"
            type Item { @id id: int; }
            type Drop { @ref(Item) item_id: int; }
        "#,
    );
    let mut ref_builder = CfdDataModel::builder(&ref_schema);
    ref_builder.add_record("Item", [("id", CfdInputValue::from(1_i64))]);
    ref_builder.add_record("Drop", [("item_id", CfdInputValue::from("1"))]);
    let ref_err = ref_builder
        .build()
        .expect_err("string ref id should not match int id field");
    assert_has_code(&ref_err, CfdErrorCode::TypeMismatch);
}

#[test]
fn key_as_enum_default_values_must_be_legal_csharp_variants() {
    let schema = compile_schema(
        r#"
            type GeneConfig {
                @IdAsEnum("GeneId")
                @id
                id: string = "bad-value";
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_input_record(CfdInputRecord::new(
        "GeneConfig",
        std::iter::empty::<(&str, CfdInputValue)>(),
    ));

    let err = builder
        .build()
        .expect_err("@IdAsEnum default should be validated");
    let diag = err
        .diagnostics
        .iter()
        .find(|diag| diag.code == CfdErrorCode::InvalidEnumVariant)
        .expect("invalid enum variant diagnostic");
    assert_eq!(
        diag.primary.as_ref().map(|label| label.path.clone()),
        Some(CfdPath::root().field("id"))
    );
}

#[test]
fn key_as_enum_input_values_reject_empty_and_keyword_variants() {
    let schema = compile_schema(
        r#"
            type Gene {
                @IdAsEnum("GeneId")
                @id
                id: string;
            }
            type GeneRef {
                @id
                ref_id: string;
                @GenAsEnum("GeneId")
                gene_id: string;
            }
        "#,
    );

    for value in ["", "class"] {
        let mut builder = CfdDataModel::builder(&schema);
        builder.add_record("Gene", [("id", CfdInputValue::from("ValidGene"))]);
        builder.add_record(
            "GeneRef",
            [
                ("ref_id", CfdInputValue::from(format!("ref_{value}"))),
                ("gene_id", CfdInputValue::from(value)),
            ],
        );
        let err = builder
            .build()
            .expect_err("invalid generated enum value should fail");
        assert_has_code(&err, CfdErrorCode::InvalidEnumVariant);
    }
}

#[test]
fn nullable_gen_as_enum_input_null_bypasses_variant_validation() {
    let schema = compile_schema(
        r#"
            type Gene {
                @IdAsEnum("GeneId")
                @id
                id: string;
            }
            type GeneRef {
                @id
                ref_id: string;
                @GenAsEnum("GeneId")
                gene_id: string? = null;
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("Gene", [("id", CfdInputValue::from("ValidGene"))]);
    builder.add_record(
        "GeneRef",
        [
            ("ref_id", CfdInputValue::from("ref_null")),
            ("gene_id", CfdInputValue::Null),
        ],
    );

    let model = builder
        .build()
        .expect("explicit null should be accepted for nullable @GenAsEnum fields");
    let record_id = record_id_at(&model, 1);
    assert_eq!(
        model
            .record(record_id)
            .and_then(|record| record.field("gene_id")),
        Some(&CfdValue::Null)
    );
}

#[test]
fn enum_value_type_mismatch_reports_expected_and_actual_enum_names() {
    let schema = compile_schema(
        r#"
            enum Rarity { Common, Rare, }
            enum Element { Fire, Ice, }
            type Item { rarity: Rarity; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Item",
        [("rarity", CfdInputValue::enum_variant("Element", "Fire"))],
    );

    let err = builder
        .build()
        .expect_err("enum value from a different enum should fail");
    let diag = diagnostic_with_code(&err, CfdErrorCode::TypeMismatch);
    assert!(
        diag.message
            .contains("expected enum `Rarity`, got `Element`"),
        "diagnostic should name both enum types: {}",
        diag.message
    );
    assert_eq!(
        diag.primary.as_ref().map(|label| label.path.clone()),
        Some(CfdPath::root().field("rarity"))
    );
}

#[test]
fn int_ref_defaults_resolve_to_existing_records() {
    let int_default = compile_schema(
        r#"
            type Item { @id id: int; }
            type Drop {
                @ref(Item)
                item_id: int = 7;
            }
        "#,
    );
    let mut int_builder = CfdDataModel::builder(&int_default);
    int_builder.add_record("Item", [("id", CfdInputValue::from(7_i64))]);
    int_builder.add_input_record(CfdInputRecord::new(
        "Drop",
        std::iter::empty::<(&str, CfdInputValue)>(),
    ));
    let model = int_builder
        .build()
        .expect("int @ref default should resolve");
    let item_id = record_id_at(&model, 0);
    let drop_id = record_id_at(&model, 1);
    assert_eq!(
        model
            .record(drop_id)
            .and_then(|record| record.field("item_id")),
        Some(&CfdValue::Ref {
            id: CfdIdValue::from(7_i64),
            target: item_id,
        })
    );
}

#[test]
fn nullable_key_as_enum_defaults_accept_null_and_reject_csharp_keywords() {
    let null_default = compile_schema(
        r#"
            type GeneConfig {
                @GenAsEnum("GeneId")
                id: string? = null;
            }
            type Gene {
                @IdAsEnum("GeneId")
                @id
                id: string;
            }
        "#,
    );
    let mut null_builder = CfdDataModel::builder(&null_default);
    null_builder.add_record("Gene", [("id", CfdInputValue::from("ValidGene"))]);
    null_builder.add_input_record(CfdInputRecord::new(
        "GeneConfig",
        std::iter::empty::<(&str, CfdInputValue)>(),
    ));
    let model = null_builder
        .build()
        .expect("nullable @GenAsEnum null default should build");
    let config_id = record_id_at(&model, 1);
    assert_eq!(
        model
            .record(config_id)
            .and_then(|record| record.field("id")),
        Some(&CfdValue::Null)
    );

    let keyword_default = compile_schema(
        r#"
            type GeneConfig {
                @IdAsEnum("GeneId")
                @id
                id: string = "class";
            }
        "#,
    );
    let mut keyword_builder = CfdDataModel::builder(&keyword_default);
    keyword_builder.add_input_record(CfdInputRecord::new(
        "GeneConfig",
        std::iter::empty::<(&str, CfdInputValue)>(),
    ));
    let err = keyword_builder
        .build()
        .expect_err("C# keyword default should not be a generated enum variant");
    assert_has_code(&err, CfdErrorCode::InvalidEnumVariant);
}

#[test]
fn key_as_enum_id_accepts_underscore_prefixed_csharp_identifiers() {
    let schema = compile_schema(
        r#"
            type GeneConfig {
                @IdAsEnum("GeneId")
                @id
                id: string;
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("GeneConfig", [("id", CfdInputValue::from("_HiddenGene"))]);

    let model = builder
        .build()
        .expect("underscore-prefixed C# identifiers should be valid generated enum variants");
    let record_id = record_id_at(&model, 0);
    assert_eq!(
        model
            .record(record_id)
            .and_then(|record| record.field("id")),
        Some(&CfdValue::String("_HiddenGene".to_string()))
    );
}
