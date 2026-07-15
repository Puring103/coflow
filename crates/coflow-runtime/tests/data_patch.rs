#![allow(
    clippy::expect_used,
    clippy::needless_raw_string_hashes,
    clippy::panic,
    clippy::too_many_lines,
    clippy::unwrap_used
)]

use coflow_project::Project;
use coflow_runtime::{
    CreateFieldSource, CreateRequiredInput, DataPatchOp, DataPatchRequest, DefaultMaterialization,
    MutationOp, MutationRequest, MutationValue, PatchDimensionValueSelector, PatchPathSegment,
    PatchRecordSelector, RecordCoordinate, Runtime,
};
use serde_json::json;

fn write_project(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        r"
            enum Rarity { Common = 0, Rare = 10 }
            enum Element { Fire = 1, Ice = 2 }

            type Item {
                name: string;
                price: int;
                rarity: Rarity = Rarity.Common;
                check { price > 0; }
            }

            type ItemReward {
                item: &Item;
                count: int;
            }

            type Loot {
                rewards: [ItemReward];
                resistances: {Element: int} = {};
                owner: &Item? = null;
            }
        ",
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

fn write_spread_project(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        r"
            type Item {
                name: string;
                power: int;
            }
        ",
    )
    .expect("write schema");
    std::fs::write(
        root.join("data").join("source.cfd"),
        r#"base: Item {
    name: "Base",
    power: 1,
}
"#,
    )
    .expect("write source");
    std::fs::write(
        root.join("data").join("host.cfd"),
        r"child: Item {
    ...&base,
}
",
    )
    .expect("write host");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema/main.cft\nsources:\n  - path: data/source.cfd\n  - path: data/host.cfd\noutputs:\n  data:\n    type: json\n    dir: generated/data\n",
    )
    .expect("write config");
}

fn write_group_cfd_project(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        r"
            type IntRange {
                lo: int;
                hi: int;
            }

            type DamageEffect {
                damage: IntRange;
                pierce_divine: bool;
            }
        ",
    )
    .expect("write schema");
    std::fs::write(
        root.join("data").join("effects.cfd"),
        r"DamageEffect {
    eff_fireball_damage {
        damage: { lo: 6, hi: 6 },
        pierce_divine: false,
    }

    eff_execute_damage {
        damage: { lo: 999, hi: 999 },
        pierce_divine: false,
    }
}
",
    )
    .expect("write cfd");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\nsources:\n  - path: data\noutputs:\n  data:\n    type: json\n    dir: generated/data\n",
    )
    .expect("write config");
}

fn write_shape_annotation_project(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        r"
            type GameConfig { value: int; }

            type Item { name: string; }

            type Holder {
                owner: &Item;
                inline_item: Item;

                configs: [&GameConfig];
            }
        ",
    )
    .expect("write schema");
    std::fs::write(
        root.join("data").join("records.cfd"),
        r#"sword: Item { name: "Sword" }
main: GameConfig { value: 1 }
"#,
    )
    .expect("write cfd");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\nsources:\n  - path: data\noutputs:\n  data:\n    type: json\n    dir: generated/data\n",
    )
    .expect("write config");
}

fn write_domain_key_project(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        r"
            type Reward { label: string; }
            type ItemReward : Reward { count: int; }
            type CurrencyReward : Reward { amount: int; }
            type Skill { label: string; }
        ",
    )
    .expect("write schema");
    std::fs::write(
        root.join("data").join("records.cfd"),
        r#"base: ItemReward { label: "Item", count: 1 }
"#,
    )
    .expect("write data");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\nsources:\n  - path: data\noutputs:\n  data:\n    type: json\n    dir: generated/data\n",
    )
    .expect("write config");
}

fn write_polymorphic_ref_rename_project(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        r"
            type Item { name: string; }
            abstract type Reward {}
            type ItemReward : Reward {
                item: &Item;
                count: int;
            }
            type Stage {
                first_clear_reward: Reward;
            }
        ",
    )
    .expect("write schema");
    std::fs::write(
        root.join("data").join("items.cfd"),
        r#"sword: Item { name: "Sword" }
shield: Item { name: "Shield" }
"#,
    )
    .expect("write items");
    std::fs::write(
        root.join("data").join("stages.cfd"),
        r"stage_start: Stage {
    first_clear_reward: ItemReward { item: &sword, count: 1 },
}
",
    )
    .expect("write stages");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\nsources:\n  - path: data/items.cfd\n  - path: data/stages.cfd\noutputs:\n  data:\n    type: json\n    dir: generated/data\n",
    )
    .expect("write config");
}

fn registry() -> coflow_api::ProviderRegistry {
    coflow_builtins::default_provider_registry().expect("default provider registry")
}

fn write_dimension_project(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("data/dimensions/language")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        "type Item { @localized name: string; }",
    )
    .expect("write schema");
    std::fs::write(root.join("data/items.csv"), "id,name\npotion,Potion\n").expect("write items");
    std::fs::write(
        root.join("data/dimensions/language/Item_name.csv"),
        "id,default,zh\npotion,Potion,药水\n",
    )
    .expect("write dimension values");
    std::fs::write(
        root.join("coflow.yaml"),
        r#"schema: schema.cft
sources:
  - path: data/items.csv
    type: csv
    sheets:
      - sheet: items
        type: Item
dimensions:
  language:
    variants: [zh]
    out_dir: data/dimensions/language
"#,
    )
    .expect("write config");
}

fn write_dimension_ref_project(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("data/dimensions/platform")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        r#"
            type Item { name: string; }
            type Offer {
                @dimension("platform")
                item: &Item;
            }
        "#,
    )
    .expect("write schema");
    std::fs::write(
        root.join("data/main.cfd"),
        "potion: Item { name: \"Potion\" }\nstarter: Offer { item: &potion }\n",
    )
    .expect("write records");
    std::fs::write(
        root.join("data/dimensions/platform/Offer_item.csv"),
        "id,default,pc\nstarter,&potion,&potion\n",
    )
    .expect("write dimension refs");
    std::fs::write(
        root.join("coflow.yaml"),
        r#"schema: schema.cft
sources:
  - path: data/main.cfd
dimensions:
  platform:
    variants: [pc]
    out_dir: data/dimensions/platform
"#,
    )
    .expect("write config");
}

fn session(root: &std::path::Path) -> coflow_runtime::WriteProjectSession {
    let project = Project::open_schema_only(Some(&root.join("coflow.yaml"))).expect("open");
    let registry = registry();
    Runtime::new(registry)
        .open_write_session(project)
        .expect("session")
}

