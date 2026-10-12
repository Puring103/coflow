#![allow(clippy::expect_used)]

use coflow_runtime::{
    CfdValue, DimensionValueCoordinate, DimensionValueExpectation, DimensionValueState, MutationOp,
    MutationRequest, MutationValue, Project, RecordCoordinate, Runtime,
};
use std::fs;
use tempfile::TempDir;

fn project(schema: &str, base: &str, overlays: &[(&str, &str)]) -> TempDir {
    let dir = tempfile::tempdir().expect("project");
    fs::create_dir(dir.path().join("data")).expect("data");
    fs::write(dir.path().join("schema.cft"), schema).expect("schema");
    fs::write(dir.path().join("data/base.cfd"), base).expect("base");
    for (name, text) in overlays {
        fs::write(dir.path().join("data").join(name), text).expect("overlay");
    }
    fs::write(
        dir.path().join("coflow.yaml"),
        "schema: schema.cft\ndata: data/\ncodegen:\n  - language: csharp\n    dir: generated\n",
    )
    .expect("config");
    dir
}

fn coordinate(ty: &str, key: &str, field: &str, variant: &str) -> DimensionValueCoordinate {
    DimensionValueCoordinate {
        actual_type: ty.try_into().expect("type"),
        record_key: key.try_into().expect("key"),
        field: field.try_into().expect("field"),
        dimension: "language".try_into().expect("dimension"),
        variant: variant.try_into().expect("variant"),
        path: vec![],
    }
}

fn set(coordinate: DimensionValueCoordinate, file: Option<&str>, value: CfdValue) -> MutationOp {
    MutationOp::SetDimensionValue {
        file: file.map(str::to_string),
        coordinate,
        expected: DimensionValueExpectation::Any,
        value: MutationValue::Cfd(value),
    }
}

fn apply(session: &mut coflow_runtime::WriteProjectSession, ops: Vec<MutationOp>) {
    let report = coflow_runtime::commands::apply_project_mutation(
        session,
        MutationRequest {
            stop_on_write_error: true,
            ops,
        },
    )
    .expect("mutation");
    assert!(report.write_ok && report.check_ok, "{report:?}");
}

#[test]
fn missing_overlay_targets_are_diagnosed_without_rewriting_sources() {
    let source = "language[ja] { removed: Item { title: \"旧文本\" } }";
    let dir = project(
        "table Item { @localized title: string; }",
        "kept: Item { title: \"Keep\" }",
        &[("ja.cfd", source)],
    );
    for build in [false, true] {
        let project = Project::open(Some(dir.path())).expect("project");
        let diagnostics = if build {
            Runtime::new()
                .build_project_session(project)
                .expect("build")
                .into_diagnostics()
        } else {
            Runtime::new()
                .open_read_only_session(project)
                .expect("open")
                .into_diagnostics()
        };
        assert!(diagnostics.diagnostics.iter().any(|d| d.code == "REF-001"));
        assert_eq!(
            fs::read_to_string(dir.path().join("data/ja.cfd")).expect("source"),
            source
        );
    }
}

#[test]
fn singleton_overrides_use_selected_files_and_preserve_other_fields_and_comments() {
    let original = "# keep this comment\nlanguage[ja] { UiText: UiText { title: \"元\", description: \"説明\" } }\n";
    let dir = project(
        "singleton UiText { @localized title: string; @localized description: string; }",
        "UiText: UiText { title: \"Title\", description: \"Description\" }",
        &[("ja.cfd", original), ("new.cfd", "# chosen destination\n")],
    );
    let mut session = Runtime::new()
        .open_write_session(Project::open(Some(dir.path())).expect("project"))
        .expect("session");
    apply(
        &mut session,
        vec![set(
            coordinate("UiText", "UiText", "title", "ja"),
            Some("data/new.cfd"),
            CfdValue::String("更新".into()),
        )],
    );
    let text = fs::read_to_string(dir.path().join("data/ja.cfd")).expect("source");
    assert!(text.starts_with("# keep this comment"));
    assert!(text.contains("description: \"説明\""));
    assert!(text.contains("title: \"更新\""));
    assert_eq!(
        fs::read_to_string(dir.path().join("data/new.cfd")).expect("destination"),
        "# chosen destination\n"
    );
    apply(
        &mut session,
        vec![set(
            coordinate("UiText", "UiText", "title", "fr"),
            Some("data/new.cfd"),
            CfdValue::String("Titre".into()),
        )],
    );
    let text = fs::read_to_string(dir.path().join("data/new.cfd")).expect("destination");
    assert!(text.contains("language[fr]"));
    assert!(text.contains("UiText: UiText"));
    assert!(text.contains("title: \"Titre\""));
    apply(
        &mut session,
        vec![MutationOp::ClearDimensionValue {
            coordinate: coordinate("UiText", "UiText", "title", "ja"),
            expected: DimensionValueExpectation::Any,
        }],
    );
    let text = fs::read_to_string(dir.path().join("data/ja.cfd")).expect("source");
    assert!(text.contains("description: \"説明\""));
    assert!(!text.contains("title:"));
    assert!(Runtime::new()
        .open_read_only_session(Project::open(Some(dir.path())).expect("project"))
        .expect("reload")
        .into_diagnostics()
        .is_empty());
}

