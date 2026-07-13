//! Round-trip tests for `ExcelWriter`: write a cell value, re-read with
//! calamine, assert the new value plus that adjacent cells are unchanged.
#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::unwrap_used
)]

use calamine::{open_workbook_auto, Data, Reader};
use coflow_api::{
    CreateTableRequest, DeleteRecordRequest, InsertRecordRequest, ResolvedSource,
    SourceLocationSpec, SourceProvider, SourceWriter, SyncHeaderRequest, TableContext,
    TableManager, WriteCellRequest, WriteContext, WriteFieldPathSegment,
};
use coflow_cft::CftContainer;
use coflow_data_model::{
    CfdDataModel, CfdInputRecord, CfdInputValue, CfdObject, CfdValue, RecordOrigin, SourceDocument,
};
use coflow_loader_excel::{ExcelLoader, ExcelWriter};
use rust_xlsxwriter::{Workbook, XlsxError};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

fn temp_xlsx(name: &str) -> PathBuf {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join("coflow-excel-writer");
    std::fs::create_dir_all(&dir).expect("mkdir temp");
    dir.join(format!("{name}-{id}.xlsx"))
}

fn write_seed_workbook(path: &PathBuf) -> Result<(), XlsxError> {
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet().set_name("Items")?;
    // Header row.
    sheet.write_string(0, 0, "id")?;
    sheet.write_string(0, 1, "name")?;
    sheet.write_string(0, 2, "value")?;
    // Data rows.
    sheet.write_string(1, 0, "sword")?;
    sheet.write_string(1, 1, "Old")?;
    sheet.write_string(1, 2, "10")?;
    sheet.write_string(2, 0, "shield")?;
    sheet.write_string(2, 1, "Round")?;
    sheet.write_string(2, 2, "5")?;
    workbook.save(path)
}

/// Hand-build a `RecordOrigin::Table` matching the test workbook's "sword"
/// row. The Excel loader normally produces one of these — but for a writer
/// round-trip test we don't need to involve the loader.
fn origin_for_sword(path: &Path) -> RecordOrigin {
    let mut field_columns = BTreeMap::new();
    field_columns.insert(vec!["name".to_string()], 2);
    field_columns.insert(vec!["value".to_string()], 3);
    RecordOrigin::Table {
        document: SourceDocument::Local(path.to_path_buf()),
        sheet: "Items".to_string(),
        row: 2,
        id_column: 1,
        field_columns,
    }
}

fn origin_for_shield(path: &Path) -> RecordOrigin {
    let mut field_columns = BTreeMap::new();
    field_columns.insert(vec!["name".to_string()], 2);
    field_columns.insert(vec!["value".to_string()], 3);
    RecordOrigin::Table {
        document: SourceDocument::Local(path.to_path_buf()),
        sheet: "Items".to_string(),
        row: 3,
        id_column: 1,
        field_columns,
    }
}

fn empty_source(path: &Path) -> ResolvedSource {
    ResolvedSource {
        provider_id: "excel".to_string(),
        location: SourceLocationSpec::Path(path.to_path_buf()),
        options: ExcelLoader
            .decode_options(&serde_json::Value::Null)
            .expect("decode excel options"),
        display_name: path.display().to_string(),
    }
}

#[allow(clippy::cast_possible_truncation)]
fn read_cell(path: &Path, sheet_name: &str, row: usize, col: usize) -> String {
    let mut workbook = open_workbook_auto(path).expect("re-open xlsx");
    let range = workbook.worksheet_range(sheet_name).expect("worksheet");
    let cell = range
        .get_value((row as u32 - 1, col as u32 - 1))
        .cloned()
        .unwrap_or(Data::Empty);
    match cell {
        Data::Empty => String::new(),
        Data::String(s) => s,
        Data::Float(v) => format!("{v}"),
        Data::Int(v) => v.to_string(),
        Data::Bool(v) => v.to_string(),
        other => format!("{other:?}"),
    }
}

fn schema_for_items() -> CftContainer {
    let mut schema = CftContainer::new();
    schema
        .add_module(
            coflow_cft::ModuleId::from("main"),
            r"
            type Item {
              name: string;
              value: int;
            }
            ",
        )
        .expect("schema parse");
    schema.compile().expect("schema compile");
    schema
}

