#![allow(clippy::expect_used)]

use coflow_runtime::{
    normalize_path, CfdPathSegment, CfdValue, DataSourceTextOverride, MutationOp, MutationRequest,
    MutationValue, Project, RecordCoordinate, Runtime,
};

#[test]
fn unchanged_source_diagnostics_survive_validation_and_other_file_mutations() {
    let root = tempfile::tempdir().expect("project");
    std::fs::create_dir(root.path().join("data")).expect("data");
    std::fs::write(
        root.path().join("coflow.yaml"),
        "schema: schema.cft\ndata: data/\ncodegen:\n  - language: csharp\n    dir: generated/\n",
    )
    .expect("config");
    std::fs::write(
        root.path().join("schema.cft"),
        "table Item { value: int; check Positive { value > 0; } }",
    )
    .expect("schema");
    std::fs::write(
        root.path().join("data/a.cfd"),
        "broken: Item { value: , }\nvalid: Item { value: 3 }\n",
    )
    .expect("broken source");
    let path = root.path().join("data/b.cfd");
    let initial = "edited: Item { value: 1 }\n";
    std::fs::write(&path, initial).expect("editable source");
    let project = Project::open_schema_only(Some(root.path())).expect("open project");
    let mut session = Runtime::new()
        .open_write_session(project.clone())
        .expect("session");
    assert_eq!(session.queries().record_count(), 2);
    assert!(session.queries().has_diagnostics());
    let context = session.validation_context();
    let override_text = |source: &str| {
        [DataSourceTextOverride {
            normalized_path: normalize_path(&path),
            source: source.to_owned(),
            deleted: false,
        }]
    };

    // 同版本快路径与完整候选检查必须返回相同诊断，缓存不能吞掉另一文件的错误。
    for source in [initial, "edited: Item { value: 2 }\n"] {
        let overrides = override_text(source);
        let full = Runtime::new()
            .open_read_only_session_with_source_overrides(project.clone(), &overrides)
            .expect("full validation");
        assert_eq!(
            serde_json::to_value(context.validate(&overrides)).expect("incremental diagnostics"),
            serde_json::to_value(full.queries().diagnostics().as_set().flat_diagnostics())
                .expect("full diagnostics")
        );
    }
    let result = session.apply_mutation(MutationRequest {
        stop_on_write_error: true,
        ops: vec![MutationOp::SetField {
            record: RecordCoordinate::try_new("Item", "edited").expect("coordinate"),
            file: None,
            path: vec![CfdPathSegment::Field("value".to_owned())],
            value: MutationValue::Cfd(CfdValue::Int(2)),
        }],
    });
    assert!(result.write_ok, "{result:?}");
    let written = std::fs::read(&path).expect("published bytes");
    assert!(result
        .published_files
        .contains(&coflow_runtime::PublishedFile::from_content(
            normalize_path(&path),
            Some(&written)
        )));

    assert!(!result.check_ok);
    let fresh = Runtime::new()
        .open_read_only_session(project)
        .expect("reload");
    assert_eq!(
        session.queries().diagnostics().as_set(),
        fresh.queries().diagnostics().as_set()
    );
    assert_eq!(
        session.queries().record_count(),
        fresh.queries().record_count()
    );
}

