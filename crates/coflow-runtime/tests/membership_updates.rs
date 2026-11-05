#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use coflow_runtime::{CfdValue, Project, RecordCoordinate, Runtime, WriteProjectSession};
use std::collections::BTreeMap;
use std::fmt::Write as _;

fn assert_full_reload(session: &WriteProjectSession, project: &Project) {
    let full = Runtime::new()
        .open_read_only_session(project.clone())
        .unwrap();
    let actual = session.queries();
    let expected = full.queries();
    assert_eq!(actual.record_count(), expected.record_count());
    assert_eq!(
        actual.diagnostics().as_set(),
        expected.diagnostics().as_set()
    );
    assert_eq!(actual.rejected_records(), expected.rejected_records());
    for file in ["data/a.cfd", "data/b.cfd"] {
        let left = actual.record_views_in_file(file).collect::<Vec<_>>();
        let right = expected.record_views_in_file(file).collect::<Vec<_>>();
        assert_eq!(left.len(), right.len());
        for (left, right) in left.iter().zip(&right) {
            assert_eq!(left.coordinate, right.coordinate);
            assert_eq!(left.origin, right.origin);
            assert_eq!(left.record, right.record);
            assert_eq!(
                actual.record_references(&left.coordinate),
                expected.record_references(&right.coordinate)
            );
        }
    }
}

#[test]
fn member_mutations_reuse_values_and_preserve_checks_sources_and_failed_writes() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("data")).unwrap();
    std::fs::write(
        root.path().join("coflow.yaml"),
        "schema: schema.cft\ndata: data/\ncodegen:\n  - language: csharp\n    dir: generated/\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join("schema.cft"),
        "table Row { score: int; title: string = \"默认🙂\"; check Positive { score > 0; } }",
    )
    .unwrap();
    let mut source = String::new();
    for index in 0..128 {
        writeln!(source, "r{index}: Row {{score: 1}}").unwrap();
    }
    std::fs::write(root.path().join("data/a.cfd"), &source).unwrap();
    std::fs::write(root.path().join("data/b.cfd"), "# transfer destination\n").unwrap();
    let project = Project::open_schema_only(Some(root.path())).unwrap();
    let mut session = Runtime::new().open_write_session(project.clone()).unwrap();
    let old_records = session
        .queries()
        .record_views_in_file("data/a.cfd")
        .map(|view| view.record.clone())
        .collect::<Vec<_>>();
    let fields = BTreeMap::from([("score".to_owned(), CfdValue::Int(-1))]);
    session
        .insert_record("data/a.cfd", "new", "Row", &fields)
        .unwrap();
    assert_eq!(session.queries().record_count(), 129);
    assert!(
        session.queries().has_diagnostics(),
        "new record check must execute"
    );
    assert_full_reload(&session, &project);
    assert_eq!(session.queries().execution_stats().records_materialized, 1);
    assert_eq!(session.queries().execution_stats().records_validated, 1);

    session.rename_record_id("Row", "new", "renamed").unwrap();
    assert_full_reload(&session, &project);
    assert_eq!(session.queries().execution_stats().records_materialized, 1);
    assert_eq!(session.queries().execution_stats().records_validated, 1);

    session
        .transfer_record(
            &RecordCoordinate::try_new("Row", "renamed").unwrap(),
            "data/b.cfd",
            0,
        )
        .unwrap();
    assert_full_reload(&session, &project);
    assert_eq!(session.queries().execution_stats().records_materialized, 0);
    assert_eq!(session.queries().execution_stats().records_validated, 0);

    let before_a = std::fs::read(root.path().join("data/a.cfd")).unwrap();
    let before_b = std::fs::read(root.path().join("data/b.cfd")).unwrap();
    assert!(session.rename_record_id("Row", "renamed", "r0").is_err());
    assert_eq!(
        std::fs::read(root.path().join("data/a.cfd")).unwrap(),
        before_a
    );
    assert_eq!(
        std::fs::read(root.path().join("data/b.cfd")).unwrap(),
        before_b
    );
    assert_full_reload(&session, &project);

    session.delete_record("Row", "renamed").unwrap();
    assert_full_reload(&session, &project);
    assert!(!session.queries().has_diagnostics());
    assert_eq!(session.queries().execution_stats().records_materialized, 0);
    assert_eq!(session.queries().execution_stats().records_validated, 0);
    assert_eq!(old_records.len(), 128);
    assert!(old_records
        .iter()
        .all(|record| record.field("score") == Some(&CfdValue::Int(1))));
}
