#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "实际工程快照以逐条密集查询为独立对照。"
)]

use super::*;
use std::fs;

const SCHEMA: &str = r#"
data Details { target: &Item; text: string = f"{self.target.name}"; }
table Item {
    @localized name: string = "Default";
    value: int = 1;
    parent: &Item;
    label: string = f"{self.parent.name}";
    details: Option<Details> = None;
    @dimension("platform") note: string = "Base";
}
"#;
const DATA: &str = "a: Item { name: \"A\", value: 1, parent: &a }\nb: Item { parent: &a, details: Some(Details { target: &a }) }\n";

fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("language")).unwrap();
    fs::create_dir(root.path().join("platform")).unwrap();
    fs::write(root.path().join("coflow.yaml"), "schema: schema.cft\ndata: data.cfd\ncodegen: [{language: csharp, dir: generated}]\ndimensions:\n  language: {dir: language}\n  platform: {dir: platform}\n").unwrap();
    fs::write(root.path().join("schema.cft"), SCHEMA).unwrap();
    fs::write(root.path().join("data.cfd"), DATA).unwrap();
    fs::write(root.path().join("language/ja.cfd"), "language[ja] {}\n").unwrap();
    fs::write(root.path().join("language/fr.cfd"), "language[fr] {}\n").unwrap();
    fs::write(
        root.path().join("platform/mobile.cfd"),
        "platform[mobile] {}\n",
    )
    .unwrap();
    root
}

fn dense_fields(
    project: &Project,
    session: &ReadOnlyProjectSession,
    model: &crate::CfdDataModel,
    selector: &Selector,
    sources: &mut super::super::query::RecordSources,
) -> BTreeMap<FieldCoordinate, DiffFieldState> {
    let mut fields = BTreeMap::new();
    // 独立遍历全部记录、全部字段；不能经稀疏采集入口或以 Arc 身份跳过任何记录。
    for (_, record) in model.records() {
        let result = super::super::query::record_result_with_sources(
            project, session, record, selector, sources, false,
        )
        .unwrap();
        for (field, source) in result.source.unwrap() {
            fields.insert(
                (
                    record.actual_type().into(),
                    record.id().into(),
                    field.clone(),
                    None,
                    None,
                ),
                DiffFieldState {
                    assigned: source.assigned,
                    origin: source.origin,
                    expression: source.expression,
                    effective: source.effective,
                    rendered: result.rendered.get(&field).cloned(),
                },
            );
        }
    }
    fields
}

fn capture(root: &Path) -> (SemanticSnapshot, SemanticSnapshot) {
    let project = Project::open_schema_only(Some(root)).unwrap();
    let schema = Runtime::open_schema_session(project.clone()).unwrap();
    let session = Runtime::new()
        .open_read_only_session(project.clone())
        .unwrap();
    assert!(!session.queries().has_diagnostics());
    let sparse = SemanticSnapshot::capture_from_schema(&schema, Some(&session), |path| {
        Ok(crate::project_path(root, path))
    })
    .unwrap();
    assert!(sparse.available);
    let selector = Selector {
        view: "both".into(),
        ..Default::default()
    };
    let mut sources = super::super::query::RecordSources::default();
    let mut dense = SemanticSnapshot {
        available: true,
        schema_available: sparse.schema_available,
        // schema/record身份未参与稀疏存储；对照只共享这部分已单独验证的元数据。
        schema: sparse.schema.clone(),
        records: sparse.records.clone(),
        fields: dense_fields(&project, &session, session.model(), &selector, &mut sources),
        ..Default::default()
    };
    for dimension in session.queries().dimensions() {
        for variant in dimension.variants {
            let selector = Selector {
                view: "both".into(),
                dimensions: BTreeMap::from([(dimension.name.clone(), variant.clone())]),
                ..Default::default()
            };
            let model =
                super::super::query::project_dimensions(&session, &selector.dimensions).unwrap();
            dense.contexts.insert(
                (dimension.name.clone(), variant),
                dense_fields(&project, &session, &model, &selector, &mut sources),
            );
        }
    }
    assert_eq!(sparse.fields, dense.fields);
    assert_eq!(
        sparse.contexts.keys().collect::<Vec<_>>(),
        dense.contexts.keys().collect::<Vec<_>>()
    );
    for (context, fields) in &dense.contexts {
        assert!(sparse.contexts[context]
            .keys()
            .all(|field| fields.contains_key(field)));
        for (field, state) in fields {
            assert_eq!(
                sparse.field(field, Some(context)),
                Some(state),
                "{context:?} {field:?}"
            );
        }
    }
    (sparse, dense)
}

