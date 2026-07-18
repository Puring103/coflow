#![allow(clippy::expect_used, clippy::panic)]

#[path = "../../../tests/support/table_conformance.rs"]
mod table_conformance;

use coflow_api::{
    ReorderRecordsOperation, ReorderRecordsRequest, ResolvedSource, SourceLocationSpec,
    SourceProvider, SourceWriter, SyncHeaderRequest, TableContext, TableManager, WriteContext,
    WriteRecordRef,
};
use coflow_cft::{build_schema, parse_modules, CftDimensionInputs};
use coflow_data_model::{RecordOrigin, SourceDocument};
use coflow_loader_csv::{parse, write, CsvLoader, CsvWriter};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use table_conformance::table_conformance_cases;

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

fn temp_csv(name: &str) -> PathBuf {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "coflow-csv-table-conformance-{}-{id}-{name}.csv",
        std::process::id()
    ))
}

fn csv_source(path: &Path) -> ResolvedSource {
    ResolvedSource {
        provider_id: "csv".to_string(),
        location: SourceLocationSpec::Path(path.to_path_buf()),
        options: CsvLoader
            .decode_options(&serde_json::Value::Null)
            .expect("decode csv options"),
        display_name: path.display().to_string(),
    }
}

fn csv_origin(path: &Path, row: usize) -> RecordOrigin {
    RecordOrigin::Table {
        document: SourceDocument::Local(path.to_path_buf()),
        sheet: "Items".to_string(),
        row,
        id_column: 1,
        field_columns: Default::default(),
    }
}

#[test]
fn csv_writer_swaps_and_moves_complete_rows() {
    let path = temp_csv("reorder");
    std::fs::write(&path, "id,name\na,Alpha\nb,Beta\nc,Gamma\n").expect("seed csv");
    let source = csv_source(&path);
    let schema =
        build_schema(&parse_modules([]), &CftDimensionInputs::default()).expect("empty schema");
    let a = csv_origin(&path, 2);
    let c = csv_origin(&path, 4);
    let writer = CsvWriter::new();
    let project_root = std::env::temp_dir();
    let ctx = WriteContext {
        project_root: project_root.as_path(),
        schema: &schema,
        model: None,
    };

    writer
        .reorder_records(
            ctx,
            &ReorderRecordsRequest {
                source: &source,
                operation: ReorderRecordsOperation::Swap {
                    first: WriteRecordRef {
                        origin: &a,
                        record_key: "a",
                        actual_type: "Item",
                    },
                    second: WriteRecordRef {
                        origin: &c,
                        record_key: "c",
                        actual_type: "Item",
                    },
                },
            },
        )
        .expect("swap csv rows");
    assert_eq!(
        parse(&std::fs::read_to_string(&path).expect("read swapped")).expect("parse swapped"),
        vec![
            vec!["id".to_string(), "name".to_string()],
            vec!["c".to_string(), "Gamma".to_string()],
            vec!["b".to_string(), "Beta".to_string()],
            vec!["a".to_string(), "Alpha".to_string()],
        ]
    );

    writer
        .reorder_records(
            ctx,
            &ReorderRecordsRequest {
                source: &source,
                operation: ReorderRecordsOperation::MoveBefore {
                    record: WriteRecordRef {
                        origin: &a,
                        record_key: "c",
                        actual_type: "Item",
                    },
                    before: None,
                },
            },
        )
        .expect("move csv row to end");
    let rows = parse(&std::fs::read_to_string(&path).expect("read moved")).expect("parse moved");
    assert_eq!(rows[1][0], "b");
    assert_eq!(rows[2][0], "a");
    assert_eq!(rows[3][0], "c");
}

#[test]
fn csv_table_manager_passes_shared_header_conformance() {
    for case in table_conformance_cases() {
        let path = temp_csv(case.name);
        std::fs::write(&path, write(&case.source_rows)).expect("write csv fixture");
        let source = csv_source(&path);
        let result = CsvWriter::new()
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
            .expect("sync csv header");

        let actual = parse(&std::fs::read_to_string(&path).expect("read csv result"))
            .expect("parse csv result");
        assert_eq!(actual, case.expected_rows, "case {}", case.name);
        assert_eq!(result.added, case.added, "case {}", case.name);
        assert_eq!(result.removed, case.removed, "case {}", case.name);
        std::fs::remove_file(path).expect("remove csv fixture");
    }
}
