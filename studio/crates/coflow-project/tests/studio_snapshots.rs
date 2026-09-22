use coflow_project::{CfdPathSegment, CfdValue, Project, RecordCoordinate, Runtime};

fn project() -> (tempfile::TempDir, coflow_project::WriteProjectSession) {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("coflow.yaml"),
        "schema: schema.cft\ndata: data/\ncodegen:\n  - language: csharp\n    dir: generated/\n",
    )
    .unwrap();
    std::fs::create_dir(root.path().join("data")).unwrap();
    std::fs::write(root.path().join("schema.cft"), "table Item { value: int; }").unwrap();
    std::fs::write(root.path().join("data/a.cfd"), "a: Item { value: 1 }").unwrap();
    std::fs::write(root.path().join("data/b.cfd"), "b: Item { value: 2 }").unwrap();
    let session = Runtime::new()
        .open_write_session(Project::open(Some(root.path())).unwrap())
        .unwrap();
    (root, session)
}

#[test]
fn source_commit_publishes_the_candidate_and_only_reloads_the_edited_source() {
    let (root, mut session) = project();
    let path = root.path().join("data/a.cfd");
    let update = session
        .prepare_source_update(&path, "a: Item { value: 7 }")
        .unwrap();
    assert_eq!(
        session
            .queries()
            .field_value("Item", "a", &[CfdPathSegment::Field("value".into())]),
        Some(&CfdValue::Int(1))
    );
    session.commit_source_update(update).unwrap();
    assert_eq!(
        session
            .queries()
            .field_value("Item", "a", &[CfdPathSegment::Field("value".into())]),
        Some(&CfdValue::Int(7))
    );
    assert_eq!(session.queries().execution_stats().sources_reloaded, 1);
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        "a: Item { value: 7 }"
    );
    let cached = session
        .project()
        .source_store()
        .cached_or_read(&root.path().join("data/a.cfd"))
        .unwrap();
    assert_eq!(cached.text.as_ref(), "a: Item { value: 7 }");
}

#[test]
fn source_candidate_cannot_be_committed_to_another_session() {
    let (root, session) = project();
    let update = session
        .prepare_source_update(&root.path().join("data/a.cfd"), "a: Item { value: 7 }")
        .unwrap();
    let mut other = Runtime::new()
        .open_write_session(Project::open(Some(root.path())).unwrap())
        .unwrap();
    assert!(other.commit_source_update(update).is_err());
    assert_eq!(other.queries().revision(), 0);
}

#[test]
fn schema_source_commit_rebuilds_data_against_the_candidate_schema() {
    let (root, mut session) = project();
    let path = root.path().join("schema.cft");
    let update = session
        .prepare_source_update(&path, "table Item { value: int; added: int = 9; }")
        .unwrap();
    session.commit_source_update(update).unwrap();
    assert_eq!(
        session
            .queries()
            .field_value("Item", "b", &[CfdPathSegment::Field("added".into())]),
        Some(&CfdValue::Int(9))
    );
    assert!(session
        .prepare_source_update(&path, "table Item { value: Missing; }")
        .is_err());
    assert_eq!(session.queries().revision(), 1);
}

#[test]
fn deletion_includes_the_removed_coordinate_in_the_change_set() {
    let (_root, mut session) = project();
    let coordinate = RecordCoordinate::try_new("Item", "a").unwrap();
    let report = session.apply_mutation(coflow_project::MutationRequest {
        stop_on_write_error: true,
        ops: vec![coflow_project::MutationOp::DeleteRecord {
            record: coordinate.clone(),
            file: None,
        }],
    });
    assert!(report.write_ok);
    assert!(report.changed_records["data/a.cfd"].contains(&coordinate));
    assert!(session.queries().record_view("Item", "a").is_none());
}

