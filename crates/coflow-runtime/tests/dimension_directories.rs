#![allow(clippy::expect_used)]

use coflow_runtime::{
    CfdValue, DimensionValueCoordinate, DimensionValueExpectation, DimensionValueState, MutationOp,
    MutationRequest, MutationValue, Project, Runtime,
};
use std::fs;

fn fixture(dimensions: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("fixture");
    fs::create_dir_all(dir.path().join("data/equipment")).expect("data");
    fs::create_dir_all(dir.path().join("other/equipment")).expect("other");
    fs::write(
        dir.path().join("schema.cft"),
        "table Item { @localized name: string; }",
    )
    .expect("schema");
    fs::write(
        dir.path().join("data/equipment/items.cfd"),
        "one: Item { name: \"One\" }",
    )
    .expect("data");
    fs::write(
        dir.path().join("other/equipment/items.cfd"),
        "two: Item { name: \"Two\" }",
    )
    .expect("other");
    fs::write(dir.path().join("coflow.yaml"), format!(
        "schema: schema.cft\ndata: [data/, other/]\ncodegen: [{{language: csharp, dir: generated}}]\ndimensions:\n{dimensions}"
    )).expect("config");
    dir
}
fn coordinate(key: &str) -> DimensionValueCoordinate {
    DimensionValueCoordinate {
        actual_type: "Item".try_into().expect("type"),
        record_id: key.try_into().expect("key"),
        field: "name".try_into().expect("field"),
        dimension: "language".try_into().expect("dimension"),
        variant: "ja".try_into().expect("variant"),
        path: vec![],
    }
}

#[test]
fn configured_dimension_directory_is_created_on_write_and_loaded_without_data_registration() {
    let dir = fixture("  language: {dir: dimensions/language}\n");
    let config = fs::read(dir.path().join("coflow.yaml")).expect("config");
    let project = Project::open(Some(dir.path())).expect("project");
    assert!(project.tracks_path(
        &dir.path()
            .join("dimensions/language/data/equipment/items.cfd")
    ));
    let mut session = Runtime::new().open_write_session(project).expect("session");
    assert!(!dir.path().join("dimensions").exists());
    let mut paths = Vec::new();
    for (key, base) in [("one", "data"), ("two", "other")] {
        let path = session
            .queries()
            .dimension_destination("Item", key, "language")
            .expect("destination");
        assert_eq!(
            path,
            format!("dimensions/language/{base}/equipment/items.cfd")
        );
        paths.push(path.clone());
        let report = coflow_runtime::commands::apply_project_mutation(
            &mut session,
            MutationRequest {
                stop_on_write_error: true,
                ops: vec![MutationOp::SetDimensionValue {
                    file: Some(path),
                    coordinate: coordinate(key),
                    expected: DimensionValueExpectation::Missing,
                    value: MutationValue::Cfd(CfdValue::String(format!("ja-{key}"))),
                }],
            },
        )
        .expect("write");
        assert!(report.write_ok && report.check_ok, "{report:?}");
    }
    assert_eq!(
        fs::read(dir.path().join("coflow.yaml")).expect("config"),
        config
    );
    let reopened = Runtime::new()
        .open_read_only_session(Project::open(Some(dir.path())).expect("project"))
        .expect("reopen");
    assert!(reopened.queries().diagnostics().is_empty());
    let mut files = reopened.queries().dimensions()[0].files.clone();
    files.sort();
    assert_eq!(files, paths);
    for key in ["one", "two"] {
        assert_eq!(
            reopened
                .queries()
                .dimension_value(&coordinate(key))
                .expect("value")
                .state,
            DimensionValueState::Value(CfdValue::String(format!("ja-{key}")))
        );
    }
}

#[test]
fn directory_in_both_data_and_dimensions_is_loaded_once() {
    let dir = fixture("  language: {dir: data/localization}\n");
    fs::create_dir_all(dir.path().join("data/localization")).expect("directory");
    fs::write(
        dir.path().join("data/localization/ja.cfd"),
        "language[ja] { one: Item { name: \"一\" } }",
    )
    .expect("overlay");
    let session = Runtime::new()
        .open_read_only_session(Project::open(Some(dir.path())).expect("project"))
        .expect("session");
    assert!(session.queries().diagnostics().is_empty());
    assert_eq!(
        session.queries().dimensions()[0].files,
        ["data/localization/ja.cfd"]
    );
    assert_eq!(session.queries().source_file_count(), 3);
}

#[test]
fn invalid_dimension_directories_are_rejected() {
    for config in [
        "  language: {dir: ''}\n",
        "  language: {dir: schema.cft}\n",
        "  language: {dir: dimensions}\n  region: {dir: dimensions/region}\n",
    ] {
        let dir = fixture(config);
        assert!(Project::open(Some(dir.path())).is_err(), "{config}");
    }
}
