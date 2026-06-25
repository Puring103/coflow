#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::unwrap_used
)]

use coflow_cft::{CftContainer, ModuleId};
use coflow_data_model::{
    CfdDataModel, CfdInputRecord, CfdInputValue, CfdValue, RecordOrigin, SourceDocument,
};
use coflow_loader_table_core::writer::{
    plan_field_write, plan_insert_record, TableFieldWrite, TableInsertRecord, TableSetCell,
    TableWritePlan, WriteFieldPathSegment,
};
use coflow_loader_table_core::{resolve_table_write_layout, TableSheetConfig};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn compile_schema(source: &str) -> CftContainer {
    let mut container = CftContainer::new();
    container
        .add_module(ModuleId::from("main"), source)
        .expect("schema parse");
    container.compile().expect("schema compile");
    container
}

fn table_origin(field_columns: BTreeMap<Vec<String>, usize>) -> RecordOrigin {
    RecordOrigin::Table {
        document: SourceDocument::Local(PathBuf::from("data.xlsx")),
        sheet: "Items".to_string(),
        row: 2,
        id_column: 1,
        field_columns,
    }
}

fn field_path(name: &str) -> Vec<WriteFieldPathSegment> {
    vec![WriteFieldPathSegment::Field(name.to_string())]
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
    let input = CfdInputRecord::new(
        "sword",
        "Item",
        [(
            "tags",
            CfdInputValue::Array(vec![
                CfdInputValue::String("old".to_string()),
                CfdInputValue::String("keep".to_string()),
            ]),
        )],
    )
    .with_origin(table_origin(BTreeMap::from([(
        vec!["tags".to_string()],
        2,
    )])));
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_input_record(input);
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
            document: SourceDocument::Local(PathBuf::from("data.xlsx")),
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
    let stats = CfdValue::Object(Box::new(coflow_data_model::CfdRecord {
        key: String::new(),
        actual_type: "Stats".to_string(),
        fields: BTreeMap::from([
            ("hp".to_string(), CfdValue::Int(100)),
            ("attack".to_string(), CfdValue::Int(9)),
        ]),
        origin: RecordOrigin::None,
        spread_field_sources: BTreeMap::new(),
    }));
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
    let _schema = CftContainer::new();
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
        document: SourceDocument::Local(PathBuf::from("data.xlsx")),
        sheet: "Items",
        record_key: "sword",
        actual_type: "Item",
        fields: &fields,
        field_columns: &field_columns,
        id_column: 1,
    })
    .expect("insert plan");

    assert_eq!(
        plan,
        TableWritePlan::AppendRow(coflow_loader_table_core::writer::TableAppendRow {
            document: SourceDocument::Local(PathBuf::from("data.xlsx")),
            sheet: "Items".to_string(),
            values: vec![
                (1, "sword".to_string()),
                (2, "Sword".to_string()),
                (3, "7".to_string()),
            ],
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
    let header = vec![
        "id".to_string(),
        "name".to_string(),
        "env".to_string(),
        String::new(),
        String::new(),
    ];

    let layout = resolve_table_write_layout(
        &schema,
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
