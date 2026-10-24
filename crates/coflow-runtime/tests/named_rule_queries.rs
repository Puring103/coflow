#![allow(clippy::unwrap_used, clippy::expect_used)]

use coflow_runtime::{
    automation::{execute, Arguments, InputSnapshot, Response, Status},
    Project,
};
use std::{fs, path::Path};

fn fixture(schema: &str, data: &str) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("data")).unwrap();
    fs::create_dir(root.path().join("dimensions")).unwrap();
    fs::write(root.path().join("coflow.yaml"), "schema: schema.cft\ndata: data/\ndimensions:\n  mode:\n    dir: dimensions/\ncodegen:\n  - language: csharp\n    dir: generated/\n").unwrap();
    fs::write(root.path().join("schema.cft"), schema).unwrap();
    fs::write(root.path().join("data/items.cfd"), data).unwrap();
    root
}

fn run(root: &Path, command: &str, options: &[(&str, &str)]) -> Response {
    let project = Project::open_schema_only(Some(root)).unwrap();
    let snapshot = InputSnapshot::capture(&project).unwrap();
    let args = Arguments(
        options
            .iter()
            .map(|(key, value)| (key.to_string(), vec![value.to_string()]))
            .collect(),
    );
    execute(command, &project, &snapshot, &args, None, root).unwrap()
}

#[test]
fn named_rule_queries_group_roots_and_include_all_dependencies_and_messages() {
    let root = fixture(
        r#"
const LIMIT: int = 7;
table Item {
    value: int;
    numbers: [int] = [];
    check Bounds: f"limit {LIMIT}" {
        value > 0;
        when true: f"when {LIMIT}" {
            all number in numbers: f"all {LIMIT}" { number > 0; }
        }
        value < LIMIT;
    }
}
"#,
        "one: Item { value: 3 }\n",
    );
    let list = run(root.path(), "rule.list", &[]);
    assert_eq!(list.status, Status::Success, "{list:?}");
    let rules = list.result["rules"].as_array().unwrap();
    assert_eq!(rules.len(), 1);
    let rule = &rules[0];
    assert_eq!(rule["id"], "Item::Bounds");
    assert_eq!(rule["messages"].as_array().unwrap().len(), 3);
    for field in ["value", "numbers"] {
        assert!(rule["dependencies"]
            .as_array()
            .unwrap()
            .iter()
            .any(|dependency| dependency["field"] == field));
        let refs = run(
            root.path(),
            "schema.refs",
            &[("symbol", &format!("Item.{field}"))],
        );
        assert_eq!(refs.status, Status::Success, "{refs:?}");
        assert_eq!(
            refs.result["references"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|item| item["kind"] == "rule_expression")
                .count(),
            1,
            "{refs:?}"
        );
    }
    let inspect = run(root.path(), "rule.inspect", &[("rule", "Item::Bounds"), ("details", "true")]);
    assert_eq!(inspect.result, list.result);
    let symbols = run(root.path(), "schema.symbols", &[("kind", "rule")]);
    assert_eq!(symbols.result["items"].as_array().unwrap().len(), 1);
    let refs = run(root.path(), "schema.refs", &[("symbol", "Item::Bounds")]);
    assert_eq!(refs.result["references"].as_array().unwrap().len(), 1);

    let refs = run(root.path(), "schema.refs", &[("symbol", "LIMIT")]);
    assert_eq!(refs.status, Status::Success, "{refs:?}");
    let uses: Vec<_> = refs.result["references"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["kind"] == "rule_expression")
        .collect();
    assert_eq!(uses.len(), 4, "{refs:?}");
    let locations: std::collections::BTreeSet<_> = uses
        .iter()
        .map(|item| item["location"].to_string())
        .collect();
    assert_eq!(locations.len(), 4);

    let checked = run(root.path(), "check", &[("rule", "Item::Bounds"), ("details", "true")]);
    assert_eq!(checked.status, Status::Success, "{checked:?}");
    assert_eq!(
        checked.result["tasks"].as_array().unwrap().len(),
        3,
        "{checked:?}"
    );
    assert_eq!(checked.result["results"].as_array().unwrap().len(), 1);
}

