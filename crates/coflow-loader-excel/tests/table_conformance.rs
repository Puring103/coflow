#![allow(clippy::expect_used, clippy::panic, clippy::cast_possible_truncation)]

#[path = "../../../tests/support/table_conformance.rs"]
mod table_conformance;

use calamine::{open_workbook_auto, Data, Reader};
use coflow_api::{
    ResolvedSource, SourceLocationSpec, SourceProvider, SyncHeaderRequest, TableContext,
    TableManager,
};
use coflow_loader_excel::{ExcelLoader, ExcelWriter};
use rust_xlsxwriter::Workbook;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use table_conformance::table_conformance_cases;

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

fn temp_xlsx(name: &str) -> PathBuf {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "coflow-excel-table-conformance-{}-{id}-{name}.xlsx",
        std::process::id()
    ))
}

fn write_workbook(path: &Path, rows: &[Vec<String>]) {
    let mut workbook = Workbook::new();
    let sheet = workbook
        .add_worksheet()
        .set_name("Items")
        .expect("name worksheet");
    for (row_index, row) in rows.iter().enumerate() {
        for (column_index, value) in row.iter().enumerate() {
            sheet
                .write_string(row_index as u32, column_index as u16, value)
                .expect("write worksheet cell");
        }
    }
    workbook.save(path).expect("save workbook fixture");
}

fn read_rows(path: &Path, width: usize) -> Vec<Vec<String>> {
    let mut workbook = open_workbook_auto(path).expect("open workbook result");
    let range = workbook.worksheet_range("Items").expect("read worksheet");
    range
        .rows()
        .map(|source| {
            (0..width)
                .map(|column| source.get(column).map_or_else(String::new, cell_text))
                .collect()
        })
        .collect()
}

fn cell_text(cell: &Data) -> String {
    match cell {
        Data::Empty => String::new(),
        Data::String(value) => value.clone(),
        Data::Float(value) => value.to_string(),
        Data::Int(value) => value.to_string(),
        Data::Bool(value) => value.to_string(),
        other => format!("{other:?}"),
    }
}

fn excel_source(path: &Path) -> ResolvedSource {
    ResolvedSource {
        provider_id: "excel".to_string(),
        location: SourceLocationSpec::Path(path.to_path_buf()),
        options: ExcelLoader
            .decode_options(&serde_json::Value::Null)
            .expect("decode excel options"),
        display_name: path.display().to_string(),
    }
}

#[test]
fn excel_table_manager_passes_shared_header_conformance() {
    for case in table_conformance_cases() {
        let path = temp_xlsx(case.name);
        write_workbook(&path, &case.source_rows);
        let source = excel_source(&path);
        let result = ExcelWriter::new()
            .sync_header(
                TableContext {
                    project_root: std::env::temp_dir().as_path(),
                },
                &SyncHeaderRequest {
                    source: &source,
                    sheet: Some("Items"),
                    actual_type: "Item",
                    headers: &case.target_header,
                    schema: None,
                },
            )
            .expect("sync excel header");

        assert_eq!(
            read_rows(&path, case.target_header.len()),
            case.expected_rows,
            "case {}",
            case.name
        );
        assert_eq!(result.added, case.added, "case {}", case.name);
        assert_eq!(result.removed, case.removed, "case {}", case.name);
        std::fs::remove_file(path).expect("remove workbook fixture");
    }
}