#[test]
fn patch_inserts_and_edits_cfd_records_then_reports_check_diagnostics() {
    let root = std::env::temp_dir().join(format!("coflow-data-patch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    write_project(&root);
    let mut session = session(&root);

    let report = session.apply_data_patch(DataPatchRequest {
        stop_on_write_error: true,
        ops: vec![
            DataPatchOp::InsertRecord {
                file: "data/items.cfd".to_string(),
                sheet: None,
                actual_type: "Item".to_string(),
                key: "bad_sword".to_string(),
                materialization: DefaultMaterialization::Minimal,
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
    });

    assert!(report.write_ok);
    assert!(!report.check_ok);
    assert_eq!(report.applied.len(), 2);
    assert_eq!(session.revision(), 1, "the batch publishes one generation");
    assert!(report.failed.is_empty());
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.stage == "CHECK"));

    let check_diag = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.stage == "CHECK")
        .expect("check diagnostic");
    assert_eq!(
        check_diag.record_key.as_deref(),
        Some("bad_sword"),
        "flat diagnostic should carry the record key so editor jump works: {check_diag:?}",
    );
    assert!(
        check_diag.field_path.is_some(),
        "flat diagnostic should carry the offending field path: {check_diag:?}",
    );

    let text = std::fs::read_to_string(root.join("data").join("items.cfd")).expect("read cfd");
    assert!(text.contains("bad_sword"));
    assert!(text.contains("Rare"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn dimension_patch_preserves_record_selector_json_shape() {
    let json = json!({
        "stop_on_write_error": true,
        "ops": [{
            "op": "set_dimension_value",
            "coordinate": {
                "record": { "type": "Item", "key": "potion" },
                "field": "name",
                "dimension": "language",
                "variant": "zh",
                "path": []
            },
            "value": "治疗药水"
        }]
    });

    let request: DataPatchRequest =
        serde_json::from_value(json.clone()).expect("deserialize existing patch shape");
    let serialized = serde_json::to_value(request).expect("serialize patch");
    assert_eq!(
        serialized["ops"][0]["coordinate"],
        json["ops"][0]["coordinate"]
    );
}

#[test]
fn patch_writes_and_clears_record_owned_dimension_values() {
    let root = std::env::temp_dir().join(format!(
        "coflow-data-patch-dimension-value-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    write_dimension_project(&root);
    let mut session = session(&root);
    let selector = PatchDimensionValueSelector {
        record: PatchRecordSelector {
            actual_type: "Item".to_string(),
            key: "potion".to_string(),
        },
        field: coflow_cft::FieldName::new("name").unwrap(),
        dimension: coflow_cft::DimensionName::new("language").unwrap(),
        variant: coflow_cft::VariantName::new("zh").unwrap(),
        path: Vec::new(),
    };

    let set = session.apply_data_patch(DataPatchRequest {
        stop_on_write_error: true,
        ops: vec![DataPatchOp::SetDimensionValue {
            coordinate: selector.clone(),
            expected: coflow_runtime::DimensionValueExpectation::Value(MutationValue::Cfd(
                coflow_data_model::CfdValue::String("药水".to_string()),
            )),
            value: json!("治疗药水"),
        }],
    });
    assert!(set.write_ok, "diagnostics: {:?}", set.diagnostics);
    assert_eq!(
        set.affected_files,
        ["data/dimensions/language/Item_name.csv"]
    );
    assert!(
        std::fs::read_to_string(root.join("data/dimensions/language/Item_name.csv"))
            .unwrap()
            .contains("治疗药水")
    );

    let dimension_file = root.join("data/dimensions/language/Item_name.csv");
    let before_stale_write = std::fs::read_to_string(&dimension_file).unwrap();
    let stale = session.apply_data_patch(DataPatchRequest {
        stop_on_write_error: true,
        ops: vec![DataPatchOp::SetDimensionValue {
            coordinate: selector.clone(),
            expected: coflow_runtime::DimensionValueExpectation::Missing,
            value: json!("过期写入"),
        }],
    });
    assert!(!stale.write_ok);
    assert!(stale
        .failed
        .iter()
        .flat_map(|failure| &failure.diagnostics)
        .any(|diagnostic| diagnostic.code == "MUTATION-DIMENSION-STALE"));
    assert_eq!(
        std::fs::read_to_string(&dimension_file).unwrap(),
        before_stale_write,
        "stale dimension writes must not touch the managed source",
    );

    let explicit_null = session.apply_data_patch(DataPatchRequest {
        stop_on_write_error: true,
        ops: vec![DataPatchOp::SetDimensionValue {
            coordinate: selector.clone(),
            expected: coflow_runtime::DimensionValueExpectation::Value(MutationValue::Cfd(
                coflow_data_model::CfdValue::String("治疗药水".to_string()),
            )),
            value: serde_json::Value::Null,
        }],
    });
    assert!(
        explicit_null.write_ok,
        "diagnostics: {:?}",
        explicit_null.diagnostics
    );
    assert!(
        std::fs::read_to_string(root.join("data/dimensions/language/Item_name.csv"))
            .unwrap()
            .contains(",null\n")
    );

    let clear = session.apply_data_patch(DataPatchRequest {
        stop_on_write_error: true,
        ops: vec![DataPatchOp::ClearDimensionValue {
            coordinate: selector.clone(),
            expected: coflow_runtime::DimensionValueExpectation::Value(MutationValue::Cfd(
                coflow_data_model::CfdValue::Null,
            )),
        }],
    });
    assert!(clear.write_ok, "diagnostics: {:?}", clear.diagnostics);
    assert!(
        std::fs::read_to_string(root.join("data/dimensions/language/Item_name.csv"))
            .unwrap()
            .contains("potion,Potion,\n")
    );

    let restore_from_missing = session.apply_data_patch(DataPatchRequest {
        stop_on_write_error: true,
        ops: vec![DataPatchOp::SetDimensionValue {
            coordinate: selector,
            expected: coflow_runtime::DimensionValueExpectation::Missing,
            value: json!("恢复值"),
        }],
    });
    assert!(
        restore_from_missing.write_ok,
        "diagnostics: {:?}",
        restore_from_missing.diagnostics
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn rename_record_rewrites_refs_in_dimension_overlays() {
    let root = std::env::temp_dir().join(format!(
        "coflow-data-patch-dimension-ref-rename-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    write_dimension_ref_project(&root);
    let mut session = session(&root);

    let report = session.apply_data_patch(DataPatchRequest {
        stop_on_write_error: true,
        ops: vec![DataPatchOp::RenameRecord {
            record: PatchRecordSelector {
                actual_type: "Item".to_string(),
                key: "potion".to_string(),
            },
            file: None,
            new_key: "elixir".to_string(),
        }],
    });

    assert!(report.write_ok, "diagnostics: {:?}", report.diagnostics);
    let dimension = std::fs::read_to_string(root.join("data/dimensions/platform/Offer_item.csv"))
        .expect("read dimension source");
    assert!(dimension.contains("starter,&elixir,&elixir"), "{dimension}");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn rename_and_delete_owner_record_rewrite_dimension_rows() {
    let root = std::env::temp_dir().join(format!(
        "coflow-data-patch-dimension-owner-lifecycle-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    write_dimension_project(&root);
    let mut session = session(&root);
    let dimension_file = root.join("data/dimensions/language/Item_name.csv");

    let renamed = session.apply_data_patch(DataPatchRequest {
        stop_on_write_error: true,
        ops: vec![DataPatchOp::RenameRecord {
            record: PatchRecordSelector {
                actual_type: "Item".to_string(),
                key: "potion".to_string(),
            },
            file: None,
            new_key: "elixir".to_string(),
        }],
    });
    assert!(renamed.write_ok, "diagnostics: {:?}", renamed.diagnostics);
    assert!(renamed
        .affected_files
        .iter()
        .any(|path| path == "data/dimensions/language/Item_name.csv"));
    let after_rename = std::fs::read_to_string(&dimension_file).expect("read renamed dimension");
    assert!(
        after_rename.contains("elixir,Potion,药水"),
        "{after_rename}"
    );
    assert!(!after_rename.contains("potion,"), "{after_rename}");

    let deleted = session.apply_data_patch(DataPatchRequest {
        stop_on_write_error: true,
        ops: vec![DataPatchOp::DeleteRecord {
            record: PatchRecordSelector {
                actual_type: "Item".to_string(),
                key: "elixir".to_string(),
            },
            file: None,
        }],
    });
    assert!(deleted.write_ok, "diagnostics: {:?}", deleted.diagnostics);
    let after_delete = std::fs::read_to_string(&dimension_file).expect("read deleted dimension");
    assert!(!after_delete.contains("elixir,"), "{after_delete}");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn rename_record_updates_refs_inside_polymorphic_cfd_values() {
    let root = std::env::temp_dir().join(format!(
        "coflow-data-patch-polymorphic-rename-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    write_polymorphic_ref_rename_project(&root);
    let mut session = session(&root);

    let report = session.apply_data_patch(DataPatchRequest {
        stop_on_write_error: true,
        ops: vec![DataPatchOp::RenameRecord {
            record: PatchRecordSelector {
                actual_type: "Item".to_string(),
                key: "sword".to_string(),
            },
            file: Some("data/items.cfd".to_string()),
            new_key: "blade".to_string(),
        }],
    });

    assert!(report.write_ok, "rename failed: {:?}", report.failed);
    assert!(
        report.check_ok,
        "post-check diagnostics: {:?}",
        report.diagnostics
    );
    assert_eq!(report.applied.len(), 1);

    let items = std::fs::read_to_string(root.join("data").join("items.cfd")).expect("read items");
    let stages =
        std::fs::read_to_string(root.join("data").join("stages.cfd")).expect("read stages");
    assert!(
        items.contains("blade: Item"),
        "item key not renamed: {items}"
    );
    assert!(
        stages.contains("ItemReward { item: &blade, count: 1 }"),
        "polymorphic ref not renamed: {stages}"
    );
    assert!(
        session.queries().record_view("Item", "blade").is_some(),
        "rebuilt session should expose renamed item"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn patch_writes_group_record_without_required_commas() {
    let root = std::env::temp_dir().join(format!(
        "coflow-data-patch-group-cfd-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    write_group_cfd_project(&root);
    let mut session = session(&root);

    let report = session.apply_data_patch(DataPatchRequest {
        stop_on_write_error: true,
        ops: vec![DataPatchOp::SetField {
            record: PatchRecordSelector {
                actual_type: "DamageEffect".to_string(),
                key: "eff_fireball_damage".to_string(),
            },
            file: None,
            path: vec![
                PatchPathSegment::Field("damage".to_string()),
                PatchPathSegment::Field("lo".to_string()),
            ],
            value: json!(7),
        }],
    });

    assert!(report.write_ok, "patch failed: {report:?}");
    assert!(report.failed.is_empty());
    let view = session
        .queries()
        .record_view("DamageEffect", "eff_fireball_damage")
        .expect("record view");
    let Some(coflow_data_model::CfdValue::Object(damage)) = view.record.fields().get("damage")
    else {
        panic!("damage should be object");
    };
    assert_eq!(
        damage.field("lo"),
        Some(&coflow_data_model::CfdValue::Int(7))
    );

    let text = std::fs::read_to_string(root.join("data").join("effects.cfd")).expect("read cfd");
    assert!(
        text.contains("damage: { lo: 7, hi: 6 }"),
        "written source should contain updated value: {text}"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn patch_insert_minimal_does_not_materialize_explicit_schema_defaults() {
    let root = std::env::temp_dir().join(format!(
        "coflow-data-patch-minimal-default-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    write_project(&root);
    let mut session = session(&root);

    let report = session.apply_data_patch(DataPatchRequest {
        stop_on_write_error: true,
        ops: vec![DataPatchOp::InsertRecord {
            file: "data/items.cfd".to_string(),
            sheet: None,
            actual_type: "Item".to_string(),
            key: "defaulted".to_string(),
            materialization: DefaultMaterialization::Minimal,
            fields: serde_json::from_value(json!({
                "name": "Defaulted",
                "price": 1
            }))
            .expect("fields map"),
        }],
    });

    assert!(report.write_ok);
    assert!(report.check_ok);
    let text = std::fs::read_to_string(root.join("data").join("items.cfd")).expect("read cfd");
    let inserted = text
        .split("defaulted")
        .nth(1)
        .expect("inserted record text");
    assert!(!inserted.contains("rarity"));

    let view = session
        .queries()
        .record_view("Item", "defaulted")
        .expect("record view");
    let Some(coflow_data_model::CfdValue::Enum(value)) = view.record.fields().get("rarity") else {
        panic!("rarity should be defaulted enum");
    };
    assert_eq!(value.variant.as_deref(), Some("Common"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn patch_insert_minimal_requires_explicit_values_for_required_ref_fields() {
    let root = std::env::temp_dir().join(format!(
        "coflow-data-patch-minimal-ref-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        r"
            type Item { name: string; }
            type Loot {
                owner: &Item;
            }
        ",
    )
    .expect("write schema");
    std::fs::write(
        root.join("data").join("items.cfd"),
        r#"sword: Item { name: "Sword" }"#,
    )
    .expect("write cfd");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\nsources:\n  - path: data\noutputs:\n  data:\n    type: json\n    dir: generated/data\n",
    )
    .expect("write config");
    let mut session = session(&root);

    let report = session.apply_data_patch(DataPatchRequest {
        stop_on_write_error: true,
        ops: vec![DataPatchOp::InsertRecord {
            file: "data/items.cfd".to_string(),
            sheet: None,
            actual_type: "Loot".to_string(),
            key: "starter_loot".to_string(),
            materialization: DefaultMaterialization::Minimal,
            fields: serde_json::from_value(json!({})).expect("fields map"),
        }],
    });

    assert!(!report.write_ok);
    assert!(!report.check_ok);
    assert!(report.applied.is_empty());
    assert_eq!(report.failed.len(), 1);
    assert!(report.failed[0]
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "MUTATION-DEFAULT"));
    let text = std::fs::read_to_string(root.join("data").join("items.cfd")).expect("read cfd");
    assert!(!text.contains("starter_loot"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn create_record_draft_marks_required_refs_and_keeps_schema_defaults_separate() {
    let root = std::env::temp_dir().join(format!(
        "coflow-create-draft-required-ref-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        r"
            enum Rarity { Common = 0, Rare = 1 }
            type Item { name: string; }
            type Loot {
                owner: &Item;
                backup: &Item? = null;
                rarity: Rarity = Rarity.Common;
                count: int;
            }
        ",
    )
    .expect("write schema");
    std::fs::write(
        root.join("data").join("items.cfd"),
        r#"sword: Item { name: "Sword" }"#,
    )
    .expect("write cfd");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\nsources:\n  - path: data\noutputs:\n  data:\n    type: json\n    dir: generated/data\n",
    )
    .expect("write config");
    let session = session(&root);

    let draft = session.create_record_draft("Loot").expect("create draft");
    let owner = draft
        .fields
        .iter()
        .find(|field| field.name == "owner")
        .expect("owner field");
    assert_eq!(owner.source, CreateFieldSource::RequiredInput);
    assert_eq!(owner.value, Some(coflow_data_model::CfdValue::Null));
    assert!(matches!(
        owner.required.as_ref(),
        Some(CreateRequiredInput::Ref { target_type }) if target_type == "Item"
    ));

    let backup = draft
        .fields
        .iter()
        .find(|field| field.name == "backup")
        .expect("backup field");
    assert_eq!(backup.source, CreateFieldSource::SchemaDefault);
    assert_eq!(backup.value, Some(coflow_data_model::CfdValue::Null));

    let count = draft
        .fields
        .iter()
        .find(|field| field.name == "count")
        .expect("count field");
    assert_eq!(count.source, CreateFieldSource::TypeSeed);
    assert_eq!(count.value, Some(coflow_data_model::CfdValue::Int(0)));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn create_record_draft_field_errors_do_not_pollute_following_fields() {
    let root = std::env::temp_dir().join(format!(
        "coflow-create-draft-independent-fields-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        r"
            type Item { name: string; }
            type Child { item: &Item; }
            type Parent {
                first: Child;
                second: Child;
            }
        ",
    )
    .expect("write schema");
    std::fs::write(
        root.join("data").join("items.cfd"),
        r#"sword: Item { name: "Sword" }"#,
    )
    .expect("write cfd");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\nsources:\n  - path: data\noutputs:\n  data:\n    type: json\n    dir: generated/data\n",
    )
    .expect("write config");
    let session = session(&root);

    let draft = session.create_record_draft("Parent").expect("create draft");
    let first = draft
        .fields
        .iter()
        .find(|field| field.name == "first")
        .expect("first field");
    let second = draft
        .fields
        .iter()
        .find(|field| field.name == "second")
        .expect("second field");
    for field in [first, second] {
        assert_eq!(field.source, CreateFieldSource::RequiredInput);
        assert!(matches!(
            field.required.as_ref(),
            Some(CreateRequiredInput::Unsupported { message })
                if message.contains("field `item` of type `&Item`")
        ));
    }

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn patch_insert_minimal_seeds_nullable_refs_and_required_enums_without_defaults() {
    let root = std::env::temp_dir().join(format!(
        "coflow-data-patch-minimal-nullable-enum-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        r"
            enum Rarity { Common = 0, Rare = 1 }
            type Item { name: string; }
            type Holder {
                backup: &Item?;
                rarity: Rarity;
            }
        ",
    )
    .expect("write schema");
    std::fs::write(
        root.join("data").join("items.cfd"),
        r#"sword: Item { name: "Sword" }"#,
    )
    .expect("write cfd");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\nsources:\n  - path: data\noutputs:\n  data:\n    type: json\n    dir: generated/data\n",
    )
    .expect("write config");
    let mut session = session(&root);

    let report = session.apply_data_patch(DataPatchRequest {
        stop_on_write_error: true,
        ops: vec![DataPatchOp::InsertRecord {
            file: "data/items.cfd".to_string(),
            sheet: None,
            actual_type: "Holder".to_string(),
            key: "holder".to_string(),
            materialization: DefaultMaterialization::Minimal,
            fields: serde_json::from_value(json!({})).expect("fields map"),
        }],
    });

    assert!(report.write_ok);
    let view = session
        .queries()
        .record_view("Holder", "holder")
        .expect("holder");
    assert_eq!(
        view.record.fields().get("backup"),
        Some(&coflow_data_model::CfdValue::Null)
    );
    let Some(coflow_data_model::CfdValue::Enum(value)) = view.record.fields().get("rarity") else {
        panic!("rarity should be enum seeded");
    };
    assert_eq!(value.variant.as_deref(), Some("Common"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn patch_insert_minimal_accepts_explicit_required_ref_fields() {
    let root = std::env::temp_dir().join(format!(
        "coflow-data-patch-minimal-explicit-ref-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        r"
            type Item { name: string; }
            type Loot {
                owner: &Item;
            }
        ",
    )
    .expect("write schema");
    std::fs::write(
        root.join("data").join("items.cfd"),
        r#"sword: Item { name: "Sword" }"#,
    )
    .expect("write cfd");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\nsources:\n  - path: data\noutputs:\n  data:\n    type: json\n    dir: generated/data\n",
    )
    .expect("write config");
    let mut session = session(&root);

    let report = session.apply_data_patch(DataPatchRequest {
        stop_on_write_error: true,
        ops: vec![DataPatchOp::InsertRecord {
            file: "data/items.cfd".to_string(),
            sheet: None,
            actual_type: "Loot".to_string(),
            key: "starter_loot".to_string(),
            materialization: DefaultMaterialization::Minimal,
            fields: serde_json::from_value(json!({
                "owner": "sword"
            }))
            .expect("fields map"),
        }],
    });

    assert!(report.write_ok);
    assert!(report.check_ok);
    let text = std::fs::read_to_string(root.join("data").join("items.cfd")).expect("read cfd");
    assert!(text.contains("starter_loot"));
    assert!(text.contains("&sword"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn patch_rejects_explicit_values_that_violate_ref_shapes() {
    let root = std::env::temp_dir().join(format!(
        "coflow-data-patch-ref-shapes-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    write_shape_annotation_project(&root);
    let mut session = session(&root);

    let report = session.apply_data_patch(DataPatchRequest {
        stop_on_write_error: false,
        ops: vec![
            DataPatchOp::InsertRecord {
                file: "data/records.cfd".to_string(),
                sheet: None,
                actual_type: "Holder".to_string(),
                key: "bad_ref".to_string(),
                materialization: DefaultMaterialization::Minimal,
                fields: serde_json::from_value(json!({
                    "owner": { "name": "Inline Owner" },
                    "inline_item": { "name": "Inline" },
                    "configs": ["main"]
                }))
                .expect("fields map"),
            },
            DataPatchOp::InsertRecord {
                file: "data/records.cfd".to_string(),
                sheet: None,
                actual_type: "Holder".to_string(),
                key: "bad_config_ref".to_string(),
                materialization: DefaultMaterialization::Minimal,
                fields: serde_json::from_value(json!({
                    "owner": "sword",
                    "inline_item": { "name": "Inline" },
                    "configs": [{ "value": 2 }]
                }))
                .expect("fields map"),
            },
        ],
    });

    assert!(!report.write_ok);
    assert_eq!(report.applied.len(), 0);
    assert_eq!(report.failed.len(), 2);
    assert!(report.failed[0]
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "MUTATION-VALUE"));
    assert!(report.failed[1]
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "MUTATION-VALUE"));

    let text = std::fs::read_to_string(root.join("data").join("records.cfd")).expect("read cfd");
    assert!(!text.contains("bad_ref"));
    assert!(!text.contains("bad_config_ref"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn patch_set_field_rejects_values_that_violate_ref_shapes() {
    let root = std::env::temp_dir().join(format!(
        "coflow-data-patch-set-ref-shapes-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    write_shape_annotation_project(&root);
    let mut session = session(&root);

    let report = session.apply_data_patch(DataPatchRequest {
        stop_on_write_error: true,
        ops: vec![DataPatchOp::InsertRecord {
            file: "data/records.cfd".to_string(),
            sheet: None,
            actual_type: "Holder".to_string(),
            key: "holder".to_string(),
            materialization: DefaultMaterialization::Minimal,
            fields: serde_json::from_value(json!({
                "owner": "sword",
                "inline_item": { "name": "Inline" },
                "configs": ["main"]
            }))
            .expect("fields map"),
        }],
    });
    assert!(report.write_ok);

    let report = session.apply_data_patch(DataPatchRequest {
        stop_on_write_error: false,
        ops: vec![
            DataPatchOp::SetField {
                record: PatchRecordSelector {
                    actual_type: "Holder".to_string(),
                    key: "holder".to_string(),
                },
                file: None,
                path: vec![PatchPathSegment::Field("owner".to_string())],
                value: json!({ "name": "Inline Owner" }),
            },
            DataPatchOp::SetField {
                record: PatchRecordSelector {
                    actual_type: "Holder".to_string(),
                    key: "holder".to_string(),
                },
                file: None,
                path: vec![
                    PatchPathSegment::Field("configs".to_string()),
                    PatchPathSegment::Index(0),
                ],
                value: json!({ "value": 2 }),
            },
        ],
    });

    assert!(!report.write_ok);
    assert_eq!(report.applied.len(), 0);
    assert_eq!(report.failed.len(), 2);
    assert!(report.failed[0]
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "MUTATION-VALUE"));
    assert!(report.failed[1]
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "MUTATION-VALUE"));

    let view = session
        .queries()
        .record_view("Holder", "holder")
        .expect("holder");
    assert!(matches!(
        view.record.fields().get("owner"),
        Some(coflow_data_model::CfdValue::Ref(target_key)) if target_key == "sword"
    ));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn direct_write_field_rejects_values_that_violate_ref_shapes_before_file_write() {
    let root = std::env::temp_dir().join(format!(
        "coflow-direct-write-ref-shapes-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    write_shape_annotation_project(&root);
    let mut session = session(&root);

    let report = session.apply_data_patch(DataPatchRequest {
        stop_on_write_error: true,
        ops: vec![DataPatchOp::InsertRecord {
            file: "data/records.cfd".to_string(),
            sheet: None,
            actual_type: "Holder".to_string(),
            key: "holder".to_string(),
            materialization: DefaultMaterialization::Minimal,
            fields: serde_json::from_value(json!({
                "owner": "sword",
                "inline_item": { "name": "Inline" },
                "configs": ["main"]
            }))
            .expect("fields map"),
        }],
    });
    assert!(report.write_ok);

    let before =
        std::fs::read_to_string(root.join("data").join("records.cfd")).expect("read before");
    let err = session
        .write_field(
            "Holder",
            "holder",
            &[coflow_api::WriteFieldPathSegment::Field(
                "inline_item".to_string(),
            )],
            &coflow_data_model::CfdValue::Ref("sword".to_string()),
        )
        .expect_err("direct write should fail before writer mutation");
    assert!(err
        .iter()
        .any(|diagnostic| diagnostic.code == "MUTATION-VALUE"));
    let after = std::fs::read_to_string(root.join("data").join("records.cfd")).expect("read after");
    assert_eq!(before, after);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn direct_write_field_rejects_missing_ref_target_before_file_write() {
    let root = std::env::temp_dir().join(format!(
        "coflow-direct-write-missing-ref-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    write_shape_annotation_project(&root);
    let mut session = session(&root);

    let report = session.apply_data_patch(DataPatchRequest {
        stop_on_write_error: true,
        ops: vec![DataPatchOp::InsertRecord {
            file: "data/records.cfd".to_string(),
            sheet: None,
            actual_type: "Holder".to_string(),
            key: "holder".to_string(),
            materialization: DefaultMaterialization::Minimal,
            fields: serde_json::from_value(json!({
                "owner": "sword",
                "inline_item": { "name": "Inline" },
                "configs": ["main"]
            }))
            .expect("fields map"),
        }],
    });
    assert!(report.write_ok);

    let before =
        std::fs::read_to_string(root.join("data").join("records.cfd")).expect("read before");
    let err = session
        .write_field(
            "Holder",
            "holder",
            &[coflow_api::WriteFieldPathSegment::Field(
                "owner".to_string(),
            )],
            &coflow_data_model::CfdValue::Ref("ghost".to_string()),
        )
        .expect_err("direct write should reject missing ref target before writer mutation");
    assert!(err.iter().any(|diagnostic| {
        diagnostic.code == "MUTATION-SHAPE" && diagnostic.message.contains("was not found")
    }));
    let after = std::fs::read_to_string(root.join("data").join("records.cfd")).expect("read after");
    assert_eq!(before, after);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn direct_write_field_rejects_ref_target_outside_expected_type_before_file_write() {
    let root = std::env::temp_dir().join(format!(
        "coflow-direct-write-ref-type-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        r"
            type Reward { label: string; }
            type ItemReward : Reward { count: int; }
            type CurrencyReward : Reward { amount: int; }
            type Holder { item_reward: &ItemReward; }
        ",
    )
    .expect("write schema");
    std::fs::write(
        root.join("data").join("records.cfd"),
        r#"item: ItemReward { label: "Item", count: 1 }
currency: CurrencyReward { label: "Currency", amount: 10 }
holder: Holder { item_reward: &item }
"#,
    )
    .expect("write cfd");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\nsources:\n  - path: data\noutputs:\n  data:\n    type: json\n    dir: generated/data\n",
    )
    .expect("write config");
    let mut session = session(&root);

    let before =
        std::fs::read_to_string(root.join("data").join("records.cfd")).expect("read before");
    let err = session
        .write_field(
            "Holder",
            "holder",
            &[coflow_api::WriteFieldPathSegment::Field(
                "item_reward".to_string(),
            )],
            &coflow_data_model::CfdValue::Ref("currency".to_string()),
        )
        .expect_err("direct write should reject sibling-type ref target before writer mutation");
    assert!(err.iter().any(|diagnostic| {
        diagnostic.code == "MUTATION-SHAPE" && diagnostic.message.contains("not assignable")
    }));
    let after = std::fs::read_to_string(root.join("data").join("records.cfd")).expect("read after");
    assert_eq!(before, after);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn direct_write_field_rejects_primitive_mismatch_before_file_write() {
    let root = std::env::temp_dir().join(format!(
        "coflow-direct-write-primitive-shape-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    write_project(&root);
    let mut session = session(&root);

    let before = std::fs::read_to_string(root.join("data").join("items.cfd")).expect("read before");
    let err = session
        .write_field(
            "Item",
            "sword",
            &[coflow_api::WriteFieldPathSegment::Field(
                "price".to_string(),
            )],
            &coflow_data_model::CfdValue::String("bad".to_string()),
        )
        .expect_err("direct write should reject primitive mismatch before writer mutation");
    assert!(err.iter().any(|diagnostic| {
        diagnostic.code == "MUTATION-VALUE"
            && diagnostic
                .message
                .contains("does not match expected schema type")
    }));
    let after = std::fs::read_to_string(root.join("data").join("items.cfd")).expect("read after");
    assert_eq!(before, after);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn insert_rejects_duplicate_key_in_same_inheritance_domain_before_file_write() {
    let root = std::env::temp_dir().join(format!(
        "coflow-domain-insert-reject-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    write_domain_key_project(&root);
    let mut session = session(&root);
    let before =
        std::fs::read_to_string(root.join("data").join("records.cfd")).expect("read before");

    let report = session.apply_data_patch(DataPatchRequest {
        stop_on_write_error: true,
        ops: vec![DataPatchOp::InsertRecord {
            file: "data/records.cfd".to_string(),
            sheet: None,
            actual_type: "CurrencyReward".to_string(),
            key: "base".to_string(),
            materialization: DefaultMaterialization::Minimal,
            fields: serde_json::from_value(json!({
                "label": "Currency",
                "amount": 10
            }))
            .expect("fields map"),
        }],
    });
    assert!(!report.write_ok);
    assert!(report.failed[0]
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "MUTATION-INSERT-CONFLICT"));
    let after = std::fs::read_to_string(root.join("data").join("records.cfd")).expect("read after");
    assert_eq!(before, after);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn direct_insert_rejects_duplicate_key_in_same_inheritance_domain_before_file_write() {
    let root = std::env::temp_dir().join(format!(
        "coflow-domain-direct-insert-reject-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    write_domain_key_project(&root);
    let mut session = session(&root);
    let before =
        std::fs::read_to_string(root.join("data").join("records.cfd")).expect("read before");
    let fields = std::collections::BTreeMap::from([
        (
            "label".to_string(),
            coflow_data_model::CfdValue::String("Currency".to_string()),
        ),
        ("amount".to_string(), coflow_data_model::CfdValue::Int(10)),
    ]);

    let err = session
        .insert_record("data/records.cfd", None, "base", "CurrencyReward", &fields)
        .expect_err("direct insert should reject domain duplicate");
    assert!(err
        .iter()
        .any(|diagnostic| diagnostic.code == "MUTATION-INSERT-CONFLICT"));
    let after = std::fs::read_to_string(root.join("data").join("records.cfd")).expect("read after");
    assert_eq!(before, after);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn direct_insert_rejects_missing_ref_target_before_file_write() {
    let root = std::env::temp_dir().join(format!(
        "coflow-direct-insert-missing-ref-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    write_shape_annotation_project(&root);
    let mut session = session(&root);
    let before =
        std::fs::read_to_string(root.join("data").join("records.cfd")).expect("read before");
    let fields = std::collections::BTreeMap::from([
        (
            "owner".to_string(),
            coflow_data_model::CfdValue::Ref("ghost".to_string()),
        ),
        (
            "inline_item".to_string(),
            coflow_data_model::CfdValue::Object(Box::new(coflow_data_model::CfdObject::new(
                "Item",
                std::collections::BTreeMap::from([(
                    "name".to_string(),
                    coflow_data_model::CfdValue::String("Inline".to_string()),
                )]),
            ))),
        ),
        (
            "configs".to_string(),
            coflow_data_model::CfdValue::Array(vec![coflow_data_model::CfdValue::Ref(
                "main".to_string(),
            )]),
        ),
    ]);

    let err = session
        .insert_record("data/records.cfd", None, "holder", "Holder", &fields)
        .expect_err("direct insert should reject missing ref target");
    assert!(err.iter().any(|diagnostic| {
        diagnostic.code == "MUTATION-SHAPE" && diagnostic.message.contains("was not found")
    }));
    let after = std::fs::read_to_string(root.join("data").join("records.cfd")).expect("read after");
    assert_eq!(before, after);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn json_patch_insert_accepts_ref_object_form() {
    let root = std::env::temp_dir().join(format!(
        "coflow-data-patch-json-ref-object-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        "type Item { name: string; } type Holder { item: &Item; }\n",
    )
    .expect("write schema");
    std::fs::write(
        root.join("data").join("records.cfd"),
        "Item { sword { name: Sword } }\n",
    )
    .expect("write data");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\nsources:\n  - path: data/records.cfd\n",
    )
    .expect("write config");
    let mut session = session(&root);
    let report = session.apply_data_patch(DataPatchRequest {
        stop_on_write_error: true,
        ops: vec![DataPatchOp::InsertRecord {
            file: "data/records.cfd".to_string(),
            sheet: None,
            actual_type: "Holder".to_string(),
            key: "main".to_string(),
            materialization: DefaultMaterialization::Minimal,
            fields: serde_json::from_value(json!({
                "item": { "$ref": "sword" }
            }))
            .expect("fields map"),
        }],
    });
    assert!(report.write_ok, "{report:?}");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn data_patch_report_includes_remaining_ops_after_failure() {
    let root = std::env::temp_dir().join(format!(
        "coflow-data-patch-remaining-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    write_shape_annotation_project(&root);
    let mut session = session(&root);
    let report = session.apply_data_patch(DataPatchRequest {
        stop_on_write_error: true,
        ops: vec![
            DataPatchOp::InsertRecord {
                file: "data/records.cfd".to_string(),
                sheet: None,
                actual_type: "Holder".to_string(),
                key: "bad".to_string(),
                materialization: DefaultMaterialization::Minimal,
                fields: serde_json::from_value(json!({ "owner": "ghost" })).expect("fields map"),
            },
            DataPatchOp::InsertRecord {
                file: "data/records.cfd".to_string(),
                sheet: None,
                actual_type: "Item".to_string(),
                key: "later".to_string(),
                materialization: DefaultMaterialization::Minimal,
                fields: serde_json::from_value(json!({ "name": "Later" })).expect("fields map"),
            },
        ],
    });

    assert!(!report.write_ok);
    assert_eq!(report.remaining_ops.len(), 2);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn direct_insert_allows_self_references() {
    let root = std::env::temp_dir().join(format!(
        "coflow-direct-insert-self-ref-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        r"
            type Node {
                parent: &Node? = null;
            }
        ",
    )
    .expect("write schema");
    std::fs::write(root.join("data").join("nodes.cfd"), "").expect("write cfd");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\nsources:\n  - path: data\noutputs:\n  data:\n    type: json\n    dir: generated/data\n",
    )
    .expect("write config");
    let mut session = session(&root);
    let fields = std::collections::BTreeMap::from([(
        "parent".to_string(),
        coflow_data_model::CfdValue::Ref("root".to_string()),
    )]);

    session
        .insert_record("data/nodes.cfd", None, "root", "Node", &fields)
        .expect("self reference should be valid for inserted record");

    let view = session
        .queries()
        .record_view("Node", "root")
        .expect("inserted node");
    assert_eq!(
        view.record.fields().get("parent"),
        Some(&coflow_data_model::CfdValue::Ref("root".to_string()))
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn batch_insert_can_reference_an_earlier_pending_insert() {
    let root = std::env::temp_dir().join(format!(
        "coflow-batch-insert-pending-ref-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        "type Node { parent: &Node? = null; }\n",
    )
    .expect("write schema");
    std::fs::write(root.join("data/nodes.cfd"), "").expect("write data");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\nsources:\n  - path: data/nodes.cfd\n",
    )
    .expect("write config");
    let mut session = session(&root);

    let report = session.apply_data_patch(DataPatchRequest {
        stop_on_write_error: true,
        ops: vec![
            DataPatchOp::InsertRecord {
                file: "data/nodes.cfd".to_string(),
                sheet: None,
                actual_type: "Node".to_string(),
                key: "root".to_string(),
                fields: std::collections::BTreeMap::default(),
                materialization: DefaultMaterialization::Minimal,
            },
            DataPatchOp::InsertRecord {
                file: "data/nodes.cfd".to_string(),
                sheet: None,
                actual_type: "Node".to_string(),
                key: "child".to_string(),
                fields: serde_json::from_value(json!({ "parent": "root" })).expect("fields map"),
                materialization: DefaultMaterialization::Minimal,
            },
        ],
    });

    assert!(report.write_ok, "failures: {:?}", report.failed);
    assert_eq!(report.applied.len(), 2);
    assert_eq!(session.revision(), 1);
    let child = session
        .queries()
        .record_view("Node", "child")
        .expect("inserted child");
    assert_eq!(
        child.record.fields().get("parent"),
        Some(&coflow_data_model::CfdValue::Ref("root".to_string()))
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn batch_rename_of_pending_insert_rewrites_self_references() {
    let root = std::env::temp_dir().join(format!(
        "coflow-batch-rename-pending-self-ref-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        "type Node { parent: &Node? = null; }\n",
    )
    .expect("write schema");
    std::fs::write(root.join("data/nodes.cfd"), "").expect("write data");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\nsources:\n  - path: data/nodes.cfd\n",
    )
    .expect("write config");
    let mut session = session(&root);

    let report = session.apply_data_patch(DataPatchRequest {
        stop_on_write_error: true,
        ops: vec![
            DataPatchOp::InsertRecord {
                file: "data/nodes.cfd".to_string(),
                sheet: None,
                actual_type: "Node".to_string(),
                key: "root".to_string(),
                fields: serde_json::from_value(json!({ "parent": "root" })).expect("fields map"),
                materialization: DefaultMaterialization::Minimal,
            },
            DataPatchOp::InsertRecord {
                file: "data/nodes.cfd".to_string(),
                sheet: None,
                actual_type: "Node".to_string(),
                key: "child".to_string(),
                fields: serde_json::from_value(json!({ "parent": "root" })).expect("fields map"),
                materialization: DefaultMaterialization::Minimal,
            },
            DataPatchOp::RenameRecord {
                record: PatchRecordSelector {
                    actual_type: "Node".to_string(),
                    key: "root".to_string(),
                },
                file: None,
                new_key: "tree".to_string(),
            },
        ],
    });

    assert!(report.write_ok, "failures: {:?}", report.failed);
    assert_eq!(report.applied.len(), 3);
    assert_eq!(session.revision(), 1);
    let tree = session
        .queries()
        .record_view("Node", "tree")
        .expect("renamed node");
    assert_eq!(
        tree.record.fields().get("parent"),
        Some(&coflow_data_model::CfdValue::Ref("tree".to_string()))
    );
    let child = session
        .queries()
        .record_view("Node", "child")
        .expect("dependent node");
    assert_eq!(
        child.record.fields().get("parent"),
        Some(&coflow_data_model::CfdValue::Ref("tree".to_string()))
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn insert_allows_same_key_for_unrelated_type() {
    let root =
        std::env::temp_dir().join(format!("coflow-domain-insert-allow-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    write_domain_key_project(&root);
    let mut session = session(&root);

    let report = session.apply_data_patch(DataPatchRequest {
        stop_on_write_error: true,
        ops: vec![DataPatchOp::InsertRecord {
            file: "data/records.cfd".to_string(),
            sheet: None,
            actual_type: "Skill".to_string(),
            key: "base".to_string(),
            materialization: DefaultMaterialization::Minimal,
            fields: serde_json::from_value(json!({
                "label": "Skill"
            }))
            .expect("fields map"),
        }],
    });
    assert!(report.write_ok);
    assert!(session
        .queries()
        .record_view("ItemReward", "base")
        .is_some());
    assert!(session.queries().record_view("Skill", "base").is_some());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn editable_shape_reports_self_referential_dependency_cycle() {
    let root = std::env::temp_dir().join(format!(
        "coflow-data-patch-editable-recursive-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        "type Node { label: string; child: Node; }\n",
    )
    .expect("write schema");
    std::fs::write(root.join("data").join("nodes.cfd"), "").expect("write cfd");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\nsources:\n  - path: data\noutputs:\n  data:\n    type: json\n    dir: generated/data\n",
    )
    .expect("write config");
    let session = session(&root);

    let err = session
        .default_record_value("Node", DefaultMaterialization::EditableShape)
        .expect_err("recursive editable shape must be rejected");
    assert!(err.iter().any(|diagnostic| {
        diagnostic.code == "MUTATION-DEFAULT"
            && diagnostic.message == "default materialization dependency cycle: Node.child -> Node"
    }));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn editable_shape_reports_indirect_dependency_cycle_stably() {
    let root = std::env::temp_dir().join(format!(
        "coflow-data-patch-editable-indirect-recursive-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        "type A { b: B; }\ntype B { c: C; }\ntype C { a: A; }\n",
    )
    .expect("write schema");
    std::fs::write(root.join("data").join("records.cfd"), "").expect("write cfd");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\nsources:\n  - path: data\noutputs:\n  data:\n    type: json\n    dir: generated/data\n",
    )
    .expect("write config");
    let session = session(&root);

    let err = session
        .default_record_value("B", DefaultMaterialization::EditableShape)
        .expect_err("indirect cycle must be rejected");
    assert!(err.iter().any(|diagnostic| {
        diagnostic.code == "MUTATION-DEFAULT"
            && diagnostic.message
                == "default materialization dependency cycle: A.b -> B.c -> C.a -> A"
    }));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn patch_insert_minimal_rejects_recursive_required_inline_defaults() {
    let root = std::env::temp_dir().join(format!(
        "coflow-data-patch-minimal-recursive-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        r"
            type Node {
                label: string;
                child: Node;
            }
        ",
    )
    .expect("write schema");
    std::fs::write(root.join("data").join("nodes.cfd"), "").expect("write cfd");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\nsources:\n  - path: data\noutputs:\n  data:\n    type: json\n    dir: generated/data\n",
    )
    .expect("write config");
    let mut session = session(&root);

    let report = session.apply_data_patch(DataPatchRequest {
        stop_on_write_error: true,
        ops: vec![DataPatchOp::InsertRecord {
            file: "data/nodes.cfd".to_string(),
            sheet: None,
            actual_type: "Node".to_string(),
            key: "root".to_string(),
            materialization: DefaultMaterialization::Minimal,
            fields: serde_json::from_value(json!({ "label": "Root" })).expect("fields map"),
        }],
    });

    assert!(!report.write_ok);
    assert!(report.failed[0]
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "MUTATION-DEFAULT"));
    let text = std::fs::read_to_string(root.join("data").join("nodes.cfd")).expect("read cfd");
    assert!(!text.contains("root"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn default_materialization_rejects_abstract_objects() {
    let root = std::env::temp_dir().join(format!(
        "coflow-data-patch-unsafe-defaults-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        r"
            abstract type Reward {}
            type ItemReward : Reward { count: int; }

            type Holder {
                reward: Reward;
            }
        ",
    )
    .expect("write schema");
    std::fs::write(root.join("data").join("records.cfd"), "").expect("write cfd");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\nsources:\n  - path: data\noutputs:\n  data:\n    type: json\n    dir: generated/data\n",
    )
    .expect("write config");
    let mut session = session(&root);

    let abstract_default = session
        .default_record_value("Reward", DefaultMaterialization::EditableShape)
        .expect_err("abstract type should not be materialized");
    assert!(abstract_default
        .iter()
        .any(|diagnostic| diagnostic.code == "MUTATION-DEFAULT"));

    let report = session.apply_data_patch(DataPatchRequest {
        stop_on_write_error: true,
        ops: vec![DataPatchOp::InsertRecord {
            file: "data/records.cfd".to_string(),
            sheet: None,
            actual_type: "Holder".to_string(),
            key: "bad_holder".to_string(),
            materialization: DefaultMaterialization::Minimal,
            fields: serde_json::from_value(json!({})).expect("fields map"),
        }],
    });
    assert!(!report.write_ok);
    assert!(report.failed[0]
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "MUTATION-DEFAULT"));

    let text = std::fs::read_to_string(root.join("data").join("records.cfd")).expect("read cfd");
    assert!(!text.contains("bad_holder"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn default_materialization_rejects_singleton_top_level_type() {
    let root = std::env::temp_dir().join(format!(
        "coflow-data-patch-singleton-default-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        r"
            @singleton
            type GameConfig { max_level: int; }
        ",
    )
    .expect("write schema");
    std::fs::write(
        root.join("data").join("records.cfd"),
        r"GameConfig: GameConfig { max_level: 10 }
",
    )
    .expect("write cfd");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\nsources:\n  - path: data\noutputs:\n  data:\n    type: json\n    dir: generated/data\n",
    )
    .expect("write config");
    let session = session(&root);

    let singleton_default = session
        .default_record_value("GameConfig", DefaultMaterialization::EditableShape)
        .expect_err("singleton type should not be materialized");
    assert!(singleton_default
        .iter()
        .any(|diagnostic| diagnostic.code == "MUTATION-DEFAULT"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn patch_file_guard_stops_batch_with_failed_report() {
    let root = std::env::temp_dir().join(format!("coflow-data-patch-guard-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    write_project(&root);
    let mut session = session(&root);

    let report = session.apply_data_patch(DataPatchRequest {
        stop_on_write_error: true,
        ops: vec![
            DataPatchOp::SetField {
                record: PatchRecordSelector {
                    actual_type: "Item".to_string(),
                    key: "sword".to_string(),
                },
                file: Some("data/other.cfd".to_string()),
                path: vec![PatchPathSegment::Field("price".to_string())],
                value: json!(200),
            },
            DataPatchOp::SetField {
                record: PatchRecordSelector {
                    actual_type: "Item".to_string(),
                    key: "sword".to_string(),
                },
                file: None,
                path: vec![PatchPathSegment::Field("name".to_string())],
                value: json!("Stopped"),
            },
        ],
    });

    assert!(!report.write_ok);
    assert!(!report.check_ok);
    assert!(report.applied.is_empty());
    assert_eq!(report.failed.len(), 1);
    let failed = report.failed.first().expect("failed op");
    assert_eq!(failed.index, 0);
    assert!(failed
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "MUTATION-FILE-GUARD"));

    let text = std::fs::read_to_string(root.join("data").join("items.cfd")).expect("read cfd");
    assert!(!text.contains("Stopped"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn patch_coerces_ref_inline_object_and_enum_key_dict_values() {
    let root =
        std::env::temp_dir().join(format!("coflow-data-patch-complex-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    write_project(&root);
    let mut session = session(&root);

    let report = session.apply_data_patch(DataPatchRequest {
        stop_on_write_error: true,
        ops: vec![DataPatchOp::InsertRecord {
            file: "data/items.cfd".to_string(),
            sheet: None,
            actual_type: "Loot".to_string(),
            key: "starter_loot".to_string(),
            materialization: DefaultMaterialization::Minimal,
            fields: serde_json::from_value(json!({
                "owner": "sword",
                "rewards": [
                    {
                        "$type": "ItemReward",
                        "item": "sword",
                        "count": 1
                    }
                ],
                "resistances": {
                    "$dict": [
                        { "key": "Fire", "value": 10 }
                    ]
                }
            }))
            .expect("fields map"),
        }],
    });

    assert!(report.write_ok);
    assert!(report.check_ok);
    assert_eq!(report.applied.len(), 1);
    assert!(report.failed.is_empty());

    let view = session
        .queries()
        .record_view("Loot", "starter_loot")
        .expect("inserted loot");
    assert!(view.record.fields().contains_key("owner"));
    assert!(view.record.fields().contains_key("rewards"));
    assert!(view.record.fields().contains_key("resistances"));

    let text = std::fs::read_to_string(root.join("data").join("items.cfd")).expect("read cfd");
    assert!(text.contains("&sword"));
    assert!(text.contains("ItemReward"));
    assert!(text.contains("Fire"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn patch_supports_dict_key_path_writes() {
    let root = std::env::temp_dir().join(format!(
        "coflow-data-patch-dict-path-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    write_project(&root);
    let mut session = session(&root);

    let report = session.apply_data_patch(DataPatchRequest {
        stop_on_write_error: true,
        ops: vec![DataPatchOp::InsertRecord {
            file: "data/items.cfd".to_string(),
            sheet: None,
            actual_type: "Loot".to_string(),
            key: "starter_loot".to_string(),
            materialization: DefaultMaterialization::Minimal,
            fields: serde_json::from_value(json!({
                "rewards": [],
                "resistances": { "$dict": [{ "key": "Fire", "value": 10 }] }
            }))
            .expect("fields map"),
        }],
    });
    assert!(report.write_ok);

    let report = session.apply_data_patch(DataPatchRequest {
        stop_on_write_error: true,
        ops: vec![DataPatchOp::SetField {
            record: PatchRecordSelector {
                actual_type: "Loot".to_string(),
                key: "starter_loot".to_string(),
            },
            file: None,
            path: vec![
                PatchPathSegment::Field("resistances".to_string()),
                PatchPathSegment::DictKey("Fire".to_string()),
            ],
            value: json!(20),
        }],
    });

    assert!(report.write_ok);
    assert!(report.failed.is_empty());
    let view = session
        .queries()
        .record_view("Loot", "starter_loot")
        .expect("inserted loot");
    let Some(coflow_data_model::CfdValue::Dict(entries)) = view.record.fields().get("resistances")
    else {
        panic!("resistances should be dict");
    };
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].1, coflow_data_model::CfdValue::Int(20));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn mutation_cfd_value_accepts_null_for_nullable_fields() {
    let root =
        std::env::temp_dir().join(format!("coflow-data-patch-cfd-null-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    write_project(&root);
    let mut session = session(&root);

    let report = session.apply_mutation(MutationRequest {
        stop_on_write_error: true,
        ops: vec![MutationOp::SetField {
            record: RecordCoordinate::new("Loot", "starter_loot"),
            file: None,
            path: vec![PatchPathSegment::Field("owner".to_string())],
            value: MutationValue::Cfd(coflow_data_model::CfdValue::Null),
        }],
    });

    assert!(!report.write_ok);
    assert!(report.failed[0]
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "MUTATION-PATH"));

    let report = session.apply_data_patch(DataPatchRequest {
        stop_on_write_error: true,
        ops: vec![DataPatchOp::InsertRecord {
            file: "data/items.cfd".to_string(),
            sheet: None,
            actual_type: "Loot".to_string(),
            key: "starter_loot".to_string(),
            materialization: DefaultMaterialization::Minimal,
            fields: serde_json::from_value(json!({ "rewards": [] })).expect("fields map"),
        }],
    });
    assert!(report.write_ok);

    let report = session.apply_mutation(MutationRequest {
        stop_on_write_error: true,
        ops: vec![MutationOp::SetField {
            record: RecordCoordinate::new("Loot", "starter_loot"),
            file: None,
            path: vec![PatchPathSegment::Field("owner".to_string())],
            value: MutationValue::Cfd(coflow_data_model::CfdValue::Null),
        }],
    });

    assert!(report.write_ok);
    assert!(report.failed.is_empty());
    let view = session
        .queries()
        .record_view("Loot", "starter_loot")
        .expect("inserted loot");
    assert_eq!(
        view.record.fields().get("owner"),
        Some(&coflow_data_model::CfdValue::Null)
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn mutation_cfd_value_rejects_nested_values_that_do_not_match_schema() {
    let root = std::env::temp_dir().join(format!(
        "coflow-data-patch-cfd-nested-invalid-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    write_project(&root);
    let mut session = session(&root);

    let bad_reward =
        coflow_data_model::CfdValue::Object(Box::new(coflow_data_model::CfdObject::new(
            "ItemReward",
            std::collections::BTreeMap::from([
                (
                    "item".to_string(),
                    coflow_data_model::CfdValue::Ref("sword".to_string()),
                ),
                (
                    "count".to_string(),
                    coflow_data_model::CfdValue::String("bad".to_string()),
                ),
            ]),
        )));

    let report = session.apply_data_patch(DataPatchRequest {
        stop_on_write_error: true,
        ops: vec![DataPatchOp::InsertRecord {
            file: "data/items.cfd".to_string(),
            sheet: None,
            actual_type: "Loot".to_string(),
            key: "starter_loot".to_string(),
            materialization: DefaultMaterialization::Minimal,
            fields: serde_json::from_value(json!({ "rewards": [] })).expect("fields map"),
        }],
    });
    assert!(report.write_ok);

    let report = session.apply_mutation(MutationRequest {
        stop_on_write_error: true,
        ops: vec![MutationOp::SetField {
            record: RecordCoordinate::new("Loot", "starter_loot"),
            file: None,
            path: vec![PatchPathSegment::Field("rewards".to_string())],
            value: MutationValue::Cfd(coflow_data_model::CfdValue::Array(vec![bad_reward])),
        }],
    });

    assert!(!report.write_ok);
    assert!(report.failed[0]
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "MUTATION-VALUE"));
    let text = std::fs::read_to_string(root.join("data").join("items.cfd")).expect("read cfd");
    assert!(!text.contains("bad"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn mutation_complete_value_rejects_missing_nested_required_fields_before_write() {
    let root = std::env::temp_dir().join(format!(
        "coflow-data-patch-cfd-nested-incomplete-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    write_project(&root);
    let mut session = session(&root);

    let insert = session.apply_data_patch(DataPatchRequest {
        stop_on_write_error: true,
        ops: vec![DataPatchOp::InsertRecord {
            file: "data/items.cfd".to_string(),
            sheet: None,
            actual_type: "Loot".to_string(),
            key: "starter_loot".to_string(),
            materialization: DefaultMaterialization::Minimal,
            fields: serde_json::from_value(json!({ "rewards": [] })).expect("fields map"),
        }],
    });
    assert!(insert.write_ok);
    let before = std::fs::read_to_string(root.join("data/items.cfd")).expect("read cfd");

    let incomplete_reward =
        coflow_data_model::CfdValue::Object(Box::new(coflow_data_model::CfdObject::new(
            "ItemReward",
            std::collections::BTreeMap::from([(
                "item".to_string(),
                coflow_data_model::CfdValue::Ref("sword".to_string()),
            )]),
        )));
    let report = session.apply_mutation(MutationRequest {
        stop_on_write_error: true,
        ops: vec![MutationOp::SetField {
            record: RecordCoordinate::new("Loot", "starter_loot"),
            file: None,
            path: vec![PatchPathSegment::Field("rewards".to_string())],
            value: MutationValue::Cfd(coflow_data_model::CfdValue::Array(vec![incomplete_reward])),
        }],
    });

    assert!(!report.write_ok);
    assert!(report.failed[0].diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "MUTATION-SHAPE"
            && diagnostic
                .message
                .contains("missing required field `count` on object type `ItemReward`")
    }));
    let after = std::fs::read_to_string(root.join("data/items.cfd")).expect("read cfd");
    assert_eq!(after, before, "provider must not see an incomplete value");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn patch_collects_validation_failures_when_stop_disabled() {
    let root = std::env::temp_dir().join(format!(
        "coflow-data-patch-validation-failures-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    write_project(&root);
    let mut session = session(&root);

    let report = session.apply_data_patch(DataPatchRequest {
        stop_on_write_error: false,
        ops: vec![
            DataPatchOp::SetField {
                record: PatchRecordSelector {
                    actual_type: "Item".to_string(),
                    key: "sword".to_string(),
                },
                file: None,
                path: vec![PatchPathSegment::Field("missing".to_string())],
                value: json!(1),
            },
            DataPatchOp::SetField {
                record: PatchRecordSelector {
                    actual_type: "Item".to_string(),
                    key: "sword".to_string(),
                },
                file: None,
                path: vec![PatchPathSegment::Field("rarity".to_string())],
                value: json!("NotARarity"),
            },
            DataPatchOp::SetField {
                record: PatchRecordSelector {
                    actual_type: "Item".to_string(),
                    key: "sword".to_string(),
                },
                file: None,
                path: vec![PatchPathSegment::Field("name".to_string())],
                value: json!("Continued"),
            },
        ],
    });

    assert!(!report.write_ok);
    assert!(!report.check_ok);
    assert_eq!(report.failed.len(), 2);
    assert_eq!(report.applied.len(), 1);
    assert!(report.failed[0]
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "MUTATION-PATH"));
    assert!(report.failed[1]
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "MUTATION-VALUE"));
    assert!(!report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "MUTATION-PATH"));
    assert!(!report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "MUTATION-VALUE"));

    let text = std::fs::read_to_string(root.join("data").join("items.cfd")).expect("read cfd");
    assert!(text.contains("Continued"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn patch_stops_on_terminal_writer_error_even_when_stop_disabled() {
    let root = std::env::temp_dir().join(format!(
        "coflow-data-patch-terminal-writer-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    write_project(&root);
    let mut session = session(&root);

    let report = session.apply_data_patch(DataPatchRequest {
        stop_on_write_error: false,
        ops: vec![
            DataPatchOp::InsertRecord {
                file: "data/items.cfd".to_string(),
                sheet: None,
                actual_type: "Item".to_string(),
                key: "sword".to_string(),
                materialization: DefaultMaterialization::Minimal,
                fields: serde_json::from_value(json!({
                    "name": "Duplicate Sword",
                    "price": 1
                }))
                .expect("fields map"),
            },
            DataPatchOp::SetField {
                record: PatchRecordSelector {
                    actual_type: "Item".to_string(),
                    key: "sword".to_string(),
                },
                file: None,
                path: vec![PatchPathSegment::Field("name".to_string())],
                value: json!("Should Not Run"),
            },
        ],
    });

    assert!(!report.write_ok);
    assert!(!report.check_ok);
    assert!(report.applied.is_empty());
    assert_eq!(report.failed.len(), 1);
    assert!(report.failed[0]
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "MUTATION-INSERT-CONFLICT"));
    assert!(!report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "MUTATION-INSERT-CONFLICT"));

    let text = std::fs::read_to_string(root.join("data").join("items.cfd")).expect("read cfd");
    assert!(!text.contains("Should Not Run"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn patch_set_field_file_guard_uses_spread_source_file() {
    let root = std::env::temp_dir().join(format!(
        "coflow-data-patch-spread-file-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    write_spread_project(&root);
    let mut session = session(&root);

    let report = session.apply_data_patch(DataPatchRequest {
        stop_on_write_error: true,
        ops: vec![DataPatchOp::SetField {
            record: PatchRecordSelector {
                actual_type: "Item".to_string(),
                key: "child".to_string(),
            },
            file: Some("data/source.cfd".to_string()),
            path: vec![PatchPathSegment::Field("name".to_string())],
            value: json!("Edited Through Spread"),
        }],
    });

    assert!(report.write_ok);
    assert!(report.failed.is_empty());
    assert_eq!(report.applied.len(), 1);
    assert_eq!(report.applied[0].file.as_deref(), Some("data/source.cfd"));

    let source = std::fs::read_to_string(root.join("data").join("source.cfd")).expect("source");
    let host = std::fs::read_to_string(root.join("data").join("host.cfd")).expect("host");
    assert!(source.contains("Edited Through Spread"));
    assert!(!host.contains("Edited Through Spread"));

    let report = session.apply_data_patch(DataPatchRequest {
        stop_on_write_error: true,
        ops: vec![DataPatchOp::SetField {
            record: PatchRecordSelector {
                actual_type: "Item".to_string(),
                key: "child".to_string(),
            },
            file: Some("data/host.cfd".to_string()),
            path: vec![PatchPathSegment::Field("power".to_string())],
            value: json!(2),
        }],
    });

    assert!(!report.write_ok);
    assert_eq!(report.failed.len(), 1);
    assert!(report.failed[0]
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "MUTATION-FILE-GUARD"));

    let source = std::fs::read_to_string(root.join("data").join("source.cfd")).expect("source");
    let host = std::fs::read_to_string(root.join("data").join("host.cfd")).expect("host");
    assert!(!source.contains("power: 2"));
    assert!(!host.contains("power: 2"));

    let _ = std::fs::remove_dir_all(root);
}
