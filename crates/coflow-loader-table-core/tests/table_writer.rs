#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::unwrap_used
)]

use coflow_cft::{build_schema, parse_modules, CftDimensionInputs, CftFile, CftSchema, ModuleId};
use coflow_data_model::{
    CfdDataModel, CfdValue, LoadedRecordDraft, LoadedValueDraft, RecordOrigin, SourceDocument,
};
use coflow_loader_table_core::writer::{
    plan_field_write, plan_insert_record, plan_reorder_records, HeaderReconciliationPlan,
    TableFieldWrite, TableInsertRecord, TableMoveRowBefore, TableRecordRef, TableReorderOperation,
    TableSetCell, TableWritePlan, WriteFieldPathSegment,
};
use coflow_loader_table_core::{resolve_table_write_layout, TableSheetConfig};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn compile_schema(source: &str) -> CftSchema {
    let modules = parse_modules([CftFile::from_source(ModuleId::from("main"), source)]);
    build_schema(&modules, &CftDimensionInputs::default()).expect("schema compile")
}

fn table_origin(field_columns: BTreeMap<Vec<String>, usize>) -> RecordOrigin {
    RecordOrigin::Table {
        document: SourceDocument::new(PathBuf::from("data.xlsx")),
        sheet: "Items".to_string(),
        row: 2,
        id_column: 1,
        field_columns,
    }
}

fn table_origin_at(row: usize) -> RecordOrigin {
    RecordOrigin::Table {
        document: SourceDocument::new(PathBuf::from("data.xlsx")),
        sheet: "Items".to_string(),
        row,
        id_column: 1,
        field_columns: BTreeMap::new(),
    }
}

#[test]
fn move_record_plan_preserves_source_and_anchor_guards() {
    let source = table_origin_at(2);
    let before = table_origin_at(4);
    let plan = plan_reorder_records(TableReorderOperation::MoveBefore {
        record: TableRecordRef {
            origin: &source,
            record_key: "sword",
        },
        before: Some(TableRecordRef {
            origin: &before,
            record_key: "potion",
        }),
    })
    .expect("move plan");

    assert_eq!(
        plan,
        TableWritePlan::MoveRowBefore(TableMoveRowBefore {
            document: SourceDocument::new(PathBuf::from("data.xlsx")),
            sheet: "Items".to_string(),
            row: 2,
            id_column: 1,
            expected_key: "sword".to_string(),
            before_row: Some(4),
            before_id_column: Some(1),
            expected_before_key: Some("potion".to_string()),
        })
    );
}

fn field_path(name: &str) -> Vec<WriteFieldPathSegment> {
    vec![WriteFieldPathSegment::Field(name.to_string())]
}

#[test]
fn header_reconciliation_preserves_values_across_add_remove_and_reorder() {
    let source = vec!["id".to_string(), "name".to_string(), "obsolete".to_string()];
    let target = vec!["name".to_string(), "id".to_string(), "power".to_string()];
    let plan = HeaderReconciliationPlan::new(&source, &target);

    assert_eq!(plan.added(), &["power".to_string()]);
    assert_eq!(plan.removed(), &["obsolete".to_string()]);
    assert_eq!(plan.source_column(0), Some(1));
    assert_eq!(plan.source_column(1), Some(0));
    assert_eq!(plan.source_column(2), None);
    assert_eq!(plan.storage_width(), 3);
    assert_eq!(
        plan.project_rows(&[
            source,
            vec![
                "sword".to_string(),
                "Sword".to_string(),
                "legacy".to_string()
            ],
        ]),
        vec![
            target,
            vec!["Sword".to_string(), "sword".to_string(), String::new()],
        ]
    );
}

#[test]
fn header_reconciliation_matches_repeated_expand_columns_by_occurrence() {
    let source = vec![
        "id".to_string(),
        "env".to_string(),
        String::new(),
        String::new(),
        "name".to_string(),
    ];
    let target = vec![
        "id".to_string(),
        "name".to_string(),
        "env".to_string(),
        String::new(),
        String::new(),
    ];
    let plan = HeaderReconciliationPlan::new(&source, &target);

    assert!(plan.added().is_empty());
    assert!(plan.removed().is_empty());
    assert_eq!(
        plan.project_row(&[
            "water".to_string(),
            "4".to_string(),
            "20".to_string(),
            "0.5".to_string(),
            "Lake".to_string(),
        ]),
        vec![
            "water".to_string(),
            "Lake".to_string(),
            "4".to_string(),
            "20".to_string(),
            "0.5".to_string(),
        ]
    );
}

#[test]
fn header_reconciliation_tracks_duplicate_column_cardinality() {
    let source = vec!["id".to_string(), String::new(), String::new()];
    let target = vec!["id".to_string(), String::new()];
    let plan = HeaderReconciliationPlan::new(&source, &target);

    assert!(plan.added().is_empty());
    assert_eq!(plan.removed(), &[String::new()]);
    assert_eq!(plan.source_width(), 3);
    assert_eq!(plan.target_width(), 2);
    assert_eq!(plan.storage_width(), 3);
}

