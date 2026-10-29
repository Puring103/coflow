#![allow(clippy::expect_used)]

use coflow_runtime::{
    CfdValue, DimensionValueCoordinate, DimensionValueExpectation, DimensionValueState, MutationOp,
    MutationRequest, MutationValue, Project, RecordCoordinate, Runtime,
};
use std::fmt::Write as _;
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
        record_id: key.try_into().expect("key"),
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
fn new_destination_is_loaded_before_publication_and_registered_for_explicit_inputs() {
    for explicit in [false, true] {
        let dir = project(
            "table Item { @localized title: string; @localized description: string; }",
            "one: Item { title: \"One\", description: \"First\" }\ntwo: Item { title: \"Two\", description: \"Second\" }",
            &[],
        );
        let config_path = dir.path().join("coflow.yaml");
        if explicit {
            fs::write(&config_path, "schema: schema.cft\ndata: [data/base.cfd]\ncodegen: [{ language: csharp, dir: generated }]\n").expect("config");
        }
        let original = fs::read(&config_path).expect("config");
        let mut session = Runtime::new()
            .open_write_session(Project::open(Some(dir.path())).expect("project"))
            .expect("session");
        apply(
            &mut session,
            vec![
                set(
                    coordinate("Item", "one", "title", "ja"),
                    Some("data/base.language.cfd"),
                    CfdValue::String("一".into()),
                ),
                set(
                    coordinate("Item", "two", "description", "en"),
                    Some("data/base.language.cfd"),
                    CfdValue::String("Second item".into()),
                ),
            ],
        );
        assert_eq!(
            session
                .queries()
                .dimension_value(&coordinate("Item", "one", "title", "ja"))
                .expect("value")
                .state,
            DimensionValueState::Value(CfdValue::String("一".into()))
        );
        let text =
            fs::read_to_string(dir.path().join("data/base.language.cfd")).expect("destination");
        assert!(
            text.contains("language[ja]") && text.contains("language[en]"),
            "{text}"
        );
        let project = Project::open(Some(dir.path())).expect("project");
        assert_eq!(project.data_paths().len(), if explicit { 2 } else { 1 });
        if !explicit {
            assert_eq!(fs::read(&config_path).expect("config"), original);
        }
        let reopened = Runtime::new()
            .open_read_only_session(project)
            .expect("reload");
        assert!(reopened.queries().diagnostics().is_empty());
        assert_eq!(
            reopened
                .queries()
                .dimension_value(&coordinate("Item", "two", "description", "en"))
                .expect("value")
                .state,
            DimensionValueState::Value(CfdValue::String("Second item".into()))
        );
    }
}

#[test]
fn new_destination_and_config_are_not_published_when_a_later_operation_fails() {
    let dir = project(
        "table Item { @localized title: string; }",
        "one: Item { title: \"One\" }",
        &[],
    );
    let config_path = dir.path().join("coflow.yaml");
    fs::write(&config_path, "schema: schema.cft\ndata: [data/base.cfd]\ncodegen: [{ language: csharp, dir: generated }]\n").expect("config");
    let original = fs::read(&config_path).expect("config");
    let mut session = Runtime::new()
        .open_write_session(Project::open(Some(dir.path())).expect("project"))
        .expect("session");
    let report = coflow_runtime::commands::apply_project_mutation(
        &mut session,
        MutationRequest {
            stop_on_write_error: true,
            ops: vec![
                set(
                    coordinate("Item", "one", "title", "ja"),
                    Some("data/base.language.cfd"),
                    CfdValue::String("一".into()),
                ),
                set(
                    coordinate("Item", "one", "title", "en"),
                    Some("data/invalid.txt"),
                    CfdValue::String("One".into()),
                ),
            ],
        },
    )
    .expect("report");
    assert!(!report.write_ok && !report.generation_changed, "{report:?}");
    assert!(!dir.path().join("data/base.language.cfd").exists());
    assert_eq!(fs::read(&config_path).expect("config"), original);
    assert_eq!(
        session
            .queries()
            .dimension_value(&coordinate("Item", "one", "title", "ja"))
            .expect("missing")
            .state,
        DimensionValueState::Missing
    );
}

