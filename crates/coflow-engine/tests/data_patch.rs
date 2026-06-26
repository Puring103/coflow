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
                item: Item;
                count: int;
            }

            type Loot {
                rewards: [ItemReward];
                resistances: {Element: int} = {};
                owner: Item? = null;
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
    ...@Item.base,
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

fn registry() -> coflow_api::ProviderRegistry {
    let mut registry = coflow_api::ProviderRegistry::default();
    registry
        .register_loader(coflow_loader_cfd::CfdLoader)
        .expect("cfd loader");
    registry
        .register_writer(coflow_loader_cfd::CfdWriter::new())
        .expect("cfd writer");
    registry
}

fn session(
    root: &std::path::Path,
) -> (coflow_engine::ProjectSession, coflow_api::ProviderRegistry) {
    let project = Project::open_schema_only(Some(&root.join("coflow.yaml"))).expect("open");
    let registry = registry();
    let session = build_project_session(project, &registry).expect("session");
    (session, registry)
}

#[test]
fn patch_inserts_and_edits_cfd_records_then_reports_check_diagnostics() {
    let root = std::env::temp_dir().join(format!("coflow-data-patch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    write_project(&root);
    let (mut session, registry) = session(&root);

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
    assert!(report.failed.is_empty());
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
fn patch_file_guard_stops_batch_with_failed_report() {
    let root = std::env::temp_dir().join(format!("coflow-data-patch-guard-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    write_project(&root);
    let (mut session, registry) = session(&root);

    let report = session
        .apply_data_patch(
            &registry,
            DataPatchRequest {
                check_after_write: true,
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
            },
        )
        .expect("write error should be reported, not returned as Err");

    assert!(!report.write_ok);
    assert!(!report.check_ok);
    assert!(report.applied.is_empty());
    assert_eq!(report.failed.len(), 1);
    let failed = report.failed.first().expect("failed op");
    assert_eq!(failed.index, 0);
    assert!(failed
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "PATCH-FILE-GUARD"));

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
    let (mut session, registry) = session(&root);

    let report = session
        .apply_data_patch(
            &registry,
            DataPatchRequest {
                check_after_write: true,
                stop_on_write_error: true,
                ops: vec![DataPatchOp::InsertRecord {
                    file: "data/items.cfd".to_string(),
                    actual_type: "Loot".to_string(),
                    key: "starter_loot".to_string(),
                    fields: serde_json::from_value(json!({
                        "owner": { "$ref": { "type": "Item", "key": "sword" } },
                        "rewards": [
                            {
                                "$type": "ItemReward",
                                "item": { "$ref": "Item.sword" },
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
            },
        )
        .expect("patch should write");

    assert!(report.write_ok);
    assert!(report.check_ok);
    assert_eq!(report.applied.len(), 1);
    assert!(report.failed.is_empty());

    let view = session
        .record_view("Loot", "starter_loot")
        .expect("inserted loot");
    assert!(view.record.fields.contains_key("owner"));
    assert!(view.record.fields.contains_key("rewards"));
    assert!(view.record.fields.contains_key("resistances"));

    let text = std::fs::read_to_string(root.join("data").join("items.cfd")).expect("read cfd");
    assert!(text.contains("@Item.sword"));
    assert!(text.contains("ItemReward"));
    assert!(text.contains("Fire"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn patch_rejects_dict_key_path_writes() {
    let root = std::env::temp_dir().join(format!(
        "coflow-data-patch-dict-path-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    write_project(&root);
    let (mut session, registry) = session(&root);

    let report = session
        .apply_data_patch(
            &registry,
            DataPatchRequest {
                check_after_write: true,
                stop_on_write_error: true,
                ops: vec![DataPatchOp::InsertRecord {
                    file: "data/items.cfd".to_string(),
                    actual_type: "Loot".to_string(),
                    key: "starter_loot".to_string(),
                    fields: serde_json::from_value(json!({
                        "rewards": [],
                        "resistances": { "$dict": [{ "key": "Fire", "value": 10 }] }
                    }))
                    .expect("fields map"),
                }],
            },
        )
        .expect("insert loot");
    assert!(report.write_ok);

    let report = session
        .apply_data_patch(
            &registry,
            DataPatchRequest {
                check_after_write: true,
                stop_on_write_error: true,
                ops: vec![DataPatchOp::SetField {
                    record: PatchRecordSelector {
                        actual_type: "Loot".to_string(),
                        key: "starter_loot".to_string(),
                    },
                    file: None,
                    path: vec![
                        PatchPathSegment::Field("resistances".to_string()),
                        PatchPathSegment::Field("Fire".to_string()),
                    ],
                    value: json!(20),
                }],
            },
        )
        .expect("path error should be reported, not returned as Err");

    assert!(!report.write_ok);
    assert_eq!(report.failed.len(), 1);
    let failed = report.failed.first().expect("failed op");
    assert!(failed
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "PATCH-PATH"));

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
    let (mut session, registry) = session(&root);

    let report = session
        .apply_data_patch(
            &registry,
            DataPatchRequest {
                check_after_write: true,
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
            },
        )
        .expect("validation failures should be reported");

    assert!(!report.write_ok);
    assert!(!report.check_ok);
    assert_eq!(report.failed.len(), 2);
    assert_eq!(report.applied.len(), 1);
    assert!(report.failed[0]
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "PATCH-PATH"));
    assert!(report.failed[1]
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "PATCH-VALUE"));
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "PATCH-PATH"));
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "PATCH-VALUE"));

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
    let (mut session, registry) = session(&root);

    let report = session
        .apply_data_patch(
            &registry,
            DataPatchRequest {
                check_after_write: true,
                stop_on_write_error: false,
                ops: vec![
                    DataPatchOp::InsertRecord {
                        file: "data/items.cfd".to_string(),
                        actual_type: "Item".to_string(),
                        key: "sword".to_string(),
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
            },
        )
        .expect("terminal write error should be reported");

    assert!(!report.write_ok);
    assert!(!report.check_ok);
    assert!(report.applied.is_empty());
    assert_eq!(report.failed.len(), 1);
    assert!(report.failed[0]
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "CFD-WRITE"));
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "CFD-WRITE"));

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
    let (mut session, registry) = session(&root);

    let report = session
        .apply_data_patch(
            &registry,
            DataPatchRequest {
                check_after_write: true,
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
            },
        )
        .expect("spread source guarded write");

    assert!(report.write_ok);
    assert!(report.failed.is_empty());
    assert_eq!(report.applied.len(), 1);
    assert_eq!(report.applied[0].file.as_deref(), Some("data/source.cfd"));

    let source = std::fs::read_to_string(root.join("data").join("source.cfd")).expect("source");
    let host = std::fs::read_to_string(root.join("data").join("host.cfd")).expect("host");
    assert!(source.contains("Edited Through Spread"));
    assert!(!host.contains("Edited Through Spread"));

    let report = session
        .apply_data_patch(
            &registry,
            DataPatchRequest {
                check_after_write: true,
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
            },
        )
        .expect("spread guard failure should be reported");

    assert!(!report.write_ok);
    assert_eq!(report.failed.len(), 1);
    assert!(report.failed[0]
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "PATCH-FILE-GUARD"));

    let source = std::fs::read_to_string(root.join("data").join("source.cfd")).expect("source");
    let host = std::fs::read_to_string(root.join("data").join("host.cfd")).expect("host");
    assert!(!source.contains("power: 2"));
    assert!(!host.contains("power: 2"));

    let _ = std::fs::remove_dir_all(root);
}