#[test]
fn disk_refresh_matches_full_generations_through_errors_membership_and_templates() {
    let root = tempfile::tempdir().expect("project");
    initialize_incremental_fixture(root.path());
    let a = root.path().join("data/a.cfd");
    let b = root.path().join("data/b.cfd");
    let c = root.path().join("data/c.cfd");
    std::fs::write(
        &a,
        "a: Item { name: \"A\", value: 0.0, parent: &b, caption: f\"{self.parent.name}\" }\n",
    )
    .expect("a");
    std::fs::write(
        &b,
        "b: Item { name: \"B\", value: 1.0, parent: &a, caption: f\"{self.parent.name}\" }\n",
    )
    .expect("b");
    let project = Project::open_schema_only(Some(root.path())).expect("project");
    let mut session = Runtime::new()
        .open_write_session(project.clone())
        .expect("session");
    let edits = [
        (&a, Some("a: Item { name: \"Longer A\", value: -0.0, parent: &b, caption: f\"{self.parent.name}\" }\n")),
        (&b, Some("// moved source\nb: Item { name: \"\", value: -1.0, parent: &a, caption: f\"{self.parent.name}\" }\n")),
        (&a, Some("a: Item { name: 42, value: 0.0, parent: &b, caption: \"invalid\" }\n")),
        (&a, Some("a: Item { name: \"restored\", value: 0.0, parent: &b, caption: f\"{self.parent.name}\" }\n")),
        (&c, Some("c: Item { name: \"new\", value: 1.0, parent: &a, caption: f\"{self.parent.name}\" }\n")),
        (&b, Some("b: Item { name: \"B\", value: 1.0, parent: &c, caption: f\"{self.parent.name}\" }\n")),
        (&c, None),
        (&b, Some("b: Item { name: \"B\", value: 1.0, parent: &a, caption: f\"{self.parent.name}\" }\nbroken: Item { value: , }\n")),
        (&b, Some("b: Item { name: \"B\", value: 1.0, parent: &a, caption: f\"{self.parent.name}\" }\n")),
        (&a, Some("// line shift only\na: Item { name: \"restored\", value: 0.0, parent: &b, caption: f\"{self.parent.name}\" }\n")),
    ];
    for (step, (path, source)) in edits.into_iter().enumerate() {
        match source {
            Some(source) => std::fs::write(path, source).expect("edit"),
            None => std::fs::remove_file(path).expect("delete"),
        }
        session = session
            .refresh_context()
            .refresh_data_sources(std::slice::from_ref(path))
            .expect("refresh");
        let full = Runtime::new()
            .open_read_only_session(project.clone())
            .expect("full");
        let actual = session.queries();
        let expected = full.queries();
        assert_eq!(
            actual.diagnostics().as_set(),
            expected.diagnostics().as_set(),
            "diagnostics step {step}"
        );
        assert_eq!(
            actual.rejected_records(),
            expected.rejected_records(),
            "rejected step {step}"
        );
        assert_eq!(
            actual.record_count(),
            expected.record_count(),
            "count step {step}"
        );
        for file in ["data/a.cfd", "data/b.cfd", "data/c.cfd"] {
            let left = actual.record_views_in_file(file).collect::<Vec<_>>();
            let right = expected.record_views_in_file(file).collect::<Vec<_>>();
            assert_eq!(left.len(), right.len(), "file {file} step {step}");
            for (left, right) in left.iter().zip(&right) {
                assert_eq!(left.coordinate, right.coordinate);
                assert_eq!(left.origin, right.origin, "origin step {step}");
                assert_eq!(
                    serde_json::to_value(left.record).expect("value"),
                    serde_json::to_value(right.record).expect("value"),
                    "record step {step}"
                );
                assert_eq!(
                    actual.record_references(&left.coordinate),
                    expected.record_references(&right.coordinate),
                    "references step {step}"
                );
                assert_eq!(
                    actual
                        .iter_record_references(&left.coordinate)
                        .collect::<Vec<_>>(),
                    expected.record_references(&right.coordinate)
                );
                assert_eq!(
                    actual.has_record_references(&left.coordinate),
                    !expected.record_references(&right.coordinate).is_empty()
                );
            }
        }
        if step == 0 {
            assert_eq!(actual.execution_stats().records_materialized, 2);
            assert_eq!(actual.execution_stats().sources_reloaded, 1);
            assert_eq!(actual.execution_stats().draft_records_collected, 0);
            assert_eq!(actual.execution_stats().identity_records_checked, 1);
            assert_eq!(actual.execution_stats().origin_records_checked, 1);
        }
    }
}

#[test]
fn unchanged_disk_refresh_reuses_sources_and_preserves_existing_diagnostics() {
    let root = tempfile::tempdir().expect("project");
    std::fs::write(
        root.path().join("coflow.yaml"),
        "schema: schema.cft\ndata: data.cfd\ncodegen:\n  - language: csharp\n    dir: generated/\n",
    )
    .expect("config");
    std::fs::write(
        root.path().join("schema.cft"),
        "table Item { value: int; check Positive { value > 0; } }",
    )
    .expect("schema");
    let path = root.path().join("data.cfd");
    std::fs::write(&path, "a: Item { value: -1 }\nbroken: Item { value: , }\n").expect("data");
    let project = Project::open_schema_only(Some(root.path())).expect("project");
    let mut session = Runtime::new()
        .open_write_session(project.clone())
        .expect("session");
    let expected = session.queries().diagnostics().as_set().clone();
    assert!(!expected.is_empty());
    for _ in 0..2 {
        session = session
            .refresh_context()
            .refresh_data_sources(std::slice::from_ref(&path))
            .expect("refresh");
        assert_eq!(session.queries().diagnostics().as_set(), &expected);
        let stats = session.queries().execution_stats();
        assert_eq!(stats.sources_reloaded, 0);
        assert_eq!(stats.draft_records_collected, 0);
        assert_eq!(stats.records_validated, 0);
        assert_eq!(stats.check_roots_executed, 0);
    }
    let full = Runtime::new()
        .open_read_only_session(project)
        .expect("full load");
    assert_eq!(
        session.queries().diagnostics().as_set(),
        full.queries().diagnostics().as_set()
    );
}

fn initialize_incremental_fixture(root: &std::path::Path) {
    std::fs::create_dir(root.join("data")).expect("data");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\ndata: data/\ncodegen:\n  - language: csharp\n    dir: generated/\n",
    )
    .expect("config");
    std::fs::write(root.join("schema.cft"),
        "table Item { name: string; value: float; parent: &Item; caption: string; check Valid { value >= 0.0; parent.name != \"\"; } }").expect("schema");
}
