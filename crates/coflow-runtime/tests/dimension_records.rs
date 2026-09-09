#![allow(clippy::expect_used)]

use coflow_runtime::{
    DimensionValueCoordinate, DimensionValueExpectation, MutationOp, MutationRequest,
    MutationValue, Project, Runtime,
};
use std::fs;

#[test]
fn generated_singleton_records_keep_field_types_through_mutation_and_reload() {
    let dir = tempfile::tempdir().expect("project");
    fs::write(
        dir.path().join("schema.cft"),
        "@singleton type UiText { @localized title: string; @localized description: string; }",
    )
    .expect("schema");
    fs::write(
        dir.path().join("base.cfd"),
        "ui: UiText { title: \"Title\", description: \"Description\" }",
    )
    .expect("base");
    fs::write(dir.path().join("coflow.yaml"),
        "schema: schema.cft\ndata: base.cfd\ndimensions:\n  language:\n    variants: [en, zh]\n    out_dir: dimensions/language\ncodegen:\n  - language: csharp\n    dir: generated\n").expect("config");
    let runtime = Runtime::new();
    runtime
        .build_project_session(Project::open(Some(dir.path())).expect("project"))
        .expect("generate dimensions");
    let mut session = runtime
        .open_write_session(Project::open(Some(dir.path())).expect("project"))
        .expect("session");
    let path = dir.path().join("dimensions/language/UiText.cfd");
    for field in ["title", "description"] {
        let body = fs::read_to_string(&path).expect("generated file");
        assert!(
            body.contains(&format!("{field}: __coflow_language_UiText_{field}")),
            "{body}"
        );
        let report = coflow_runtime::commands::apply_project_mutation(
            &mut session,
            MutationRequest {
                stop_on_write_error: true,
                ops: vec![MutationOp::SetDimensionValue {
                    coordinate: DimensionValueCoordinate {
                        actual_type: "UiText".try_into().expect("type"),
                        record_key: "ui".try_into().expect("key"),
                        field: field.try_into().expect("field"),
                        dimension: "language".try_into().expect("dimension"),
                        variant: "zh".try_into().expect("variant"),
                        path: vec![],
                    },
                    expected: DimensionValueExpectation::Any,
                    value: MutationValue::Cfd(coflow_runtime::CfdValue::String(format!(
                        "translated {field}"
                    ))),
                }],
            },
        )
        .expect("mutation");
        assert!(report.write_ok && report.check_ok, "{report:?}");
    }
    let body = fs::read_to_string(&path).expect("updated file");
    for field in ["title", "description"] {
        assert!(
            body.contains(&format!("{field}: __coflow_language_UiText_{field}")),
            "{body}"
        );
        assert!(
            body.contains(&format!("zh: \"translated {field}\"")),
            "{body}"
        );
    }
    let reloaded = Runtime::new()
        .open_read_only_session(Project::open(Some(dir.path())).expect("project"))
        .expect("reload");
    let diagnostics = reloaded.into_diagnostics();
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn old_and_wrong_field_dimension_types_are_rejected() {
    for record_type in [
        "Item",
        "Item_titleVariants",
        "__coflow_language_Item_description",
        "__coflow_platform_Item_title",
    ] {
        let dir = tempfile::tempdir().expect("project");
        fs::write(
            dir.path().join("schema.cft"),
            "type Item { @localized title: string; }",
        )
        .expect("schema");
        fs::write(
            dir.path().join("base.cfd"),
            "one: Item { title: \"Title\" }",
        )
        .expect("base");
        fs::create_dir_all(dir.path().join("dimensions/language")).expect("directory");
        fs::write(
            dir.path().join("dimensions/language/Item_title.cfd"),
            format!("one: {record_type} {{ zh: \"translation\" }}"),
        )
        .expect("dimension");
        fs::write(dir.path().join("coflow.yaml"),
            "schema: schema.cft\ndata: base.cfd\ndimensions:\n  language:\n    variants: [zh]\n    out_dir: dimensions/language\ncodegen:\n  - language: csharp\n    dir: generated\n").expect("config");
        let diagnostics = match Runtime::new()
            .open_read_only_session(Project::open(Some(dir.path())).expect("project"))
        {
            Ok(session) => session.into_diagnostics(),
            Err(diagnostics) => diagnostics,
        };
        assert!(
            diagnostics
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "CFD-DIMENSION-TYPE"),
            "{record_type}: {diagnostics:?}"
        );
        let original = fs::read_to_string(dir.path().join("dimensions/language/Item_title.cfd"))
            .expect("original");
        let diagnostics = match Runtime::new()
            .build_project_session(Project::open(Some(dir.path())).expect("project"))
        {
            Ok(session) => session.into_diagnostics(),
            Err(diagnostics) => diagnostics,
        };
        assert!(
            diagnostics.diagnostics.iter().any(|diagnostic| diagnostic
                .message
                .contains("expected `__coflow_language_Item_title`")),
            "build accepted {record_type}: {diagnostics:?}"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("dimensions/language/Item_title.cfd"))
                .expect("unchanged file"),
            original
        );
    }
}

#[test]
fn inherited_dimension_records_keep_declaring_type_when_renamed() {
    let dir = tempfile::tempdir().expect("project");
    fs::write(dir.path().join("schema.cft"),
        "type Base { @localized name: string; @dimension(\"platform\") hint: string; } type Child : Base {}")
        .expect("schema");
    fs::write(
        dir.path().join("base.cfd"),
        "one: Child { name: \"Name\", hint: \"Hint\" }",
    )
    .expect("base");
    fs::write(dir.path().join("coflow.yaml"),
        "schema: schema.cft\ndata: base.cfd\ndimensions:\n  language:\n    variants: [zh]\n    out_dir: dimensions/language\n  platform:\n    variants: [mobile]\n    out_dir: dimensions/platform\ncodegen:\n  - language: csharp\n    dir: generated\n").expect("config");
    let runtime = Runtime::new();
    runtime
        .build_project_session(Project::open(Some(dir.path())).expect("project"))
        .expect("generate dimensions");
    let mut session = runtime
        .open_write_session(Project::open(Some(dir.path())).expect("project"))
        .expect("session");
    let report = coflow_runtime::commands::apply_project_mutation(
        &mut session,
        MutationRequest {
            stop_on_write_error: true,
            ops: vec![MutationOp::RenameRecord {
                record: coflow_runtime::RecordCoordinate::try_new("Child", "one")
                    .expect("coordinate"),
                file: None,
                new_key: "renamed".into(),
            }],
        },
    )
    .expect("rename");
    assert!(report.write_ok && report.check_ok, "{report:?}");
    for (dimension, field) in [("language", "name"), ("platform", "hint")] {
        let body = fs::read_to_string(
            dir.path()
                .join(format!("dimensions/{dimension}/Base_{field}.cfd")),
        )
        .expect("dimension file");
        assert!(
            body.contains(&format!("renamed: __coflow_{dimension}_Base_{field}")),
            "{body}"
        );
        assert!(!body.contains("one:"), "{body}");
    }
    let reloaded = Runtime::new()
        .open_read_only_session(Project::open(Some(dir.path())).expect("project"))
        .expect("reload");
    let diagnostics = reloaded.into_diagnostics();
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}