#[test]
fn batches_multiple_field_writes_to_one_workbook() {
    let path = temp_xlsx("field-batch");
    write_seed_workbook(&path).expect("seed workbook");
    let schema = schema_for_items();
    let source = empty_source(&path);
    let sword_origin = origin_for_sword(&path);
    let shield_origin = origin_for_shield(&path);
    let name_path = [WriteFieldPathSegment::Field("name".to_string())];
    let sword_name = CfdValue::String("Sharp".to_string());
    let shield_name = CfdValue::String("Sturdy".to_string());
    let requests = [
        WriteCellRequest {
            origin: &sword_origin,
            record_key: "sword",
            actual_type: "Item",
            field_path: &name_path,
            new_value: &sword_name,
            schema: schema.compiled_schema(),
            source: &source,
        },
        WriteCellRequest {
            origin: &shield_origin,
            record_key: "shield",
            actual_type: "Item",
            field_path: &name_path,
            new_value: &shield_name,
            schema: schema.compiled_schema(),
            source: &source,
        },
    ];

    let outcomes = ExcelWriter::new()
        .write_field_batch(
            WriteContext {
                project_root: &std::env::temp_dir(),
                schema: schema.compiled_schema(),
                model: None,
            },
            &requests,
        )
        .expect("batch write succeeds");

    assert_eq!(outcomes.len(), 2);
    assert_eq!(read_cell(&path, "Items", 2, 2), "Sharp");
    assert_eq!(read_cell(&path, "Items", 3, 2), "Sturdy");
}

#[test]
fn writer_capabilities_are_derived_from_workbook_format() {
    let writer = ExcelWriter::new();
    let writable_source = empty_source(Path::new("items.xlsx"));
    let macro_source = empty_source(Path::new("items.xlsm"));
    let legacy_source = empty_source(Path::new("items.xls"));

    let writable = writer.capabilities(&writable_source);
    assert!(writable.can_edit_field);
    assert!(writable.can_edit_key);
    assert!(writable.can_insert_record);
    assert!(writable.can_delete_record);

    for read_only in [
        writer.capabilities(&macro_source),
        writer.capabilities(&legacy_source),
    ] {
        assert!(!read_only.can_edit_field);
        assert!(!read_only.can_edit_key);
        assert!(!read_only.can_insert_record);
        assert!(!read_only.can_delete_record);
    }
}

#[test]
fn writer_rejects_xlsm_and_xls_before_mutating_workbook_bytes() {
    let seed = temp_xlsx("read-only-format-seed");
    write_seed_workbook(&seed).expect("seed workbook");
    let schema = CftContainer::new();
    let compiled_schema = schema.compiled_schema();
    let new_value = CfdValue::String("Changed".to_string());
    let segments = vec![WriteFieldPathSegment::Field("name".to_string())];
    let writer = ExcelWriter::new();

    for extension in ["xlsm", "xls"] {
        let path = seed.with_extension(extension);
        std::fs::copy(&seed, &path).expect("copy workbook package");
        let before = std::fs::read(&path).expect("read before");
        let source = empty_source(&path);
        let origin = origin_for_sword(&path);
        let request = WriteCellRequest {
            origin: &origin,
            record_key: "sword",
            actual_type: "Item",
            field_path: &segments,
            new_value: &new_value,
            schema: compiled_schema,
            source: &source,
        };
        let context = WriteContext {
            project_root: &std::env::temp_dir(),
            schema: compiled_schema,
            model: None,
        };

        let preflight = writer.preflight(context, &request);
        assert!(preflight
            .iter()
            .any(|diagnostic| diagnostic.code == "EXCEL-FORMAT-READ-ONLY"));
        let error = writer
            .write_field(context, &request)
            .expect_err("read-only format should reject writes");
        assert!(error
            .iter()
            .any(|diagnostic| diagnostic.code == "EXCEL-FORMAT-READ-ONLY"));
        assert_eq!(std::fs::read(&path).expect("read after"), before);
    }
}

