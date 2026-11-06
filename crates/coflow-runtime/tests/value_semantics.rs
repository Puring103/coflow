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

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "端到端回归同时验证业务字段、记录身份、引用、模板、维度写入与重载一致性。"
)]
fn data_id_writes_preserve_record_identity_references_and_dimension_values() {
    let dir = fixture();
    fs::write(
        dir.path().join("schema.cft"),
        r#"
        data Payload {
            id: int = 7;
            label: string = f"payload {self.id}";
            labels: [string] = [f"list {self.id}"];
            check Positive { id > 0; }
        }
        table Item {
            @dimension("region") payload: Payload = {};
            label: string = f"item {self.id}";
        }
        table Holder { target: &Item; }
        singleton Config { payload: Payload = {}; label: string = f"singleton {self.id}"; }
    "#,
    )
    .expect("id schema");
    fs::write(
        dir.path().join("items.cfd"),
        r"
        one: Item { payload: { id: 7 } }
        two: Item {}
        holder: Holder { target: &Item::one }
        Config: Config {}
        region[eu] { one: Item { payload: { id: 11 } } }
    ",
    )
    .expect("id data");
    let mut session = Runtime::new()
        .open_write_session(Project::open(Some(dir.path())).expect("project"))
        .expect("session");
    assert!(!session.queries().has_diagnostics());
    let path = [
        CfdPathSegment::Field("payload".into()),
        CfdPathSegment::Field("id".into()),
    ];
    let outcome = session
        .write_field("Item", "one", &path, &CfdValue::Int(9))
        .expect("business id write");
    assert!(outcome.renamed.is_none());
    let coordinate = coflow_runtime::DimensionValueCoordinate {
        actual_type: "Item".try_into().expect("type"),
        record_id: "one".try_into().expect("id"),
        field: "payload".try_into().expect("field"),
        dimension: "region".try_into().expect("dimension"),
        variant: "eu".try_into().expect("variant"),
        path: vec![CfdPathSegment::Field("id".into())],
    };
    assert_eq!(
        session
            .queries()
            .dimension_value(&coordinate)
            .expect("override")
            .state,
        coflow_runtime::DimensionValueState::Value(CfdValue::Int(11))
    );
    let report = session.apply_mutation(coflow_runtime::MutationRequest {
        stop_on_write_error: true,
        ops: vec![coflow_runtime::MutationOp::SetDimensionValue {
            file: None,
            coordinate: coordinate.clone(),
            expected: coflow_runtime::DimensionValueExpectation::Any,
            value: coflow_runtime::MutationValue::Cfd(CfdValue::Int(13)),
        }],
    });
    assert!(report.write_ok && report.check_ok, "{report:?}");

    let reloaded = Runtime::new()
        .open_write_session(Project::open(Some(dir.path())).expect("reload project"))
        .expect("reload");
    for current in [&session, &reloaded] {
        let queries = current.queries();
        assert!(!queries.has_diagnostics());
        assert_eq!(
            queries
                .record_view("Item", "one")
                .expect("original identity")
                .record
                .id(),
            "one"
        );
        assert!(queries.record_view("Item", "9").is_none());
        assert_eq!(
            queries.field_value("Item", "one", &path),
            Some(&CfdValue::Int(9))
        );
        assert_eq!(
            queries.field_value("Item", "two", &path),
            Some(&CfdValue::Int(7))
        );
        assert_eq!(
            queries.field_value(
                "Holder",
                "holder",
                &[CfdPathSegment::Field("target".into())]
            ),
            Some(&CfdValue::record_ref("one").expect("reference"))
        );
        for (ty, id, fields, text) in [
            ("Item", "one", vec!["label"], "item one"),
            ("Item", "one", vec!["payload", "label"], "payload 9"),
            ("Config", "Config", vec!["label"], "singleton Config"),
            ("Config", "Config", vec!["payload", "label"], "payload 7"),
        ] {
            let fields = fields
                .into_iter()
                .map(|field| CfdPathSegment::Field(field.into()))
                .collect::<Vec<_>>();
            assert!(
                matches!(queries.field_value(ty, id, &fields), Some(CfdValue::FormattedString(value)) if value.rendered == text)
            );
        }
        assert_eq!(
            queries
                .dimension_value(&coordinate)
                .expect("saved override")
                .state,
            coflow_runtime::DimensionValueState::Value(CfdValue::Int(13))
        );
        let mut label = coordinate.clone();
        label.path = vec![CfdPathSegment::Field("label".into())];
        assert!(
            matches!(queries.dimension_value(&label).expect("dimension template").state,
            coflow_runtime::DimensionValueState::Value(CfdValue::FormattedString(value)) if value.rendered == "payload 13")
        );
    }
}
