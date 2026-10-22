#![allow(clippy::unwrap_used, clippy::expect_used)]

use coflow_runtime::{
    automation::{execute, Arguments, InputSnapshot, Response, Status},
    Project,
};
use serde_json::{json, Value};
use std::{fs, path::Path};

fn fixture(records: usize) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("coflow.yaml"),
        "schema: schema.cft\ndata: data.cfd\ncodegen: [{language: csharp, dir: generated}]\n",
    )
    .unwrap();
    fs::write(root.path().join("schema.cft"), "table Item { value: int; note: Option<string> = None; tags: [string] = []; check Positive { value > 0: \"must be positive\"; } }\n").unwrap();
    fs::write(
        root.path().join("data.cfd"),
        (0..records)
            .map(|index| format!("item_{index}: Item {{ value: -1 }}\n"))
            .collect::<String>(),
    )
    .unwrap();
    root
}

fn run(root: &Path, command: &str, options: &[(&str, &str)], input: Option<Value>) -> Response {
    let project = Project::open_schema_only(Some(root)).unwrap();
    let snapshot = InputSnapshot::capture(&project).unwrap();
    let args = Arguments(
        options
            .iter()
            .map(|(name, value)| (name.to_string(), vec![value.to_string()]))
            .collect(),
    );
    let input = input.map(|value| value.to_string());
    match execute(command, &project, &snapshot, &args, input.as_deref(), root) {
        Ok(response) => response,
        Err(failure) => {
            let mut response = Response::new(command);
            response.fail(failure);
            response
        }
    }
}

#[test]
fn bounded_diagnostics_keep_failure_status_counts_coordinates_and_repairs() {
    let root = fixture(80);
    let mut response = run(root.path(), "check", &[("rules", "true")], None);
    response.prepare_output(false);
    let wire = serde_json::to_value(&response).unwrap();
    assert_eq!(response.status, Status::Invalid);
    assert_eq!(response.exit_code(), 1);
    assert_eq!(
        wire["diagnostics_summary"],
        json!({"total":80,"returned":50,"truncated":true})
    );
    assert_eq!(
        wire["result"]["rule_counts"],
        json!({"total":80,"failed":80})
    );
    assert!(wire["result"].get("tasks").is_none());
    assert!(wire["result"].get("results").is_none());
    for diagnostic in wire["diagnostics"].as_array().unwrap() {
        assert_eq!(diagnostic["record"]["type"], "Item");
        assert_eq!(diagnostic["field_path"], "value");
        assert!(diagnostic["location"]["path"].is_string());
        assert!(diagnostic["help"]
            .as_str()
            .is_some_and(|help| !help.is_empty()));
        assert!(diagnostic.get("explanation").is_none());
    }
    // 再次准备输出不能把总数误改为已经截断的条数。
    response.prepare_output(false);
    assert_eq!(
        serde_json::to_value(response).unwrap()["diagnostics_summary"],
        wire["diagnostics_summary"]
    );

    let mut full = run(
        root.path(),
        "check",
        &[("rules", "true"), ("details", "true")],
        None,
    );
    full.prepare_output(true);
    assert_eq!(full.diagnostics.len(), 80);
    assert_eq!(full.result["tasks"].as_array().unwrap().len(), 80);
    assert_eq!(full.result["results"].as_array().unwrap().len(), 80);
    assert!(!full.diagnostics_summary.unwrap().truncated);

    let mut limited = run(
        root.path(),
        "check",
        &[("rules", "true"), ("limit", "3")],
        None,
    );
    limited.prepare_output(false);
    assert_eq!(limited.diagnostics.len(), 3);
    assert_eq!(limited.diagnostics_summary.unwrap().total, 80);
}

#[test]
fn compact_output_preserves_nullable_values_empty_collections_and_sources() {
    let root = fixture(1);
    let mut response = run(
        root.path(),
        "data.get",
        &[("type", "Item"), ("id", "item_0"), ("view", "both")],
        None,
    );
    let before = response.result.clone();
    response.prepare_output(false);
    assert_eq!(response.result, before);
    assert_eq!(response.result["fields"]["note"], json!({"kind":"none"}));
    assert_eq!(
        response.result["fields"]["tags"],
        json!({"kind":"array","items":[]})
    );
    assert_eq!(
        response.result["source"]["note"]["origin"],
        "schema_default"
    );
    assert!(response
        .revision
        .as_ref()
        .is_some_and(|revision| revision.len() == 64));
}