#[test]
fn table_manager_rejects_unsafe_formats_before_create_or_sync() {
    let writer = ExcelWriter::new();
    let headers = vec!["id".to_string(), "name".to_string()];
    let table_context = TableContext {
        project_root: &std::env::temp_dir(),
    };

    let create_path = temp_xlsx("read-only-create").with_extension("xlsm");
    let create_source = empty_source(&create_path);
    let create_error = writer
        .create_table(
            table_context,
            &CreateTableRequest {
                source: &create_source,
                sheet: "Items",
                actual_type: "Item",
                headers: &headers,
            },
        )
        .expect_err("xlsm create should fail preflight");
    assert!(create_error
        .iter()
        .any(|diagnostic| diagnostic.code == "EXCEL-FORMAT-READ-ONLY"));
    assert!(!create_path.exists());

    let seed = temp_xlsx("read-only-sync-seed");
    write_seed_workbook(&seed).expect("seed workbook");
    let sync_path = seed.with_extension("xls");
    std::fs::copy(&seed, &sync_path).expect("copy workbook package");
    let before = std::fs::read(&sync_path).expect("read before");
    let sync_source = empty_source(&sync_path);
    let sync_error = writer
        .sync_header(
            table_context,
            &SyncHeaderRequest {
                source: &sync_source,
                sheet: Some("Items"),
                actual_type: "Item",
                headers: &headers,
                schema: None,
            },
        )
        .expect_err("xls sync should fail preflight");
    assert!(sync_error
        .iter()
        .any(|diagnostic| diagnostic.code == "EXCEL-FORMAT-READ-ONLY"));
    assert_eq!(std::fs::read(&sync_path).expect("read after"), before);
}

fn schema_for_tagged_items() -> CftContainer {
    let mut schema = CftContainer::new();
    schema
        .add_module(
            coflow_cft::ModuleId::from("main"),
            r"
            type Item {
              tags: [string];
            }
            ",
        )
        .expect("schema parse");
    schema.compile().expect("schema compile");
    schema
}

fn write_tagged_workbook(path: &PathBuf) -> Result<(), XlsxError> {
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet().set_name("Items")?;
    sheet.write_string(0, 0, "id")?;
    sheet.write_string(0, 1, "tags")?;
    sheet.write_string(1, 0, "sword")?;
    sheet.write_string(1, 1, "[old | keep]")?;
    workbook.save(path)
}

fn schema_for_terrain() -> CftContainer {
    let mut schema = CftContainer::new();
    schema
        .add_module(
            coflow_cft::ModuleId::from("main"),
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
        )
        .expect("schema parse");
    schema.compile().expect("schema compile");
    schema
}

fn write_terrain_workbook_with_expand(path: &PathBuf) -> Result<(), XlsxError> {
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet().set_name("Terrain")?;
    sheet.write_string(0, 0, "id")?;
    sheet.write_string(0, 1, "name")?;
    sheet.write_string(0, 2, "env")?;
    sheet.write_string(1, 0, "Water")?;
    sheet.write_string(1, 1, "lake")?;
    sheet.write_number(1, 2, 4.0)?;
    sheet.write_number(1, 3, 20.0)?;
    sheet.write_number(1, 4, 0.5)?;
    workbook.save(path)
}

fn expanded_env_value(shc: f64, temperature: f64, diffusion: f64) -> CfdValue {
    CfdValue::Object(Box::new(CfdObject::new(
        "EnvCfg",
        BTreeMap::from([
            ("shc".to_string(), CfdValue::Float(shc)),
            ("temperature".to_string(), CfdValue::Float(temperature)),
            ("diffusion".to_string(), CfdValue::Float(diffusion)),
        ]),
    )))
}

#[test]
fn writes_string_cell_and_preserves_neighbors() {
    let path = temp_xlsx("scalar");
    write_seed_workbook(&path).expect("seed");

    let schema = CftContainer::new();

    let compiled_schema = schema.compiled_schema();
    let source = empty_source(&path);
    let origin = origin_for_sword(&path);
    let new_value = CfdValue::String("New Sword".to_string());
    let segments = vec![WriteFieldPathSegment::Field("name".to_string())];
    let writer = ExcelWriter::new();
    writer
        .write_field(
            WriteContext {
                project_root: &std::env::temp_dir(),
                schema: compiled_schema,
                model: None,
            },
            &WriteCellRequest {
                origin: &origin,
                record_key: "sword",
                actual_type: "Item",
                field_path: &segments,
                new_value: &new_value,
                schema: compiled_schema,
                source: &source,
            },
        )
        .expect("write succeeds");

    assert_eq!(read_cell(&path, "Items", 2, 2), "New Sword");
    // Other cells in the same row are unchanged.
    assert_eq!(read_cell(&path, "Items", 2, 1), "sword");
    assert_eq!(read_cell(&path, "Items", 2, 3), "10");
    // The sibling row is unchanged.
    assert_eq!(read_cell(&path, "Items", 3, 2), "Round");
    assert_eq!(read_cell(&path, "Items", 3, 3), "5");
}

