#![allow(clippy::expect_used, clippy::panic)]

use coflow_runtime::{CfdPathSegment, CfdValue, Project, RecordCoordinate, Runtime};
use std::fs;

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("fixture");
    fs::write(
        dir.path().join("schema.cft"),
        r"
        data Label { text: string; }
        table Item {
            name: string;
            label: Label;
            required: Option<int>;
            optional: Option<int> = None;
        }
    ",
    )
    .expect("schema");
    fs::write(
        dir.path().join("items.cfd"),
        r#"
        item: Item { name: "Item", label: { text: "Initial" }, required: None, optional: 4 }
    "#,
    )
    .expect("data");
    fs::write(
        dir.path().join("coflow.yaml"),
        "schema: schema.cft\ndata: [items.cfd]\ncodegen: [{language: csharp, dir: generated}]\n",
    )
    .expect("config");
    dir
}

#[test]
fn cell_edits_preserve_explicit_templates_inside_string_objects() {
    let dir = fixture();
    let mut session = Runtime::new()
        .open_write_session(Project::open(Some(dir.path())).expect("project"))
        .expect("session");
    let record = RecordCoordinate::try_new("Item", "item").expect("record");
    let path = [CfdPathSegment::Field("label".into())];
    let value = session
        .parse_cell_text(&record, &path, r#"{ text: f"{&Item::item.name}" }"#)
        .expect("template input");
    let CfdValue::Object(object) = &value else {
        panic!("object")
    };
    assert!(matches!(
        object.field("text"),
        Some(CfdValue::FormattedString(_))
    ));
    session
        .write_field("Item", "item", &path, &value)
        .expect("write template");
    let source = fs::read_to_string(dir.path().join("items.cfd")).expect("source");
    assert!(source.contains("f\"{&Item::item.name}\""));
    let text_path = [
        CfdPathSegment::Field("label".into()),
        CfdPathSegment::Field("text".into()),
    ];
    assert!(
        matches!(session.queries().field_value("Item", "item", &text_path), Some(CfdValue::FormattedString(value)) if value.rendered == "Item")
    );
    let literal = session
        .parse_cell_text(&record, &text_path, r#""f\"{self.name}\"""#)
        .expect("quoted literal");
    assert_eq!(literal, CfdValue::String("f\"{self.name}\"".into()));
}

#[test]
fn unsetting_an_option_only_restores_a_declared_default() {
    let dir = fixture();
    let mut session = Runtime::new()
        .open_write_session(Project::open(Some(dir.path())).expect("project"))
        .expect("session");
    let optional = [CfdPathSegment::Field("optional".into())];
    session
        .unset_field("Item", "item", &optional)
        .expect("unset defaulted field");
    assert_eq!(
        session
            .queries()
            .record_view("Item", "item")
            .expect("item")
            .record
            .field("optional"),
        Some(&CfdValue::OptionNone)
    );
    let required = [CfdPathSegment::Field("required".into())];
    session
        .unset_field("Item", "item", &required)
        .expect("editable missing field");
    assert_eq!(
        session.queries().field_value("Item", "item", &required),
        None
    );
    assert!(session.queries().has_diagnostics());
}