#[test]
fn incomplete_data_scope_reports_counts_without_repeating_every_diagnostic() {
    let root = fixture(120);
    fs::write(
        root.path().join("data.cfd"),
        (0..120)
            .map(|index| format!("item_{index}: Item {{ value: \"invalid\" }}\n"))
            .collect::<String>(),
    )
    .unwrap();
    let mut response = run(root.path(), "data.list", &[], None);
    response.prepare_output(false);
    let wire = serde_json::to_value(&response).unwrap();
    assert_eq!(wire["data_scope"]["complete"], false);
    assert_eq!(wire["data_scope"]["selected_file_count"], 1);
    assert_eq!(wire["data_scope"]["incomplete_file_count"], 1);
    assert_eq!(wire["data_scope"]["rejected_record_count"], 0);
    assert_eq!(wire["data_scope"]["details_omitted"], true);
    for key in ["selected_files", "incomplete_files", "rejected_records"] {
        assert!(wire["data_scope"].get(key).is_none());
    }
    assert_eq!(wire["diagnostics_summary"]["total"], 120);
    assert_eq!(wire["diagnostics_summary"]["returned"], 50);
    response.prepare_output(false);
    assert_eq!(
        serde_json::to_value(&response).unwrap()["data_scope"],
        wire["data_scope"]
    );

    let mut full = run(root.path(), "data.list", &[("details", "true")], None);
    full.prepare_output(true);
    let full = serde_json::to_value(full).unwrap();
    assert_eq!(full["data_scope"]["details_omitted"], false);
    assert_eq!(
        full["data_scope"]["incomplete_files"][0]["reasons"]
            .as_array()
            .unwrap()
            .len(),
        120
    );
    assert!(full["data_scope"].get("rejected_records").is_none());
}

#[test]
fn compact_transaction_retains_failed_operation_and_precondition_without_publishing() {
    let root = fixture(2);
    let before = fs::read(root.path().join("data.cfd")).unwrap();
    let mut response = run(
        root.path(),
        "apply",
        &[],
        Some(json!({
            "protocol_version":1,"kind":"operations","ops":[
                {"op":"set_field","record":{"type":"Item","id":"item_0"},"path":[{"kind":"field","name":"value"}],"value":{"kind":"expression","source":"2"}},
                {"op":"set_field","record":{"type":"Item","id":"item_1"},"path":[{"kind":"field","name":"value"}],"expected":{"kind":"expression","source":"10"},"value":{"kind":"expression","source":"3"}},
                {"op":"delete_record","record":{"type":"Item","id":"item_0"}}
            ]
        })),
    );
    response.prepare_output(false);
    assert_eq!(response.status, Status::Conflict);
    assert_eq!(response.result["written"], false);
    assert_eq!(
        response.result["operation_counts"],
        json!({"total":3,"success":1,"failed":1,"not_executed":1})
    );
    assert_eq!(response.result["operations"].as_array().unwrap().len(), 1);
    assert_eq!(response.result["operations"][0]["index"], 1);
    assert_eq!(
        response.result["operations"][0]["operation"]["record"]["id"],
        "item_1"
    );
    assert_eq!(response.result["precondition"]["expected"]["value"], "10");
    assert_eq!(response.result["precondition"]["actual"]["value"], "-1");
    assert_eq!(fs::read(root.path().join("data.cfd")).unwrap(), before);
    let counts = response.result["operation_counts"].clone();
    response.prepare_output(false);
    assert_eq!(response.result["operation_counts"], counts);
}

#[test]
fn formatting_reports_a_local_patch_and_preserves_preview_source() {
    let root = fixture(120);
    let path = root.path().join("data.cfd");
    let canonical =
        coflow_runtime::automation::format_source(&fs::read_to_string(&path).unwrap(), "cfd")
            .unwrap();
    let source = canonical.replacen("value: -1", "value:-1", 1);
    assert_ne!(source, canonical);
    fs::write(&path, &source).unwrap();
    let response = run(
        root.path(),
        "format",
        &[("file", "data.cfd"), ("dry-run", "true")],
        None,
    );
    assert_eq!(response.status, Status::Success);
    let file = &response.result["files"][0];
    assert_eq!(file["change"], "modified");
    assert!(file["patch"].as_str().unwrap().contains("value:-1"));
    assert!(file.to_string().len() < 1000);
    assert!(file.get("before").is_none());
    assert!(file.get("after").is_none());
    assert_eq!(fs::read_to_string(&path).unwrap(), source);
}