#[test]
fn transfer_marks_both_source_and_destination_in_the_change_set() {
    let (_root, mut session) = project();
    let coordinate = RecordCoordinate::try_new("Item", "a").unwrap();
    let report = session.apply_mutation(coflow_project::MutationRequest {
        stop_on_write_error: true,
        ops: vec![coflow_project::MutationOp::TransferRecord {
            record: coordinate.clone(),
            source_file: Some("data/a.cfd".into()),
            destination_file: "data/b.cfd".into(),
            target_index: 0,
        }],
    });
    assert!(report.write_ok);
    for file in ["data/a.cfd", "data/b.cfd"] {
        assert!(report.changed_records[file].contains(&coordinate));
    }
    assert_eq!(
        session.queries().file_for_record("Item", "a"),
        Some("data/b.cfd")
    );
}

#[test]
fn unchanged_record_diagnostics_do_not_expand_an_unrelated_change_set() {
    let (root, _) = project();
    std::fs::write(root.path().join("schema.cft"),
        "table Item { value: int; check { Coflow::Check::require(self.value > 0, \"positive\"); } }").unwrap();
    std::fs::write(root.path().join("data/b.cfd"), "b: Item { value: -1 }").unwrap();
    let mut session = Runtime::new()
        .open_write_session(Project::open(Some(root.path())).unwrap())
        .unwrap();
    assert!(!session
        .queries()
        .diagnostics()
        .flat_diagnostics()
        .is_empty());
    let report = session.apply_mutation(coflow_project::MutationRequest {
        stop_on_write_error: true,
        ops: vec![coflow_project::MutationOp::SetField {
            record: RecordCoordinate::try_new("Item", "a").unwrap(),
            file: None,
            path: vec![CfdPathSegment::Field("value".into())],
            value: coflow_project::MutationValue::Cfd(CfdValue::Int(3)),
        }],
    });
    assert!(report.write_ok);
    assert!(!report.diagnostics.is_empty());
    assert!(!report.changed_records.contains_key("data/b.cfd"));
}

#[test]
fn prepared_source_commit_rejects_external_changes_without_overwriting_them() {
    let (root, mut session) = project();
    let path = root.path().join("data/a.cfd");
    let update = session
        .prepare_source_update(&path, "a: Item { value: 7 }")
        .unwrap();
    std::fs::write(&path, "a: Item { value: 9 }").unwrap();
    assert!(session.commit_source_update(update).is_err());
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        "a: Item { value: 9 }"
    );
    assert_eq!(session.queries().revision(), 0);
}

#[test]
fn prepared_source_commit_rejects_a_newer_session_generation() {
    let (root, mut session) = project();
    let path = root.path().join("data/a.cfd");
    let update = session
        .prepare_source_update(&path, "a: Item { value: 7 }")
        .unwrap();
    session
        .write_field(
            "Item",
            "b",
            &[CfdPathSegment::Field("value".into())],
            &CfdValue::Int(8),
        )
        .unwrap();
    assert!(session.commit_source_update(update).is_err());
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        "a: Item { value: 1 }"
    );
}

#[test]
fn mutation_changes_do_not_include_unchanged_file_records() {
    let (_root, mut session) = project();
    let report = session.apply_mutation(coflow_project::MutationRequest {
        stop_on_write_error: true,
        ops: vec![coflow_project::MutationOp::SetField {
            record: RecordCoordinate::try_new("Item", "a").unwrap(),
            file: None,
            path: vec![CfdPathSegment::Field("value".into())],
            value: coflow_project::MutationValue::Cfd(CfdValue::Int(3)),
        }],
    });
    assert!(report.write_ok);
    assert_eq!(report.changed_records.len(), 1);
    assert_eq!(
        report.changed_records["data/a.cfd"],
        vec![RecordCoordinate::try_new("Item", "a").unwrap()]
    );
}

#[test]
fn source_diagnostics_survive_an_unrelated_cached_reload() {
    let (root, _) = project();
    std::fs::write(root.path().join("data/b.cfd"), "b: Item { value: ??? }").unwrap();
    let mut session = Runtime::new()
        .open_write_session(Project::open(Some(root.path())).unwrap())
        .unwrap();
    let before = session.queries().diagnostics().as_set().diagnostics.len();
    assert!(before > 0);
    session
        .write_field(
            "Item",
            "a",
            &[CfdPathSegment::Field("value".into())],
            &CfdValue::Int(4),
        )
        .unwrap();
    assert_eq!(
        session.queries().diagnostics().as_set().diagnostics.len(),
        before
    );
}