fn compare_oracle(
    before: &(SemanticSnapshot, SemanticSnapshot),
    after: &(SemanticSnapshot, SemanticSnapshot),
) -> SemanticDifference {
    let actual = compare(&before.0, &after.0);
    let expected = compare(&before.1, &after.1);
    assert_eq!(
        serde_json::to_value(&actual).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    actual
}

#[test]
fn empty_context_addition_and_removal_report_every_effective_field() {
    let root = fixture();
    let before = capture(root.path());
    fs::write(root.path().join("language/es.cfd"), "language[es] {}\n").unwrap();
    let added = capture(root.path());
    let difference = compare_oracle(&before, &added);
    assert_eq!(difference.changed_dimension_fields, before.0.fields.len());
    assert!(difference
        .dimensions
        .iter()
        .all(|field| field.variant.as_deref() == Some("es")
            && field.before.is_none()
            && field.after.is_some()));
    fs::remove_file(root.path().join("language/es.cfd")).unwrap();
    let removed = capture(root.path());
    let difference = compare_oracle(&added, &removed);
    assert_eq!(difference.changed_dimension_fields, before.0.fields.len());
    assert!(difference
        .dimensions
        .iter()
        .all(|field| field.before.is_some() && field.after.is_none()));
}

#[test]
fn explicit_override_equal_to_base_preserves_assignment_origin_changes() {
    let root = fixture();
    let before = capture(root.path());
    fs::write(
        root.path().join("language/ja.cfd"),
        "language[ja] { a: Item { name: \"A\" } }\n",
    )
    .unwrap();
    let added = capture(root.path());
    let difference = compare_oracle(&before, &added);
    assert_eq!(difference.dimensions.len(), 1);
    let field = &difference.dimensions[0];
    assert!(field.assignment_changed && !field.effective_changed && !field.rendered_changed);
    assert_eq!(field.after.as_ref().unwrap().origin, "dimension_override");
    fs::write(root.path().join("language/ja.cfd"), "language[ja] {}\n").unwrap();
    let removed = capture(root.path());
    let difference = compare_oracle(&added, &removed);
    assert_eq!(difference.dimensions.len(), 1);
    assert_eq!(
        difference.dimensions[0].after.as_ref().unwrap().origin,
        "explicit_base"
    );
}

#[test]
fn indirect_reference_and_nested_template_renders_are_captured_without_host_overrides() {
    let root = fixture();
    let before = capture(root.path());
    fs::write(
        root.path().join("language/ja.cfd"),
        "language[ja] { a: Item { name: \"甲😀\" } }\n",
    )
    .unwrap();
    let after = capture(root.path());
    let difference = compare_oracle(&before, &after);
    assert!(difference.fields.is_empty());
    for name in ["label", "details"] {
        let field = difference
            .dimensions
            .iter()
            .find(|field| {
                field.id == "b" && field.field == name && field.variant.as_deref() == Some("ja")
            })
            .unwrap();
        assert!(field.rendered_changed && !field.assignment_changed);
    }
}

#[test]
fn schema_default_change_propagates_to_fallback_contexts_but_preserves_override_shielding() {
    let root = fixture();
    fs::write(
        root.path().join("language/ja.cfd"),
        "language[ja] { b: Item { name: \"Shield\" } }\n",
    )
    .unwrap();
    let before = capture(root.path());
    fs::write(
        root.path().join("schema.cft"),
        SCHEMA.replace("\"Default\"", "\"New default\""),
    )
    .unwrap();
    let after = capture(root.path());
    let difference = compare_oracle(&before, &after);
    assert!(difference
        .fields
        .iter()
        .any(|field| field.id == "b" && field.field == "name" && field.effective_changed));
    assert!(!difference.dimensions.iter().any(|field| field.id == "b"
        && field.field == "name"
        && field.variant.as_deref() == Some("ja")));
    assert!(difference.dimensions.iter().any(|field| field.id == "b"
        && field.field == "name"
        && field.variant.as_deref() == Some("fr")
        && field.effective_changed));
}

#[test]
fn simultaneous_record_field_and_context_membership_changes_match_dense_capture() {
    let root = fixture();
    let before = capture(root.path());
    fs::write(
        root.path().join("schema.cft"),
        SCHEMA.replace("value: int = 1;", "extra: int = 5;"),
    )
    .unwrap();
    fs::write(
        root.path().join("data.cfd"),
        "a: Item { name: \"A\", parent: &a }\nc: Item { parent: &a }\n",
    )
    .unwrap();
    fs::remove_file(root.path().join("language/fr.cfd")).unwrap();
    fs::write(root.path().join("language/es.cfd"), "language[es] {}\n").unwrap();
    let after = capture(root.path());
    let difference = compare_oracle(&before, &after);
    assert!(difference
        .records
        .iter()
        .any(|record| record.id == "b" && record.change == "deleted"));
    assert!(difference
        .records
        .iter()
        .any(|record| record.id == "c" && record.change == "added"));
    assert!(difference
        .dimensions
        .iter()
        .any(|field| field.field == "value" && field.after.is_none()));
    assert!(difference
        .dimensions
        .iter()
        .any(|field| field.field == "extra" && field.before.is_none()));
}

#[test]
fn trivia_only_assignment_changes_keep_raw_sources_when_indirect_render_also_changes() {
    let root = fixture();
    fs::write(root.path().join("data.cfd"), "a: Item { name: \"A\", parent: &a, details: Some(Details { target: &b }) }\nb: Item { parent: &a }\n").unwrap();
    let before = capture(root.path());
    fs::write(
        root.path().join("schema.cft"),
        SCHEMA.replace("\"Default\"", "\"New default\""),
    )
    .unwrap();
    fs::write(root.path().join("data.cfd"), "a: Item { name: \"A\", parent: &a, details: Some( Details { # comment\n target: &b } ) }\nb: Item { parent: &a }\n").unwrap();
    let after = capture(root.path());
    let difference = compare_oracle(&before, &after);
    let field = difference
        .fields
        .iter()
        .find(|field| field.id == "a" && field.field == "details")
        .unwrap();
    assert!(!field.assignment_changed && field.rendered_changed);
    assert_ne!(
        field.before.as_ref().unwrap().expression,
        field.after.as_ref().unwrap().expression
    );
    assert!(difference.dimensions.iter().any(|field| field.id == "a"
        && field.field == "details"
        && field.rendered_changed
        && !field.assignment_changed));
}