#[test]
fn data_dimension_rule_results_have_complete_object_coordinates() {
    let root = fixture(
        r#"
data Reward { count: int; check Positive { count > 0; } }
table Item { @dimension("mode") rewards: [Reward]; }
"#,
        "one: Item { rewards: [{ count: 1 }] }\n",
    );
    fs::write(
        root.path().join("dimensions/hard.cfd"),
        "mode[hard] { one: Item { rewards: [{ count: -1 }] } }\n",
    )
    .unwrap();
    let checked = run(
        root.path(),
        "check",
        &[("rules", "true"), ("explain", "true"), ("details", "true")],
    );
    assert_eq!(checked.status, Status::Invalid, "{checked:?}");
    let results = checked.result["results"].as_array().unwrap();
    let failed = results
        .iter()
        .find(|result| result["state"] == "failed")
        .unwrap();
    assert_eq!(failed["target"]["field_path"], "rewards[0]");
    assert_eq!(failed["target"]["dimension"]["field"], "rewards");
    assert_eq!(failed["target"]["dimension"]["dimension"], "mode");
    assert_eq!(failed["target"]["dimension"]["variant"], "hard");
    assert_eq!(failed["rule"], "Reward::Positive");
    let encoded = serde_json::to_value(checked).unwrap();
    assert_eq!(
        encoded["diagnostics"][0]["explanation"]["actual_value"]["value"],
        "-1"
    );
    assert_eq!(encoded["diagnostics"][0]["field_path"], "rewards[0].count");
}

#[test]
fn explanations_read_the_evaluated_variant_and_preserve_target_base_after_references() {
    let root = fixture(r#"
table Reward { @dimension("mode") count: int; }
table Item {
    @dimension("mode") reward: &Reward;
    @dimension("mode") count: int;
    check Direct { count.for("hard") > 0; }
    check Target { reward.for("hard").count > 0; }
}
"#, "good: Reward { count: 1 }\nbad: Reward { count: -2 }\none: Item { reward: &good, count: 3 }\n");
    fs::write(
        root.path().join("dimensions/hard.cfd"),
        "mode[hard] { one: Item { reward: &bad, count: -1 } bad: Reward { count: 9 } }\n",
    )
    .unwrap();
    for (rule, actual, record) in [("Item::Direct", "-1", "one"), ("Item::Target", "-2", "bad")] {
        let checked = run(root.path(), "check", &[("rule", rule), ("explain", "true")]);
        assert_eq!(checked.status, Status::Invalid, "{checked:?}");
        let encoded = serde_json::to_value(checked).unwrap();
        let diagnostic = &encoded["diagnostics"][0];
        assert_eq!(
            diagnostic["explanation"]["actual_value"]["value"], actual,
            "{encoded}"
        );
        assert_eq!(diagnostic["record"]["id"], record);
        assert!(diagnostic["contexts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|context| context["dimension"] == "mode" && context["variant"] == "hard"));
    }
}

#[test]
fn data_checks_do_not_shadow_a_constant_named_id_without_an_id_field() {
    let root = fixture("const id: int = 7; data Value { value: int; check Bound { value < id; } } table Item { item: Value; }", "one: Item { item: { value: 3 } }\n");
    let refs = run(root.path(), "schema.refs", &[("symbol", "id")]);
    assert_eq!(refs.status, Status::Success, "{refs:?}");
    assert_eq!(
        refs.result["references"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|item| item["kind"] == "rule_expression")
            .count(),
        1
    );
}

#[test]
fn anonymous_rules_can_be_listed_inspected_and_selected_individually() {
    let root = fixture(r#"
        table Item { value: int; check { value > 0; } check { value < 10; } }
        check { records(Item).len() == 1; }
        check: "project message" { records(Item).len() > 0; }
    "#, "one: Item { value: 3 }\n");
    let list = run(root.path(), "rule.list", &[]);
    assert_eq!(list.status, Status::Success, "{list:?}");
    let rules = list.result["rules"].as_array().unwrap();
    assert_eq!(rules.len(), 4);
    let ids: std::collections::BTreeSet<_> = rules.iter().map(|rule| rule["id"].as_str().unwrap()).collect();
    assert_eq!(ids.len(), 4);
    for id in ids {
        let inspect = run(root.path(), "rule.inspect", &[("rule", id)]);
        assert_eq!(inspect.status, Status::Success, "{inspect:?}");
        let checked = run(root.path(), "check", &[("rule", id), ("details", "true")]);
        assert_eq!(checked.status, Status::Success, "{checked:?}");
        assert_eq!(checked.result["tasks"].as_array().unwrap().len(), 1);
    }
}
