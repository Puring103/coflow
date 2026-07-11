#![allow(clippy::expect_used, clippy::panic)]

#[path = "../../../tests/support/table_conformance.rs"]
mod table_conformance;

use coflow_api::{
    ResolvedSource, SourceLocationSpec, SourceProvider, SyncHeaderRequest, TableContext,
    TableManager,
};
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