#[test]
fn new_destination_does_not_overwrite_an_external_config_change() {
    let dir = project(
        "table Item { @localized title: string; }",
        "one: Item { title: \"One\" }",
        &[],
    );
    let config_path = dir.path().join("coflow.yaml");
    fs::write(&config_path, "schema: schema.cft\ndata: [data/base.cfd]\ncodegen: [{ language: csharp, dir: generated }]\n").expect("config");
    let mut session = Runtime::new()
        .open_write_session(Project::open(Some(dir.path())).expect("project"))
        .expect("session");
    let external = "schema: schema.cft\ndata: [data/base.cfd]\ncodegen: [{ language: csharp, dir: generated }]\n# external edit\n";
    fs::write(&config_path, external).expect("external config");
    let report = coflow_runtime::commands::apply_project_mutation(
        &mut session,
        MutationRequest {
            stop_on_write_error: true,
            ops: vec![set(
                coordinate("Item", "one", "title", "ja"),
                Some("data/base.language.cfd"),
                CfdValue::String("一".into()),
            )],
        },
    )
    .expect("report");
    assert!(!report.write_ok && !report.generation_changed, "{report:?}");
    assert!(!dir.path().join("data/base.language.cfd").exists());
    assert_eq!(fs::read_to_string(&config_path).expect("config"), external);
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
            new_id: "renamed".into(),
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
            new_id: "renamed".into(),
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

#[test]
fn one_overlay_change_rebuilds_only_its_dependency_closure_and_preserves_origins() {
    let mut base = String::new();
    for i in 0..40 {
        writeln!(base, "r{i}: Child {{ name: \"base {i}\", parent: &r{}, caption: f\"{{self.parent.name}}\", note: Some(\"base\") }}", (i + 1) % 40).expect("fixture source");
    }
    let entries = (0..40)
        .map(|i| format!("r{i}: Base {{ name: \"日本 {i}\", note: Some(\"note {i}\") }}\n"))
        .collect::<Vec<_>>();
    let original = format!("language[ja] {{\n{}}}\n", entries.concat());
    let dir = project(
        "table Base { @localized name: string; parent: &Base; @localized caption: string; @localized note: Option<string>; } table Child: Base {}",
        &base,
        &[("ja.cfd", &original)],
    );
    let project = Project::open(Some(dir.path())).expect("project");
    let mut session = Runtime::new()
        .open_write_session(project.clone())
        .expect("session");
    let path = dir.path().join("data/ja.cfd");
    let changed = original.replace("日本 0", "変更された長い名前😀");
    let reordered = format!(
        "# only source order changes\nlanguage[ja] {{\n{}}}\n",
        entries.iter().rev().cloned().collect::<String>()
    )
    .replace("日本 0", "変更された長い名前😀");
    let removed = reordered.replace(&entries[1], "");
    let explicit_none = removed.replace("Some(\"note 2\")", "None");
    let alias = explicit_none.replace("r3: Base", "r3: Child");
    let duplicate = format!("{alias}\nlanguage[ja] {{ r4: Base {{ name: \"duplicate\" }} }}\n");
    for (step, source) in [
        changed,
        reordered,
        removed,
        explicit_none,
        alias.clone(),
        duplicate,
        alias,
    ]
    .into_iter()
    .enumerate()
    {
        // 保存上一代的完整记录；更新位置或覆盖值不能污染旧的不可变快照。
        let old = session
            .queries()
            .record_views_in_file("data/base.cfd")
            .map(|v| v.record.clone())
            .collect::<Vec<_>>();
        let snapshot = session.refresh_context();
        fs::write(&path, source).expect("edit overlays");
        let next = snapshot
            .refresh_data_sources(std::slice::from_ref(&path))
            .expect("refresh");
        let full = Runtime::new()
            .open_read_only_session(project.clone())
            .expect("reload");
        assert_eq!(
            next.queries().diagnostics().as_set(),
            full.queries().diagnostics().as_set(),
            "diagnostics at {step}"
        );
        let expected = full
            .queries()
            .record_views_in_file("data/base.cfd")
            .collect::<Vec<_>>();
        for (actual, expected) in next
            .queries()
            .record_views_in_file("data/base.cfd")
            .zip(expected)
        {
            assert_eq!(
                actual.record, expected.record,
                "all values, dimensions and origins at {step}"
            );
            assert_eq!(
                next.queries().record_references(&actual.coordinate),
                full.queries().record_references(&expected.coordinate)
            );
        }
        assert_eq!(
            session
                .queries()
                .record_views_in_file("data/base.cfd")
                .map(|v| v.record.clone())
                .collect::<Vec<_>>(),
            old,
            "old generation at {step}"
        );
        if step == 0 {
            assert_eq!(next.queries().execution_stats().records_materialized, 2);
        }
        if step == 1 {
            assert_eq!(next.queries().execution_stats().records_materialized, 0);
        }
        session = next;
    }
}