#[test]
fn writes_numeric_cell_as_text() {
    let path = temp_xlsx("number");
    write_seed_workbook(&path).expect("seed");

    let schema = CftContainer::new();

    let compiled_schema = schema.compiled_schema();
    let source = empty_source(&path);
    let origin = origin_for_sword(&path);
    let new_value = CfdValue::Int(99);
    let segments = vec![WriteFieldPathSegment::Field("value".to_string())];
    let writer = ExcelWriter::new();
    writer
        .write_field(
            WriteContext {
                project_root: &std::env::temp_dir(),
                schema: compiled_schema,
                model: None,
            },
            &WriteCellRequest {
                origin: &origin,
                record_key: "sword",
                actual_type: "Item",
                field_path: &segments,
                new_value: &new_value,
                schema: compiled_schema,
                source: &source,
            },
        )
        .expect("write succeeds");

    // umya may write the integer as an actual number; calamine will return
    // either a numeric-looking text or `Data::Float(99.0)`. Accept both.
    let cell = read_cell(&path, "Items", 2, 3);
    assert!(cell == "99" || cell == "99.0", "cell={cell}");
}

#[test]
fn writes_record_key_cell() {
    let path = temp_xlsx("key");
    write_seed_workbook(&path).expect("seed");

    let schema = CftContainer::new();

    let compiled_schema = schema.compiled_schema();
    let source = empty_source(&path);
    let origin = origin_for_sword(&path);
    let new_value = CfdValue::String("blade".to_string());
    let segments = vec![WriteFieldPathSegment::Field("id".to_string())];
    let writer = ExcelWriter::new();
    writer
        .write_field(
            WriteContext {
                project_root: &std::env::temp_dir(),
                schema: compiled_schema,
                model: None,
            },
            &WriteCellRequest {
                origin: &origin,
                record_key: "sword",
                actual_type: "Item",
                field_path: &segments,
                new_value: &new_value,
                schema: compiled_schema,
                source: &source,
            },
        )
        .expect("write key succeeds");

    assert_eq!(read_cell(&path, "Items", 2, 1), "blade");
    assert_eq!(read_cell(&path, "Items", 2, 2), "Old");
}

#[test]
fn writes_collection_element_by_rewriting_owning_cell() {
    let path = temp_xlsx("collection-element");
    write_tagged_workbook(&path).expect("seed");

    let schema = schema_for_tagged_items();

    let compiled_schema = schema.compiled_schema();
    let source = empty_source(&path);
    let origin = RecordOrigin::Table {
        document: SourceDocument::Local(path.clone()),
        sheet: "Items".to_string(),
        row: 2,
        id_column: 1,
        field_columns: BTreeMap::from([(vec!["tags".to_string()], 2)]),
    };
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
    .with_origin(origin.clone());
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_input_record(input);
    let model = builder.build().expect("model");

    let new_value = CfdValue::String("new".to_string());
    let segments = vec![
        WriteFieldPathSegment::Field("tags".to_string()),
        WriteFieldPathSegment::Index(0),
    ];
    let writer = ExcelWriter::new();
    writer
        .write_field(
            WriteContext {
                project_root: &std::env::temp_dir(),
                schema: compiled_schema,
                model: Some(&model),
            },
            &WriteCellRequest {
                origin: &origin,
                record_key: "sword",
                actual_type: "Item",
                field_path: &segments,
                new_value: &new_value,
                schema: compiled_schema,
                source: &source,
            },
        )
        .expect("write collection element succeeds");

    assert_eq!(read_cell(&path, "Items", 2, 2), "[new | keep]");
}

