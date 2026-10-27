#![allow(
    clippy::panic,
    clippy::literal_string_with_formatting_args,
    reason = "测试通过 panic 报告失败，输入花括号属于 CFT 语法。"
)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_json::{json, Value};
use std::fmt::Write as _;
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("data")).unwrap();
    fs::write(
        root.path().join("coflow.yaml"),
        "schema: schema.cft\ndata: data/\ncodegen:\n  - language: csharp\n    dir: generated/\n",
    )
    .unwrap();
    fs::write(
        root.path().join("schema.cft"),
        "table Item { value: int; check Valid { value > 0; } }\n",
    )
    .unwrap();
    fs::write(
        root.path().join("data/items.cfd"),
        "a: Item { value: -1 }\n",
    )
    .unwrap();
    root
}

fn call(root: &Path, arguments: &[&str], stdin: Option<&str>) -> (i32, Value, String) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_coflow"));
    command
        .current_dir(root)
        .args(arguments)
        .arg("--json")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    if let Some(input) = stdin {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
    } else {
        drop(child.stdin.take());
    }
    let output = child.wait_with_output().unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    let text = String::from_utf8(output.stdout).unwrap();
    let result = serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("invalid JSON: {error}; stdout={text}; stderr={stderr}"));
    (output.status.code().unwrap(), result, stderr)
}

