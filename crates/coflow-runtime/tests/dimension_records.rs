#![allow(clippy::expect_used)]

use coflow_runtime::{
    DimensionValueCoordinate, DimensionValueExpectation, DimensionValueState, MutationOp,
    MutationRequest, MutationValue, Project, Runtime,
};
use std::fs;

fn project(schema: &str, data: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("project");
    fs::write(dir.path().join("schema.cft"), schema).expect("schema");
    fs::write(dir.path().join("data.cfd"), data).expect("data");
    fs::write(
        dir.path().join("coflow.yaml"),
        "schema: schema.cft\ndata: data.cfd\ncodegen:\n  - language: csharp\n    dir: generated\n",
    )
    .expect("config");
    dir
}

fn coordinate(variant: &str) -> DimensionValueCoordinate {
    DimensionValueCoordinate {
        actual_type: "Item".try_into().expect("type"),
        record_key: "sword".try_into().expect("key"),
        field: "title".try_into().expect("field"),
        dimension: "language".try_into().expect("dimension"),
        variant: variant.try_into().expect("variant"),
        path: Vec::new(),
    }
}

#[test]
fn variants_are_discovered_from_inline_dimension_values() {
    let dir = project(
        "table Item { @localized title: string; }",
        "sword: Item { title: dimension { default: \"Sword\", zh: \"剑\", ja: \"剣\" } }",
    );
    let session = Runtime::new()
        .open_read_only_session(Project::open(Some(dir.path())).expect("project"))
        .expect("session");
    let info = session
        .queries()
        .dimensions()
        .into_iter()
        .next()
        .expect("dimension");
    assert_eq!(info.variants, ["ja", "zh"]);
    let value = session
        .queries()
        .dimension_value(&coordinate("zh"))
        .expect("value");
    assert_eq!(
        value.state,
        DimensionValueState::Value(coflow_runtime::CfdValue::String("剑".into()))
    );
}

#[test]
fn dimension_mutation_updates_and_clears_the_business_record_block() {
    let dir = project(
        "table Item { @localized title: string; }",
        "sword: Item {\n  title: dimension {\n    default: \"Sword\",\n    zh: \"剑\",\n  },\n}\n",
    );
    let runtime = Runtime::new();
    let mut session = runtime
        .open_write_session(Project::open(Some(dir.path())).expect("project"))
        .expect("session");
    let report = coflow_runtime::commands::apply_project_mutation(
        &mut session,
        MutationRequest {
            stop_on_write_error: true,
            ops: vec![
                MutationOp::SetDimensionValue {
                    coordinate: coordinate("ja"),
                    expected: DimensionValueExpectation::Missing,
                    value: MutationValue::Cfd(coflow_runtime::CfdValue::String("剣".into())),
                },
                MutationOp::ClearDimensionValue {
                    coordinate: coordinate("zh"),
                    expected: DimensionValueExpectation::Any,
                },
            ],
        },
    )
    .expect("mutation");
    assert!(report.write_ok && report.check_ok, "{report:?}");
    let source = fs::read_to_string(dir.path().join("data.cfd")).expect("source");
    assert!(source.contains("ja: \"剣\""), "{source}");
    assert!(!source.contains("zh:"), "{source}");
    assert!(source.contains("default: \"Sword\""), "{source}");
}

#[test]
fn dimension_fields_require_a_default_and_reject_plain_values() {
    for data in [
        "sword: Item { title: dimension { zh: \"剑\" } }",
        "sword: Item { title: \"Sword\" }",
    ] {
        let dir = project("table Item { @localized title: string; }", data);
        let diagnostics = Runtime::new()
            .open_read_only_session(Project::open(Some(dir.path())).expect("project"))
            .map_or_else(|errors| errors, |session| session.into_diagnostics());
        assert!(!diagnostics.is_empty(), "{data}");
    }
}