#[test]
fn new_overrides_require_an_explicit_destination() {
    let dir = project(
        "table Item { @localized title: string; }",
        "one: Item { title: \"Title\" }",
        &[("ja.cfd", "language[ja] {}")],
    );
    let mut session = Runtime::new()
        .open_write_session(Project::open(Some(dir.path())).expect("project"))
        .expect("session");
    let report = coflow_runtime::commands::apply_project_mutation(
        &mut session,
        MutationRequest {
            stop_on_write_error: true,
            ops: vec![set(
                coordinate("Item", "one", "title", "ja"),
                None,
                CfdValue::String("名".into()),
            )],
        },
    )
    .expect("report");
    assert!(!report.write_ok);
    assert_eq!(
        fs::read_to_string(dir.path().join("data/ja.cfd")).expect("source"),
        "language[ja] {}"
    );
}

#[test]
fn inherited_record_rename_and_delete_update_all_overlay_files() {
    let dir = project("table Base { @localized name: string; @dimension(\"platform\") hint: string; } table Child: Base {}",
        "one: Child { name: \"Name\", hint: \"Hint\" }",
        &[("ja.cfd", "language[ja] { one: Base { name: \"名\" } }\nlanguage[en] { one: Child { name: \"Name\" } }"),
          ("mobile.cfd", "platform[mobile] { one: Child { hint: \"Tap\" } }")]);
    let mut session = Runtime::new()
        .open_write_session(Project::open(Some(dir.path())).expect("project"))
        .expect("session");
    apply(
        &mut session,
        vec![MutationOp::RenameRecord {
            record: RecordCoordinate::try_new("Child", "one").expect("coordinate"),
            file: None,
            new_key: "renamed".into(),
        }],
    );
    for file in ["ja.cfd", "mobile.cfd"] {
        let text = fs::read_to_string(dir.path().join("data").join(file)).expect("source");
        assert!(text.contains("renamed:"), "{text}");
        assert!(!text.contains("one:"), "{text}");
    }
    apply(
        &mut session,
        vec![MutationOp::DeleteRecord {
            record: RecordCoordinate::try_new("Child", "renamed").expect("coordinate"),
            file: None,
        }],
    );
    for file in ["ja.cfd", "mobile.cfd"] {
        assert!(!fs::read_to_string(dir.path().join("data").join(file))
            .expect("source")
            .contains("renamed:"));
    }
}

#[test]
fn optional_none_is_written_as_an_explicit_override() {
    let dir = project(
        "table Item { @localized note: Option<string>; }",
        "one: Item { note: Some(\"base\") }",
        &[("ja.cfd", "language[ja] {}")],
    );
    let mut session = Runtime::new()
        .open_write_session(Project::open(Some(dir.path())).expect("project"))
        .expect("session");
    let coordinate = coordinate("Item", "one", "note", "ja");
    apply(
        &mut session,
        vec![set(
            coordinate.clone(),
            Some("data/ja.cfd"),
            CfdValue::OptionNone,
        )],
    );
    assert_eq!(
        session
            .queries()
            .dimension_value(&coordinate)
            .expect("value")
            .state,
        DimensionValueState::Value(CfdValue::OptionNone)
    );
    assert!(fs::read_to_string(dir.path().join("data/ja.cfd"))
        .expect("source")
        .contains("note: None"));
}

#[test]
fn record_rename_updates_references_in_each_original_overlay_file() {
    let dir = project(
        "table Item { name: string; } table Holder { @localized target: &Item; }",
        "one: Item { name: \"One\" }\ntwo: Item { name: \"Two\" }\nholder: Holder { target: &two }",
        &[
            (
                "ja.cfd",
                "# Japanese\nlanguage[ja] { holder: Holder { target: &one } }",
            ),
            (
                "fr.cfd",
                "# French\nlanguage[fr] { holder: Holder { target: &one } }",
            ),
        ],
    );
    let mut session = Runtime::new()
        .open_write_session(Project::open(Some(dir.path())).expect("project"))
        .expect("session");
    apply(
        &mut session,
        vec![MutationOp::RenameRecord {
            record: RecordCoordinate::try_new("Item", "one").expect("record"),
            file: None,
            new_key: "renamed".into(),
        }],
    );
    for (file, comment) in [("ja.cfd", "# Japanese"), ("fr.cfd", "# French")] {
        let text = fs::read_to_string(dir.path().join("data").join(file)).expect("source");
        assert!(text.starts_with(comment));
        assert!(text.contains("target: &renamed"), "{text}");
    }
    assert!(Runtime::new()
        .open_read_only_session(Project::open(Some(dir.path())).expect("project"))
        .expect("reload")
        .into_diagnostics()
        .is_empty());
}