#[test]
fn writes_expanded_object_fields_to_child_columns() {
    let path = temp_xlsx("expand-write");
    write_terrain_workbook_with_expand(&path).expect("seed");

    let schema = schema_for_terrain();

    let compiled_schema = schema.compiled_schema();
    let source = empty_source(&path);
    let source_def = coflow_loader_excel::ExcelSource::new(
        &path,
        vec![coflow_loader_excel::ExcelSheet::new("Terrain")],
    );
    let loaded = coflow_loader_excel::collect_input_records(compiled_schema, &[source_def])
        .expect("load records");
    let origin = loaded.records[0].origin.clone();
    let new_value = expanded_env_value(6.0, 21.5, 0.75);
    let segments = vec![WriteFieldPathSegment::Field("env".to_string())];
    let writer = ExcelWriter::new();
    writer
        .write_field(
            WriteContext {
                project_root: &std::env::temp_dir(),
                schema: compiled_schema,
                model: None,
            },
            &WriteCellRequest {
                origin: &origin,
                record_key: "Water",
                actual_type: "Terrain",
                field_path: &segments,
                new_value: &new_value,
                schema: compiled_schema,
                source: &source,
            },
        )
        .expect("write expanded object succeeds");

    assert!(matches!(
        read_cell(&path, "Terrain", 2, 3).as_str(),
        "6" | "6.0"
    ));
    assert_eq!(read_cell(&path, "Terrain", 2, 4), "21.5");
    assert_eq!(read_cell(&path, "Terrain", 2, 5), "0.75");
}

#[test]
fn rejects_missing_file_with_friendly_error() {
    let path = std::env::temp_dir().join("coflow-excel-writer-no-such-file.xlsx");
    if path.exists() {
        std::fs::remove_file(&path).expect("rm pre-existing");
    }
    let schema = CftContainer::new();
    let compiled_schema = schema.compiled_schema();
    let source = empty_source(&path);
    let origin = origin_for_sword(&path);
    let new_value = CfdValue::String("X".to_string());
    let segments = vec![WriteFieldPathSegment::Field("name".to_string())];
    let writer = ExcelWriter::new();
    let Err(diag) = writer.write_field(
        WriteContext {
            project_root: &std::env::temp_dir(),
            schema: compiled_schema,
            model: None,
        },
        &WriteCellRequest {
            origin: &origin,
            record_key: "sword",
            actual_type: "Item",
            field_path: &segments,
            new_value: &new_value,
            schema: compiled_schema,
            source: &source,
        },
    ) else {
        panic!("missing file should fail");
    };
    assert!(diag.iter().any(|d| d.message.contains("does not exist")));
}

#[test]
fn refuses_field_write_when_row_key_changed() {
    let path = temp_xlsx("row-key-guard");
    write_seed_workbook(&path).expect("seed");
    let mut workbook = umya_spreadsheet::reader::xlsx::read(&path).expect("read workbook");
    workbook
        .get_sheet_by_name_mut("Items")
        .expect("sheet")
        .get_cell_mut((1, 2))
        .set_value("other");
    umya_spreadsheet::writer::xlsx::write(&workbook, &path).expect("save workbook");

    let schema = CftContainer::new();

    let compiled_schema = schema.compiled_schema();
    let source = empty_source(&path);
    let origin = origin_for_sword(&path);
    let new_value = CfdValue::String("New Sword".to_string());
    let segments = vec![WriteFieldPathSegment::Field("name".to_string())];
    let writer = ExcelWriter::new();
    let Err(diag) = writer.write_field(
        WriteContext {
            project_root: &std::env::temp_dir(),
            schema: compiled_schema,
            model: None,
        },
        &WriteCellRequest {
            origin: &origin,
            record_key: "sword",
            actual_type: "Item",
            field_path: &segments,
            new_value: &new_value,
            schema: compiled_schema,
            source: &source,
        },
    ) else {
        panic!("stale row should fail");
    };
    assert!(diag
        .iter()
        .any(|d| d.message.contains("expected key `sword`")));
    assert_eq!(read_cell(&path, "Items", 2, 2), "Old");
}