#[test]
fn nested_collection_edit_rewrites_owning_cell_value() {
    let schema = compile_schema(
        r"
        type Item {
          tags: [string];
        }
        ",
    );
    let input = LoadedRecordDraft::new(
        "sword",
        "Item",
        [(
            "tags",
            LoadedValueDraft::Array(vec![
                LoadedValueDraft::String("old".to_string()),
                LoadedValueDraft::String("keep".to_string()),
            ]),
        )],
    )
    .with_origin(table_origin(BTreeMap::from([(
        vec!["tags".to_string()],
        2,
    )])));
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_loaded_record(input);
    let model = builder.build().expect("model");
    let origin = table_origin(BTreeMap::from([(vec!["tags".to_string()], 2)]));
    let new_value = CfdValue::String("new".to_string());
    let path = vec![
        WriteFieldPathSegment::Field("tags".to_string()),
        WriteFieldPathSegment::Index(0),
    ];
    let request = TableFieldWrite {
        origin: &origin,
        record_key: "sword",
        actual_type: "Item",
        field_path: &path,
        new_value: &new_value,
        model: Some(&model),
    };

    let plan = plan_field_write(&request).expect("plan");

    assert_eq!(
        plan,
        TableWritePlan::SetCells {
            document: SourceDocument::new(PathBuf::from("data.xlsx")),
            sheet: "Items".to_string(),
            id_column: 1,
            expected_key: "sword".to_string(),
            cells: vec![TableSetCell {
                row: 2,
                column: 2,
                value: "[new | keep]".to_string(),
            }],
        }
    );
}

#[test]
fn nested_dict_entry_edit_rewrites_owning_cell_value() {
    let schema = compile_schema(
        r"
        type Item {
          weights: {string: int};
        }
        ",
    );
    let input = LoadedRecordDraft::new(
        "sword",
        "Item",
        [(
            "weights",
            LoadedValueDraft::dict([
                (
                    coflow_data_model::LoadedDictKeyDraft::from("rare"),
                    LoadedValueDraft::Int(1),
                ),
                (
                    coflow_data_model::LoadedDictKeyDraft::from("common"),
                    LoadedValueDraft::Int(2),
                ),
            ]),
        )],
    )
    .with_origin(table_origin(BTreeMap::from([(
        vec!["weights".to_string()],
        2,
    )])));
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_loaded_record(input);
    let model = builder.build().expect("model");
    let origin = table_origin(BTreeMap::from([(vec!["weights".to_string()], 2)]));
    let new_value = CfdValue::Int(9);
    let path = vec![
        WriteFieldPathSegment::Field("weights".to_string()),
        WriteFieldPathSegment::DictKey("\"rare\"".to_string()),
    ];
    let request = TableFieldWrite {
        origin: &origin,
        record_key: "sword",
        actual_type: "Item",
        field_path: &path,
        new_value: &new_value,
        model: Some(&model),
    };

    let plan = plan_field_write(&request).expect("plan");

    assert_eq!(
        plan,
        TableWritePlan::SetCells {
            document: SourceDocument::new(PathBuf::from("data.xlsx")),
            sheet: "Items".to_string(),
            id_column: 1,
            expected_key: "sword".to_string(),
            cells: vec![TableSetCell {
                row: 2,
                column: 2,
                value: "{rare: 9, common: 2}".to_string(),
            }],
        }
    );
}

