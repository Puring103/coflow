//! Round-trip tests for `ExcelWriter`: write a cell value, re-read with
//! calamine, assert the new value plus that adjacent cells are unchanged.
#![allow(clippy::expect_used, clippy::panic, clippy::panic_in_result_fn, clippy::unwrap_used)]

use calamine::{open_workbook_auto, Data, Reader};
use coflow_api::{
    CfdValue, DataWriter, RecordOrigin, ResolvedSource, SourceDocument, SourceLocationSpec,
    WriteCellRequest, WriteContext, WriteFieldPathSegment,
};
use coflow_cft::CftContainer;
use coflow_loader_excel::ExcelWriter;
use rust_xlsxwriter::{Workbook, XlsxError};
use std::collections::BTreeMap;
use std::path::PathBuf;
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
fn origin_for_sword(path: &PathBuf) -> RecordOrigin {
    let mut field_columns = BTreeMap::new();
    field_columns.insert(vec!["name".to_string()], 2);
    field_columns.insert(vec!["value".to_string()], 3);
    RecordOrigin::Table {
        document: SourceDocument::Local(path.clone()),
        sheet: "Items".to_string(),
        row: 2,
        id_column: 1,
        field_columns,
    }
}

fn empty_source(path: &PathBuf) -> ResolvedSource {
    ResolvedSource {
        provider_id: "excel".to_string(),
        location: SourceLocationSpec::Path(path.clone()),
        options: serde_json::Value::default(),
        display_name: path.display().to_string(),
    }
}

fn read_cell(path: &PathBuf, sheet_name: &str, row: usize, col: usize) -> String {
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

#[test]
fn writes_string_cell_and_preserves_neighbors() {
    let path = temp_xlsx("scalar");
    write_seed_workbook(&path).expect("seed");

    let schema = CftContainer::new();
    let source = empty_source(&path);
    let origin = origin_for_sword(&path);
    let new_value = CfdValue::String("New Sword".to_string());
    let segments = vec![WriteFieldPathSegment::Field("name".to_string())];
    let writer = ExcelWriter::new();
    writer
        .write_field(
            WriteContext {
                project_root: &std::env::temp_dir(),
                schema: &schema,
                model: None,
            },
            &WriteCellRequest {
                origin: &origin,
                record_key: "sword",
                actual_type: "Item",
                field_path: &segments,
                new_value: &new_value,
                schema: &schema,
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
    let source = empty_source(&path);
    let origin = origin_for_sword(&path);
    let new_value = CfdValue::Int(99);
    let segments = vec![WriteFieldPathSegment::Field("value".to_string())];
    let writer = ExcelWriter::new();
    writer
        .write_field(
            WriteContext {
                project_root: &std::env::temp_dir(),
                schema: &schema,
                model: None,
            },
            &WriteCellRequest {
                origin: &origin,
                record_key: "sword",
                actual_type: "Item",
                field_path: &segments,
                new_value: &new_value,
                schema: &schema,
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
fn rejects_missing_file_with_friendly_error() {
    let path = std::env::temp_dir().join("coflow-excel-writer-no-such-file.xlsx");
    if path.exists() {
        std::fs::remove_file(&path).expect("rm pre-existing");
    }
    let schema = CftContainer::new();
    let source = empty_source(&path);
    let origin = origin_for_sword(&path);
    let new_value = CfdValue::String("X".to_string());
    let segments = vec![WriteFieldPathSegment::Field("name".to_string())];
    let writer = ExcelWriter::new();
    let Err(diag) = writer.write_field(
        WriteContext {
            project_root: &std::env::temp_dir(),
            schema: &schema,
            model: None,
        },
        &WriteCellRequest {
            origin: &origin,
            record_key: "sword",
            actual_type: "Item",
            field_path: &segments,
            new_value: &new_value,
            schema: &schema,
            source: &source,
        },
    ) else {
        panic!("missing file should fail");
    };
    assert!(diag
        .iter()
        .any(|d| d.message.contains("does not exist")));
}