#[test]
fn codegen_check_and_preview_agree_for_equivalent_line_endings() {
    let root = fixture();
    let (exit, response, _) = call(root.path(), &["codegen"], None);
    assert_eq!(exit, 0, "{response}");
    let mut paths = vec![root.path().join("generated")];
    let mut files = Vec::new();
    while let Some(path) = paths.pop() {
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                paths.push(path);
            } else if path.extension().is_some_and(|extension| extension == "cs") {
                files.push(path);
            }
        }
    }
    assert!(!files.is_empty());
    for path in &files {
        let text = fs::read_to_string(path).unwrap().replace("\r\n", "\n");
        fs::write(path, text.trim_end_matches('\n').replace('\n', "\r\n")).unwrap();
    }
    let originals = files
        .iter()
        .map(|path| {
            (
                fs::read(path).unwrap(),
                fs::metadata(path).unwrap().modified().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    let (exit, response, _) = call(root.path(), &["codegen", "--check"], None);
    assert_eq!(exit, 0, "{response}");
    assert!(
        !response["result"].to_string().contains("\"changed\":true"),
        "{response}"
    );
    let (exit, response, _) = call(root.path(), &["codegen"], None);
    assert_eq!(exit, 0, "{response}");
    for (path, (bytes, modified)) in files.iter().zip(originals) {
        assert_eq!(fs::read(path).unwrap(), bytes);
        assert_eq!(fs::metadata(path).unwrap().modified().unwrap(), modified);
    }
}

#[test]
fn opt_in_cli_logs_correlate_parameter_errors_without_logging_arguments() {
    let root = fixture();
    let child = Command::new(env!("CARGO_BIN_EXE_coflow"))
        .current_dir(root.path())
        .env("COFLOW_LOG", "1")
        .args(["data", "list", "--private-sentinel-do-not-log", "--json"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let operation_prefix = format!("{}-", child.id());
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["diagnostics"][0]["code"], "CLI-REQUEST");
    let directory = coflow_observability::directory("cli").unwrap();
    let events = (0..5)
        .filter_map(|index| {
            fs::read_to_string(directory.join(format!("events.{index}.jsonl"))).ok()
        })
        .flat_map(|text| {
            text.lines()
                .filter_map(|line| serde_json::from_str::<Value>(line).ok())
                .collect::<Vec<_>>()
        })
        .filter(|event| {
            event["span"]["operation_id"]
                .as_str()
                .is_some_and(|id| id.starts_with(&operation_prefix))
        })
        .collect::<Vec<_>>();
    let finished = events
        .iter()
        .find(|event| event["fields"]["event"] == "command_finished")
        .expect("correlated final event");
    assert_eq!(finished["fields"]["exit_code"], 2);
    assert_eq!(finished["fields"]["diagnostic_count"], 1);
    assert!(finished["fields"]["codes"]
        .as_str()
        .unwrap()
        .contains("CLI-REQUEST"));
    assert!(!serde_json::to_string(&events)
        .unwrap()
        .contains("private-sentinel"));
}

#[test]
fn project_inspection_contract_preserves_configuration_shape_and_generator_options() {
    let root = fixture();
    let (exit, scalar, _) = call(root.path(), &["project", "inspect"], None);
    assert_eq!(exit, 0);
    assert_eq!(scalar["result"]["config"]["schema"], "schema.cft");
    assert!(scalar["result"]["config"].get("dimensions").is_none());
    fs::write(root.path().join("coflow.yaml"), "schema: [schema.cft]\ndata: data/\ncodegen:\n  - language: csharp\n    dir: generated/\n    namespace: Game.Config\ndimensions:\n  language:\n    dir: translations/\n    display_name: Language\n").unwrap();
    fs::write(root.path().join("schema.cft"), "broken schema").unwrap();
    let (exit, response, _) = call(root.path(), &["project", "inspect"], None);
    assert_eq!(exit, 0, "{response}");
    assert_eq!(
        response["result"]["config"]["schema"],
        json!(["schema.cft"])
    );
    assert_eq!(response["result"]["config"]["data"], json!(["data/"]));
    assert_eq!(
        response["result"]["config"]["codegen"][0]["namespace"],
        "Game.Config"
    );
    assert_eq!(
        response["result"]["config"]["dimensions"]["language"]["display_name"],
        "Language"
    );
    assert_eq!(response["result"]["config_path"], "coflow.yaml");
    assert!(response["result"]["input_roots"]
        .as_array()
        .unwrap()
        .iter()
        .all(|path| !path.as_str().unwrap().contains('\\')));
    let (exit, description, _) = call(
        root.path(),
        &["describe", "project.inspect", "--schema", "output"],
        None,
    );
    assert_eq!(exit, 0);
    let definitions = &description["result"]["definition"]["output_schema"]["definitions"];
    assert_eq!(
        definitions["SchemaInputs"]["anyOf"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        definitions["ProjectConfiguration"]["properties"]["data"]["type"],
        "array"
    );
    assert_eq!(
        definitions["ProjectDimension"]["properties"]["display_name"]["type"],
        json!(["string", "null"])
    );
    assert!(definitions["CodegenTarget"]["properties"]
        .get("options")
        .is_none());
    // 可省略的空维度配置不能被自描述 Schema 标为必填。
    for field in definitions["ProjectConfiguration"]["required"]
        .as_array()
        .unwrap()
    {
        let field = field.as_str().unwrap();
        assert!(
            scalar["result"]["config"].get(field).is_some(),
            "required config field {field} was omitted"
        );
    }
    assert!(!definitions["ProjectConfiguration"]["required"]
        .as_array()
        .unwrap()
        .contains(&json!("dimensions")));
}

#[test]
fn project_files_contract_describes_roles_and_reports_unavailable_metadata() {
    let root = fixture();
    let (exit, description, _) = call(
        root.path(),
        &["describe", "project.files", "--schema", "output"],
        None,
    );
    assert_eq!(exit, 0);
    let definitions = &description["result"]["definition"]["output_schema"]["definitions"];
    assert_eq!(
        definitions["ProjectFiles"]["properties"]["files"]["type"],
        "array"
    );
    assert_eq!(
        definitions["DataFileRole"]["enum"],
        json!(["base", "dimension", "mixed", "empty"])
    );
    assert_eq!(
        definitions["ProjectFile"]["properties"]["metadata_complete"]["type"],
        "boolean"
    );
    fs::write(root.path().join("data/items.cfd"), "a: Item { value: [").unwrap();
    let (exit, response, _) = call(root.path(), &["project", "files", "--kind", "data"], None);
    assert_eq!(exit, 1, "{response}");
    assert_eq!(response["status"], "partial");
    assert_eq!(
        response["diagnostics"][0]["location"]["path"],
        "data/items.cfd"
    );
    assert!(response["diagnostics"][0]["location"]["range"]["start"]["line"].is_number());
    assert_eq!(response["result"]["files"][0]["metadata_complete"], false);
    assert!(response["result"]["files"][0]["data"].is_null());
}

#[test]
fn source_read_contract_exposes_metadata_without_internal_snapshot_fields() {
    let root = fixture();
    let (exit, description, _) = call(
        root.path(),
        &["describe", "source.read", "--schema", "output"],
        None,
    );
    assert_eq!(exit, 0);
    let definitions = &description["result"]["definition"]["output_schema"]["definitions"];
    let required = definitions["SourceRead"]["required"].as_array().unwrap();
    for field in ["file", "source", "start_line", "end_line"] {
        assert!(required.contains(&json!(field)));
    }
    let properties = definitions["InputFile"]["properties"].as_object().unwrap();
    for field in ["path", "kind", "inputs", "fingerprint"] {
        assert!(properties.contains_key(field));
    }
    assert!(!properties.contains_key("absolute"));
    assert!(!properties.contains_key("contents"));
    fs::write(
        root.path().join("schema.cft"),
        "broken schema\r\nsecond line\r\n",
    )
    .unwrap();
    let (exit, response, _) = call(
        root.path(),
        &[
            "source",
            "read",
            "--file",
            "schema.cft",
            "--start-line",
            "1",
            "--end-line",
            "2",
        ],
        None,
    );
    assert_eq!(exit, 0, "{response}");
    assert_eq!(response["result"]["source"], "second line\r\n");
    assert_eq!(response["result"]["file"]["path"], "schema.cft");
    assert_eq!(response["result"]["file"]["kind"], "schema");
    assert_eq!(
        response["result"]["file"]["fingerprint"]
            .as_str()
            .unwrap()
            .len(),
        64
    );
}

#[test]
fn describe_reference_query_exposes_edges_paths_locations_and_truncation() {
    let root = tempfile::tempdir().unwrap();
    let (exit, response, _) = call(
        root.path(),
        &["describe", "data.refs", "--schema", "output"],
        None,
    );
    assert_eq!(exit, 0, "{response}");
    let definitions = &response["result"]["definition"]["output_schema"]["definitions"];
    let query = &definitions["ReferenceQuery"];
    for field in ["items", "total", "truncated", "depth_truncated"] {
        assert!(
            query["required"]
                .as_array()
                .unwrap()
                .contains(&json!(field)),
            "{field}"
        );
    }
    let edge = &definitions["ReferenceEdge"]["properties"];
    for field in ["host", "target", "path", "dimension", "location"] {
        assert!(edge.get(field).is_some(), "{field}");
    }
    assert_eq!(
        definitions["ReferencePath"]["properties"]["segments"]["type"],
        "array"
    );
    assert_eq!(
        definitions["ReferencePathSegment"]["oneOf"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        definitions["ReferenceDimension"]["properties"]["variant"]["type"],
        "string"
    );
    assert_eq!(
        definitions["CommandLocation"]["properties"]["path"]["type"],
        "string"
    );
}

#[test]
fn invalid_arguments_return_json_before_project_loading() {
    let root = tempfile::tempdir().unwrap();
    for arguments in [
        vec!["data", "set"],
        vec!["data", "list", "--limit", "oops"],
        vec!["codegen", "--check", "--dry-run"],
        vec!["check", "--rules", "--rule", "Item#0"],
        vec!["format", "--stdin"],
        vec!["data", "list", "--dimension", "language"],
    ] {
        let (code, response, stderr) = call(root.path(), &arguments, None);
        assert_eq!(code, 2, "{response}");
        assert_eq!(response["status"], "error");
        assert_eq!(response["error_category"], "request");
        assert!(!response["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("no coflow"));
        assert!(stderr.is_empty(), "{stderr}");
    }
}

#[test]
fn validation_and_request_errors_have_different_exit_codes() {
    let root = fixture();
    let (code, response, _) = call(root.path(), &["check"], None);
    assert_eq!(code, 0);
    assert_eq!(response["result"]["rules"], "not_executed");
    let (code, response, _) = call(root.path(), &["check", "--rules", "--limit", "1"], None);
    assert_eq!(code, 1);
    assert_eq!(response["status"], "invalid");
    assert_eq!(response["error_category"], Value::Null);
    assert_eq!(
        response["diagnostics"][0]["record"],
        json!({"type":"Item","id":"a"})
    );
    assert_eq!(
        response["diagnostics"][0]["location"]["path"],
        "data/items.cfd"
    );
    assert!(response.get("snapshot").is_none());
}

#[test]
fn missing_targets_and_unreadable_request_files_return_complete_results() {
    let root = fixture();
    let (code, response, _) = call(
        root.path(),
        &["data", "get", "--type", "Item", "--id", "missing"],
        None,
    );
    assert_eq!(code, 1);
    assert_eq!(response["status"], "invalid");
    let (code, response, _) = call(root.path(), &["apply", "--input", "missing.json"], None);
    assert_eq!(code, 4);
    assert_eq!(response["error_category"], "execution");
}

#[test]
fn describe_reports_canonical_names_and_required_value_groups() {
    let root = tempfile::tempdir().unwrap();
    let (code, response, _) = call(root.path(), &["describe", "data.set"], None);
    assert_eq!(code, 0);
    assert_eq!(response["result"]["definition"]["command"], "data.set");
    assert_eq!(response["result"]["definition"]["read_only"], false);
    assert_eq!(response["result"]["definition"]["writes"], true);
    assert!(response["result"]["definition"]["examples"][0]
        .as_str()
        .unwrap()
        .contains("data set"));
    let groups = response["result"]["definition"]["groups"]
        .as_array()
        .unwrap();
    assert!(groups.iter().any(|group| group["name"] == "value"
        && group["required"] == true
        && group["multiple"] == false));
    let parameters = response["result"]["definition"]["parameters"]
        .as_array()
        .unwrap();
    assert!(parameters
        .iter()
        .any(|argument| argument["name"] == "path" && argument["required"] == true));
    let (code, response, _) = call(root.path(), &["describe", "data.get"], None);
    assert_eq!(code, 0);
    assert_eq!(response["result"]["definition"]["read_only"], true);
    assert_eq!(response["result"]["definition"]["writes"], false);
}

#[test]
fn describe_distinguishes_record_fields_change_sets_and_source_carriers() {
    let root = tempfile::tempdir().unwrap();
    let (code, response, _) = call(
        root.path(),
        &["describe", "data.create", "--schema", "input"],
        None,
    );
    assert_eq!(code, 0);
    let schema = &response["result"]["definition"]["input_schema"];
    assert_eq!(schema["type"], "object");
    assert!(schema["additionalProperties"].is_object());
    assert!(schema["definitions"]["InputValue"].is_object());
    let (_, response, _) = call(
        root.path(),
        &["describe", "apply", "--schema", "input"],
        None,
    );
    let variants = response["result"]["definition"]["input_schema"]["oneOf"]
        .as_array()
        .unwrap();
    assert!(variants
        .iter()
        .any(|variant| variant["properties"]["kind"]["enum"] == json!(["operations"])));
    for variant in variants {
        assert_eq!(
            variant["properties"]["protocol_version"]["minimum"].as_f64(),
            Some(1.0)
        );
        assert_eq!(
            variant["properties"]["protocol_version"]["maximum"].as_f64(),
            Some(1.0)
        );
    }
    let (_, response, _) = call(
        root.path(),
        &["describe", "source.write", "--schema", "input"],
        None,
    );
    assert!(response["result"]["definition"]["input_schema"].is_null());
    assert_eq!(
        response["result"]["definition"]["input_formats"],
        json!([{"parameter":"input","format":"source_text"}])
    );
    let (_, response, _) = call(root.path(), &["describe", "data.get"], None);
    assert!(response["result"]["definition"]["input_schema"].is_null());
    assert!(response["result"]["definition"]
        .get("input_formats")
        .is_none());
}

#[test]
fn explain_uses_real_diagnostic_codes_and_describes_its_typed_result() {
    let root = tempfile::tempdir().unwrap();
    for code in [
        "CFT-SCHEMA-006",
        "DATA-006",
        "REF-001",
        "CHECK-020",
        "PROJECT-CONFIG-NOT-FOUND",
        "CLI-PUBLISH",
    ] {
        let (exit, response, _) = call(root.path(), &["explain", code], None);
        assert_eq!(exit, 0, "{response}");
        assert_eq!(response["result"]["code"], code);
        for field in ["meaning", "trigger", "repair"] {
            assert_ne!(response["result"][field].as_str().unwrap(), "");
        }
    }
    let (exit, _, _) = call(root.path(), &["explain", "CFT"], None);
    assert_eq!(exit, 1);
    let (exit, response, _) = call(
        root.path(),
        &["describe", "explain", "--schema", "output"],
        None,
    );
    assert_eq!(exit, 0);
    let schema =
        &response["result"]["definition"]["output_schema"]["definitions"]["DiagnosticExplanation"];
    assert_eq!(schema["properties"]["meaning"]["type"], "string");
    assert!(schema["required"]
        .as_array()
        .unwrap()
        .contains(&json!("trigger")));
    let required = response["result"]["definition"]["output_schema"]["required"]
        .as_array()
        .unwrap();
    for field in [
        "protocol_version",
        "command",
        "status",
        "error_category",
        "project",
        "result",
        "diagnostics",
    ] {
        assert!(required.contains(&json!(field)), "{field}");
    }
}

#[test]
fn stdin_format_check_reports_changes_without_returning_source() {
    let root = tempfile::tempdir().unwrap();
    let (code, response, _) = call(
        root.path(),
        &["format", "--stdin", "--language", "cfd", "--check"],
        Some("a: Item{value:1}"),
    );
    assert_eq!(code, 1);
    assert_eq!(response["result"]["changed"], true);
    assert_eq!(response["result"]["written"], false);
    assert_eq!(response["result"]["source"], Value::Null);
}

#[test]
fn default_terminal_output_is_text() {
    let root = fixture();
    let output = Command::new(env!("CARGO_BIN_EXE_coflow"))
        .current_dir(root.path())
        .args(["data", "get", "--type", "Item", "--id", "a"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.starts_with("data.get:"));
    assert!(text.contains("id: a"));
    assert!(serde_json::from_str::<Value>(&text).is_err());
}

#[test]
fn named_rules_are_listed_once_and_inline_diagnostics_keep_record_and_path() {
    let root = fixture();
    fs::write(
        root.path().join("schema.cft"),
        r#"
data Reward {
  count: int;
  check Positive: "奖励规则" { count > 0: "数量必须为正"; count < 100; }
}
table Item {
  rewards: [Reward];
  check HasReward { rewards.len() > 0; }
}
"#,
    )
    .unwrap();
    fs::write(
        root.path().join("data/items.cfd"),
        "a: Item { rewards: [{ count: -1 }] }\n",
    )
    .unwrap();
    let (exit, listed, _) = call(root.path(), &["rule", "list"], None);
    assert_eq!(exit, 0, "{listed}");
    let rules = listed["result"]["rules"].as_array().unwrap();
    assert_eq!(rules.len(), 2, "{listed}");
    let reward = rules
        .iter()
        .find(|rule| rule["id"] == "Reward::Positive")
        .unwrap();
    assert!(reward["expression"]
        .as_str()
        .unwrap()
        .contains("count < 100"));
    assert!(reward["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|text| text.as_str().unwrap().contains("奖励规则")));
    let (exit, checked, _) = call(
        root.path(),
        &[
            "check",
            "--rule",
            "Reward::Positive",
            "--explain",
            "--details",
        ],
        None,
    );
    assert_eq!(exit, 1, "{checked}");
    assert_eq!(checked["result"]["complete"], true);
    assert_eq!(checked["result"]["results"].as_array().unwrap().len(), 1);
    assert_eq!(checked["result"]["results"][0]["rule"], "Reward::Positive");
    assert_eq!(checked["result"]["results"][0]["state"], "failed");
    assert_eq!(
        checked["diagnostics"][0]["record"],
        json!({"type":"Item", "id":"a"})
    );
    assert_eq!(checked["diagnostics"][0]["field_path"], "rewards[0].count");
    assert!(checked["diagnostics"][0]["explanation"].is_object());
    let (exit, _) = {
        let (exit, response, _) = call(
            root.path(),
            &["data", "get", "--type", "Item", "--id", "a"],
            None,
        );
        assert_eq!(response["result"]["id"], "a");
        (exit, response)
    };
    assert_eq!(exit, 0);
}

#[test]
fn evaluation_error_aborts_only_its_rule_without_claiming_budget_exhaustion() {
    let root = fixture();
    fs::write(
        root.path().join("schema.cft"),
        r"
table Item {
  value: int;
  check Division { 10 // value > 1; value >= 0; }
  check StillRuns { value == 0; }
}
",
    )
    .unwrap();
    fs::write(root.path().join("data/items.cfd"), "a: Item { value: 0 }\n").unwrap();
    let (exit, response, _) = call(root.path(), &["check", "--rules", "--details"], None);
    assert_eq!(exit, 1, "{response}");
    assert_eq!(response["result"]["rules"], "failed");
    assert_eq!(response["result"]["complete"], false);
    let tasks = response["result"]["tasks"].as_array().unwrap();
    assert_eq!(
        tasks
            .iter()
            .map(|task| task["state"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["evaluation_error", "not_executed", "passed"]
    );
    let results = response["result"]["results"].as_array().unwrap();
    assert_eq!(results[0]["state"], "evaluation_error");
    assert_eq!(results[1]["state"], "passed");
}

#[test]
fn describe_defaults_to_a_small_catalog_and_schemas_are_explicit_and_unique() {
    let root = tempfile::tempdir().unwrap();
    let (exit, catalog, _) = call(root.path(), &["describe"], None);
    assert_eq!(exit, 0, "{catalog}");
    let encoded = serde_json::to_string(&catalog).unwrap();
    assert!(
        encoded.len() < 12_000,
        "catalog grew to {} bytes",
        encoded.len()
    );
    assert!(catalog["result"]["commands"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["command"] == "data.set"));
    assert!(catalog["result"].get("definition").is_none());
    assert!(!encoded.contains("input_schema"));
    assert!(!encoded.contains("output_schema"));
    let (exit, command, _) = call(root.path(), &["describe", "data.set"], None);
    assert_eq!(exit, 0, "{command}");
    assert!(serde_json::to_vec(&command).unwrap().len() < 6000);
    assert!(command["result"]["definition"]
        .get("output_schema")
        .is_none());
    assert!(command["result"]["definition"]["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .all(|parameter| parameter["description"] != "Command option"));
    let (exit, schema, _) = call(
        root.path(),
        &["describe", "apply", "--schema", "both"],
        None,
    );
    assert_eq!(exit, 0, "{schema}");
    assert!(schema["result"]["definition"]["input_schema"].is_object());
    assert!(schema["result"]["definition"]["output_schema"].is_object());
    assert!(schema["result"].get("input_schema").is_none());
    assert!(schema["result"].get("output_schema").is_none());
    let (exit, group, _) = call(
        root.path(),
        &["describe", "data", "--schema", "output"],
        None,
    );
    assert_eq!(exit, 2, "{group}");
}

#[test]
fn json_is_one_line_and_pretty_is_an_explicit_json_mode() {
    let root = tempfile::tempdir().unwrap();
    for (arguments, pretty) in [
        (vec!["describe", "data.get", "--json"], false),
        (vec!["describe", "data.get", "--pretty"], true),
        (vec!["data", "set", "--pretty"], true),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_coflow"))
            .current_dir(root.path())
            .args(arguments)
            .output()
            .unwrap();
        let text = String::from_utf8(output.stdout).unwrap();
        let _: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(text.lines().count() > 1, pretty, "{text}");
        assert_eq!(output.stderr, [] as [u8; 0]);
    }
}

#[test]
fn argument_errors_preserve_cause_and_offer_targeted_help_without_usage_dump() {
    let root = tempfile::tempdir().unwrap();
    for (arguments, expected) in [
        (vec!["data", "list", "--limit", "oops"], "--limit"),
        (vec!["data", "set"], "--type"),
        (vec!["codegen", "--check", "--dry-run"], "--dry-run"),
    ] {
        let (exit, response, _) = call(root.path(), &arguments, None);
        assert_eq!(exit, 2, "{response}");
        let diagnostic = &response["diagnostics"][0];
        let message = diagnostic["message"].as_str().unwrap();
        assert!(message.contains(expected), "{diagnostic}");
        assert!(!message.contains("Usage:"), "{diagnostic}");
        assert!(diagnostic["help"].as_str().unwrap().contains("--help"));
    }
}

#[test]
fn terminal_diagnostics_include_source_target_and_repair_guidance() {
    let root = fixture();
    let output = Command::new(env!("CARGO_BIN_EXE_coflow"))
        .current_dir(root.path())
        .args(["check", "--rules"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("[CHECK-"), "{text}");
    assert!(text.contains("data/items.cfd:1:"), "{text}");
    assert!(text.contains("target Item/a.value"), "{text}");
    assert!(text.contains("help:"), "{text}");
}

#[test]
fn successful_rule_runs_default_to_counts_and_details_are_explicit() {
    let root = fixture();
    fs::write(
        root.path().join("data/items.cfd"),
        (0..80).fold(String::new(), |mut text, index| {
            let _ = writeln!(text, "item{index}: Item {{ value: 1 }}");
            text
        }),
    )
    .unwrap();
    let (exit, summary, _) = call(root.path(), &["check", "--rules"], None);
    assert_eq!(exit, 0, "{summary}");
    assert!(summary["result"].get("tasks").is_none());
    assert!(summary["result"].get("results").is_none());
    assert_eq!(summary["result"]["rule_counts"]["total"], 80);
    assert_eq!(summary["result"]["rule_counts"]["passed"], 80);
    assert!(serde_json::to_vec(&summary).unwrap().len() < 2000);
    let (exit, details, _) = call(root.path(), &["check", "--rules", "--details"], None);
    assert_eq!(exit, 0, "{details}");
    assert_eq!(details["result"]["tasks"].as_array().unwrap().len(), 80);
    assert_eq!(details["result"]["results"].as_array().unwrap().len(), 80);
}

#[test]
fn data_set_exposes_revision_and_typed_value_preconditions() {
    let root = fixture();
    let (exit, read, _) = call(
        root.path(),
        &["data", "get", "--type", "Item", "--id", "a"],
        None,
    );
    assert_eq!(exit, 0, "{read}");
    let revision = read["revision"].as_str().unwrap();
    let original = fs::read_to_string(root.path().join("data/items.cfd")).unwrap();
    let common = [
        "data",
        "set",
        "--type",
        "Item",
        "--id",
        "a",
        "--path",
        "value",
        "--value-cfd",
        "5",
    ];
    let arguments = common
        .into_iter()
        .chain([
            "--if-revision",
            revision,
            "--expected-cfd",
            "-1",
            "--dry-run",
        ])
        .collect::<Vec<_>>();
    let (exit, preview, _) = call(root.path(), &arguments, None);
    assert_eq!(exit, 0, "{preview}");
    for condition in [["--if-revision", "stale"], ["--expected-cfd", "0"]] {
        let arguments = common.into_iter().chain(condition).collect::<Vec<_>>();
        let (exit, conflict, _) = call(root.path(), &arguments, None);
        assert_eq!(exit, 3, "{conflict}");
        assert_eq!(conflict["status"], "conflict");
    }
    assert_eq!(
        fs::read_to_string(root.path().join("data/items.cfd")).unwrap(),
        original
    );
}

#[test]
fn failed_transactions_and_schema_previews_return_compact_source_patches() {
    let root = fixture();
    let original = (0..500).fold(String::new(), |mut text, index| {
        let _ = writeln!(text, "item{index}: Item {{ value: 1 }}");
        text
    });
    fs::write(root.path().join("data/items.cfd"), &original).unwrap();
    let changes = json!({"protocol_version":1,"kind":"operations","ops":
        (["item0", "missing"].into_iter().map(|id| json!({"op":"set_field","record":{"type":"Item","id":id},
            "path":[{"kind":"field","name":"value"}],"value":{"kind":"expression","source":"2"}})).collect::<Vec<_>>())});
    let (exit, failed, _) = call(
        root.path(),
        &["apply", "--input", "-"],
        Some(&changes.to_string()),
    );
    assert_eq!(exit, 1, "{failed}");
    assert_eq!(failed["result"]["failed_operation"], 1);
    assert_eq!(
        failed["result"]["target"],
        json!({"type":"Item","id":"missing"})
    );
    assert_eq!(failed["result"]["written"], false);
    let source = json!({"protocol_version":1,"kind":"files","changes":[
        {"op":"write","file":"data/items.cfd","content":original.replacen("value: 1", "value: 2", 1)}]});
    let (exit, preview, _) = call(
        root.path(),
        &["schema", "check", "--input", "-"],
        Some(&source.to_string()),
    );
    assert_eq!(exit, 0, "{preview}");
    for response in [&failed, &preview] {
        assert!(
            serde_json::to_vec(response).unwrap().len() < 5000,
            "preview repeated unchanged source"
        );
        let file = &response["result"]["files"][0];
        assert!(file.get("before").is_none());
        assert!(file.get("after").is_none());
        assert!(file["patch"]
            .as_str()
            .unwrap()
            .contains("+item0: Item { value: 2 }"));
    }
    assert_eq!(
        fs::read_to_string(root.path().join("data/items.cfd")).unwrap(),
        original
    );
}