#[test]
fn replacing_ref_inside_array_rewrites_owning_cell() {
    let schema = compile_schema(
        r"
        type Reward {
          name: string;
          amount: int;
        }

        type Drop {
          rewards: [&Reward];
        }
        ",
    );
    let input = LoadedRecordDraft::new(
        "drop_1",
        "Drop",
        [(
            "rewards",
            LoadedValueDraft::Array(vec![LoadedValueDraft::record_ref("coin")]),
        )],
    )
    .with_origin(table_origin(BTreeMap::from([(
        vec!["rewards".to_string()],
        2,
    )])));
    let source = LoadedRecordDraft::new(
        "coin",
        "Reward",
        [
            ("name", LoadedValueDraft::String("Coin".to_string())),
            ("amount", LoadedValueDraft::Int(10)),
        ],
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_loaded_record(source);
    builder.add_loaded_record(input);
    let model = builder.build().expect("model");
    let origin = table_origin(BTreeMap::from([(vec!["rewards".to_string()], 2)]));
    let new_value = CfdValue::record_ref("gem").unwrap();
    let path = vec![
        WriteFieldPathSegment::Field("rewards".to_string()),
        WriteFieldPathSegment::Index(0),
    ];
    let request = TableFieldWrite {
        origin: &origin,
        record_key: "drop_1",
        actual_type: "Drop",
        field_path: &path,
        new_value: &new_value,
        model: Some(&model),
    };

    let plan = plan_field_write(&request).expect("array cell rewrite should succeed");

    assert_eq!(
        plan,
        TableWritePlan::SetCells {
            document: SourceDocument::new(PathBuf::from("data.xlsx")),
            sheet: "Items".to_string(),
            id_column: 1,
            expected_key: "drop_1".to_string(),
            cells: vec![TableSetCell {
                row: 2,
                column: 2,
                value: "[&gem]".to_string(),
            }],
        }
    );
}

#[test]
fn expanded_object_edit_writes_each_child_column() {
    let _schema = compile_schema(
        r"
        type Stats {
          hp: int;
          attack: int;
        }

        type Monster {
          stats: Stats;
        }
        ",
    );
    let field_columns = BTreeMap::from([
        (vec!["stats".to_string()], 2),
        (vec!["stats".to_string(), "hp".to_string()], 2),
        (vec!["stats".to_string(), "attack".to_string()], 3),
    ]);
    let origin = table_origin(field_columns);
    let stats = CfdValue::Object(Box::new(
        coflow_data_model::CfdObject::try_new(
            "Stats",
            BTreeMap::from([
                ("hp".to_string(), CfdValue::Int(100)),
                ("attack".to_string(), CfdValue::Int(9)),
            ]),
        )
        .unwrap(),
    ));
    let path = field_path("stats");
    let request = TableFieldWrite {
        origin: &origin,
        record_key: "monster",
        actual_type: "Monster",
        field_path: &path,
        new_value: &stats,
        model: None,
    };

    let plan = plan_field_write(&request).expect("plan");

    let TableWritePlan::SetCells { cells, .. } = plan else {
        panic!("expected set cells");
    };
    assert_eq!(
        cells,
        vec![
            TableSetCell {
                row: 2,
                column: 3,
                value: "9".to_string(),
            },
            TableSetCell {
                row: 2,
                column: 2,
                value: "100".to_string(),
            },
        ]
    );
}

#[test]
fn unmapped_field_path_returns_diagnostic() {
    let origin = table_origin(BTreeMap::new());
    let path = field_path("missing");
    let new_value = CfdValue::Int(1);
    let request = TableFieldWrite {
        origin: &origin,
        record_key: "item_1",
        actual_type: "Item",
        field_path: &path,
        new_value: &new_value,
        model: None,
    };

    let Err(err) = plan_field_write(&request) else {
        panic!("missing column should fail");
    };

    assert!(err.iter().any(|d| d.message.contains("does not map")));
}

#[test]
fn insert_record_plan_renders_id_and_known_fields() {
    let fields = BTreeMap::from([
        ("name".to_string(), CfdValue::String("Sword".to_string())),
        ("power".to_string(), CfdValue::Int(7)),
    ]);
    let field_columns = BTreeMap::from([
        (vec!["name".to_string()], 2),
        (vec!["power".to_string()], 3),
    ]);

    let plan = plan_insert_record(&TableInsertRecord {
        document: SourceDocument::new(PathBuf::from("data.xlsx")),
        sheet: "Items",
        record_key: "sword",
        actual_type: "Item",
        fields: &fields,
        field_columns: &field_columns,
        id_column: 1,
        before: None,
    })
    .expect("insert plan");

    assert_eq!(
        plan,
        TableWritePlan::AppendRow(coflow_loader_table_core::writer::TableAppendRow {
            document: SourceDocument::new(PathBuf::from("data.xlsx")),
            sheet: "Items".to_string(),
            values: vec![
                (1, "sword".to_string()),
                (2, "Sword".to_string()),
                (3, "7".to_string()),
            ],
            before_row: None,
            before_id_column: None,
            expected_before_key: None,
        })
    );
}

#[test]
fn write_layout_resolves_expand_child_columns_from_header() {
    let schema = compile_schema(
        r"
        @struct sealed type EnvCfg {
          shc: float;
          temperature: float;
          diffusion: float;
        }

        type Terrain {
          name: string;
          @expand
          env: EnvCfg;
        }
        ",
    );
    let schema = &schema;
    let header = vec![
        "id".to_string(),
        "name".to_string(),
        "env".to_string(),
        String::new(),
        String::new(),
    ];

    let layout = resolve_table_write_layout(
        schema,
        &PathBuf::from("terrain.xlsx"),
        &TableSheetConfig::new("Terrain"),
        &header,
    )
    .expect("layout");

    assert_eq!(layout.id_column, 1);
    assert_eq!(
        layout.field_columns,
        BTreeMap::from([
            (vec!["name".to_string()], 2),
            (vec!["env".to_string()], 3),
            (vec!["env".to_string(), "shc".to_string()], 3),
            (vec!["env".to_string(), "temperature".to_string()], 4),
            (vec!["env".to_string(), "diffusion".to_string()], 5),
        ])
    );
}