#[test]
fn inserts_record_row_and_loader_can_read_it() {
    let path = temp_xlsx("insert");
    write_seed_workbook(&path).expect("seed");

    let schema = schema_for_items();

    let compiled_schema = schema.compiled_schema();
    let source = empty_source(&path);
    let writer = ExcelWriter::new();
    let fields = BTreeMap::from([
        ("name".to_string(), CfdValue::String("Potion".to_string())),
        ("value".to_string(), CfdValue::Int(3)),
    ]);
    let outcome = writer
        .insert_record(
            WriteContext {
                project_root: &std::env::temp_dir(),
                schema: compiled_schema,
                model: None,
            },
            &InsertRecordRequest {
                source: &source,
                sheet: Some("Items"),
                record_key: "potion",
                actual_type: "Item",
                fields: &fields,
                schema: compiled_schema,
            },
        )
        .expect("insert succeeds");

    assert!(outcome.diagnostics.is_empty());
    assert_eq!(read_cell(&path, "Items", 4, 1), "potion");
    assert_eq!(read_cell(&path, "Items", 4, 2), "Potion");
    assert!(matches!(
        read_cell(&path, "Items", 4, 3).as_str(),
        "3" | "3.0"
    ));

    let source_def = coflow_loader_excel::ExcelSource::new(
        path,
        vec![coflow_loader_excel::ExcelSheet::new("Items").with_type("Item")],
    );
    let compiled_schema = schema.compiled_schema();
    let loaded = coflow_loader_excel::collect_input_records(compiled_schema, &[source_def])
        .expect("reload records");
    assert!(loaded.records.iter().any(|record| record.key == "potion"));
}

#[test]
fn inserts_record_with_expanded_object_into_child_columns() {
    let path = temp_xlsx("insert-expand");
    write_terrain_workbook_with_expand(&path).expect("seed");

    let schema = schema_for_terrain();

    let compiled_schema = schema.compiled_schema();
    let source = empty_source(&path);
    let writer = ExcelWriter::new();
    let fields = BTreeMap::from([
        ("name".to_string(), CfdValue::String("desert".to_string())),
        ("env".to_string(), expanded_env_value(2.0, 35.0, 0.2)),
    ]);
    let outcome = writer
        .insert_record(
            WriteContext {
                project_root: &std::env::temp_dir(),
                schema: compiled_schema,
                model: None,
            },
            &InsertRecordRequest {
                source: &source,
                sheet: Some("Terrain"),
                record_key: "Sand",
                actual_type: "Terrain",
                fields: &fields,
                schema: compiled_schema,
            },
        )
        .expect("insert expanded record succeeds");

    assert!(outcome.diagnostics.is_empty());
    assert_eq!(read_cell(&path, "Terrain", 3, 1), "Sand");
    assert_eq!(read_cell(&path, "Terrain", 3, 2), "desert");
    assert!(matches!(
        read_cell(&path, "Terrain", 3, 3).as_str(),
        "2" | "2.0"
    ));
    assert!(matches!(
        read_cell(&path, "Terrain", 3, 4).as_str(),
        "35" | "35.0"
    ));
    assert_eq!(read_cell(&path, "Terrain", 3, 5), "0.2");
}

#[test]
fn deletes_record_row() {
    let path = temp_xlsx("delete");
    write_seed_workbook(&path).expect("seed");

    let schema = CftContainer::new();

    let compiled_schema = schema.compiled_schema();
    let source = empty_source(&path);
    let origin = origin_for_sword(&path);
    let writer = ExcelWriter::new();
    writer
        .delete_record(
            WriteContext {
                project_root: &std::env::temp_dir(),
                schema: compiled_schema,
                model: None,
            },
            &DeleteRecordRequest {
                origin: &origin,
                record_key: "sword",
                actual_type: "Item",
                source: &source,
            },
        )
        .expect("delete succeeds");

    assert_eq!(read_cell(&path, "Items", 2, 1), "shield");
    assert_eq!(read_cell(&path, "Items", 2, 2), "Round");
}

#[test]
fn refuses_delete_when_row_key_changed() {
    let path = temp_xlsx("delete-key-guard");
    write_seed_workbook(&path).expect("seed");

    let schema = CftContainer::new();

    let compiled_schema = schema.compiled_schema();
    let source = empty_source(&path);
    let origin = origin_for_shield(&path);
    let writer = ExcelWriter::new();
    let Err(diag) = writer.delete_record(
        WriteContext {
            project_root: &std::env::temp_dir(),
            schema: compiled_schema,
            model: None,
        },
        &DeleteRecordRequest {
            origin: &origin,
            record_key: "sword",
            actual_type: "Item",
            source: &source,
        },
    ) else {
        panic!("delete should reject mismatched key");
    };
    assert!(diag
        .iter()
        .any(|d| d.message.contains("expected key `sword`")));
    assert_eq!(read_cell(&path, "Items", 3, 1), "shield");
}
