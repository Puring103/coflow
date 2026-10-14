#![allow(clippy::unwrap_used,clippy::expect_used)]
use coflow_runtime::{Project,automation::{Arguments,InputSnapshot,execute,Status}};
use std::{fs,path::Path};
use serde_json::{json,Value};

fn fixture()->tempfile::TempDir{
    let root=tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("data")).unwrap();
    fs::write(root.path().join("coflow.yaml"),"schema: schema.cft\ndata: data/\ndimensions:\n  language:\n    dir: translations/\ncodegen:\n  - language: csharp\n    dir: generated/\n").unwrap();
    fs::write(root.path().join("schema.cft"),"table Item { value: int; next: Option<&Item>; numbers: [int] = []; check { value > 0; } }\n").unwrap();
    fs::write(root.path().join("data/items.cfd"),"a: Item { value: 10 }\nb: Item { value: 20 }\n").unwrap();root
}
fn run(root:&Path,command:&str,options:&[(&str,&str)],input:Option<Value>)->coflow_runtime::automation::Response{
    let project=Project::open_schema_only(Some(root)).unwrap();let snapshot=InputSnapshot::capture(&project).unwrap();
    let args=Arguments(options.iter().map(|(k,v)|(k.to_string(),vec![v.to_string()])).collect());
    execute(command,&project,&snapshot,&args,input.as_ref().map(Value::to_string).as_deref(),root).unwrap()
}
#[test]
fn source_ranges_preserve_original_line_endings_and_trailing_newlines() {
    let root = fixture();
    fs::write(root.path().join("data/items.cfd"), "a: Item { value: 1 }\r\n\r\nb: Item { value: 2 }\n").unwrap();
    let result = run(root.path(), "source.read", &[("file", "data/items.cfd"), ("start-line", "0"), ("end-line", "2")], None);
    assert_eq!(result.result["source"], "a: Item { value: 1 }\r\n\r\n");
    let result = run(root.path(), "source.read", &[("file", "data/items.cfd"), ("start-line", "2")], None);
    assert_eq!(result.result["source"], "b: Item { value: 2 }\n");
    fs::write(root.path().join("data/items.cfd"), "a: Item { value: 1 }").unwrap();
    let result = run(root.path(), "source.read", &[("file", "data/items.cfd"), ("start-line", "0"), ("end-line", "1")], None);
    assert_eq!(result.result["source"], "a: Item { value: 1 }");
}

#[test]
fn schema_symbols_reject_unknown_kinds_before_returning_an_empty_page() {
    let root = fixture();
    let project = Project::open_schema_only(Some(root.path())).unwrap();
    let snapshot = InputSnapshot::capture(&project).unwrap();
    let args = Arguments([("kind".into(), vec!["unknown".into()])].into());
    let error = execute("schema.symbols", &project, &snapshot, &args, None, root.path()).unwrap_err();
    assert!(matches!(error.error_category, Some(coflow_runtime::automation::ErrorCategory::Request)));
}

#[test]
fn context_includes_related_declarations_target_sources_and_bounded_references() {
    let root = fixture();
    fs::write(root.path().join("schema.cft"), "enum Color { Red, Blue }\ndata Details { color: Color = Color::Red; }\ntable Item { value: int = 10; details: Details; next: Option<&Item>; }\n").unwrap();
    fs::write(root.path().join("data/items.cfd"), "a: Item { details: {} }\nb: Item { value: 20, details: {} }\n").unwrap();
    let result = run(root.path(), "context", &[("type", "Item"), ("key", "a"), ("examples", "0"), ("ref-limit", "1")], None);
    assert_eq!(result.status, Status::Success, "{result:?}");
    assert_eq!(result.result["examples"], json!([]));
    assert!(result.result["schema"]["types"].as_array().unwrap().iter().any(|item| item["name"] == "Details"));
    assert!(result.result["schema"]["enums"].as_array().unwrap().iter().any(|item| item["name"] == "Color"));
    assert_eq!(result.result["target"]["fields"]["value"]["value"], "10");
    assert_eq!(result.result["target"]["source"]["value"]["origin"], "schema_default");
    assert_eq!(result.result["references"][0]["total"], 2);
    assert_eq!(result.result["references"][0]["items"].as_array().unwrap().len(), 1);
    assert_eq!(result.result["references"][0]["truncated"], true);
}

#[test]
fn context_budget_preserves_core_identity_and_reports_declaration_omissions() {
    let root = fixture();
    let fields = (0..80).map(|index| format!("field{index}: int = {index};")).collect::<String>();
    fs::write(root.path().join("schema.cft"), format!("table Item {{ {fields} }}\n")).unwrap();
    fs::write(root.path().join("data/items.cfd"), "a: Item {}\n").unwrap();
    let result = run(root.path(), "context", &[("type", "Item"), ("budget", "2500")], None);
    assert_eq!(result.status, Status::Success, "{result:?}");
    assert!(result.result.to_string().chars().count() <= 2500);
    assert_eq!(result.result["schema"]["types"][0]["name"], "Item");
    assert!(result.result["schema"]["types"][0]["fields"].as_array().unwrap().len() < 80);
    assert!(result.result["omitted"].as_array().unwrap().iter().any(|item| item["section"] == "fields" && item["query"].as_str().unwrap().contains("schema inspect")));
}
#[test]
fn writes_are_previewed_and_invalid_candidates_never_publish(){
    let root=fixture();let file=root.path().join("data/items.cfd");let before=fs::read(&file).unwrap();
    let result=run(root.path(),"data.set",&[("type","Item"),("key","a"),("path","value"),("value-cfd","30"),("dry-run","true")],None);
    assert_eq!(result.status,Status::Success);assert_eq!(result.result["changed"],true);assert_eq!(fs::read(&file).unwrap(),before);
    let result=run(root.path(),"data.set",&[("type","Item"),("key","a"),("path","value"),("value-cfd","-1"),("rules","true")],None);
    assert_eq!(result.status,Status::Invalid);assert_eq!(result.result["written"],false);assert_eq!(fs::read(&file).unwrap(),before);
    let result=run(root.path(),"data.set",&[("type","Item"),("key","a"),("path","value"),("value-cfd","30")],None);
    assert_eq!(result.status,Status::Success);assert!(fs::read_to_string(&file).unwrap().contains("30"));
}
#[test]
fn operations_support_forward_cycles_transfer_and_sequential_arrays(){
    let root=fixture();
    let result=run(root.path(),"apply",&[],Some(json!({"protocol_version":1,"kind":"operations","ops":[
        {"op":"create_record","record":{"type":"Item","key":"c"},"file":"data/items.cfd","fields":{"value":{"kind":"expression","source":"3"},"next":{"kind":"expression","source":"&d"}}},
        {"op":"create_record","record":{"type":"Item","key":"d"},"file":"data/items.cfd","fields":{"value":{"kind":"expression","source":"4"},"next":{"kind":"expression","source":"&c"}}},
        {"op":"transfer_record","record":{"type":"Item","key":"c"},"to_file":"data/other.cfd"},
        {"op":"array_insert","record":{"type":"Item","key":"c"},"path":[{"kind":"field","name":"numbers"}],"index":0,"value":{"kind":"expression","source":"1"}},
        {"op":"array_insert","record":{"type":"Item","key":"c"},"path":[{"kind":"field","name":"numbers"}],"index":0,"value":{"kind":"expression","source":"2"}}
    ]})));
    assert_eq!(result.status,Status::Success,"{result:?}");
    assert!(root.path().join("data/other.cfd").exists());
    let record=run(root.path(),"data.get",&[("type","Item"),("key","c")],None);
    assert_eq!(record.result["fields"]["numbers"]["items"][0]["value"],"2");
}
#[test]
fn cursor_reads_current_data_and_default_check_skips_business_rules(){
    let root=fixture();let first=run(root.path(),"data.list",&[("limit","1")],None);
    let cursor=first.result["next_cursor"].as_str().unwrap().to_string();
    fs::write(root.path().join("data/items.cfd"),"a: Item { value: -1 }\nb: Item { value: 20 }\n").unwrap();
    let project=Project::open_schema_only(Some(root.path())).unwrap();let snapshot=InputSnapshot::capture(&project).unwrap();
    let args=Arguments([("limit".into(),vec!["1".into()]),("cursor".into(),vec![cursor])].into());
    let page=execute("data.list",&project,&snapshot,&args,None,root.path()).unwrap();
    assert_eq!(page.status,Status::Success);
    assert_eq!(page.result["items"][0]["key"],"b");
    assert!(serde_json::to_value(&page).unwrap().get("snapshot").is_none());
    assert_eq!(run(root.path(),"check",&[],None).status,Status::Success);
    assert_eq!(run(root.path(),"check",&[("rules","true")],None).status,Status::Invalid);
}

#[test]
fn requests_and_results_do_not_expose_project_snapshots() {
    let root = fixture();
    let request = json!({"protocol_version": 1, "kind": "operations", "ops": []});
    let result = run(root.path(), "apply", &[], Some(request.clone()));
    let encoded = serde_json::to_value(result).unwrap();
    assert!(encoded.get("snapshot").is_none());
    assert!(encoded["result"].get("before_snapshot").is_none());
    assert!(encoded["result"].get("after_snapshot").is_none());

    let mut legacy = request;
    legacy["expected_snapshot"] = json!("old");
    assert!(serde_json::from_value::<coflow_runtime::automation::ChangeSet>(legacy).is_err());
    assert!(!serde_json::to_string(&coflow_runtime::automation::response_schema()).unwrap().contains("snapshot"));
    assert!(!serde_json::to_string(&coflow_runtime::automation::change_set_schema()).unwrap().contains("snapshot"));
}

#[test]
fn cursors_allow_page_size_changes_but_reject_another_query() {
    let root = fixture();
    let first = run(root.path(), "data.list", &[("limit", "1")], None);
    let cursor = first.result["next_cursor"].as_str().unwrap();
    let next = run(root.path(), "data.list", &[("limit", "20"), ("cursor", cursor)], None);
    assert_eq!(next.result["items"].as_array().unwrap().len(), 1);
    assert_eq!(next.result["items"][0]["key"], "b");
    let project = Project::open_schema_only(Some(root.path())).unwrap();
    let inputs = InputSnapshot::capture(&project).unwrap();
    let args = Arguments([("cursor".into(), vec![cursor.into()]), ("type".into(), vec!["Item".into()])].into());
    assert_eq!(execute("data.list", &project, &inputs, &args, None, root.path()).unwrap_err().status, Status::Error);
}

fn value_fixture() -> tempfile::TempDir {
    let root = fixture();
    fs::write(root.path().join("schema.cft"), r#"
enum Quality { Common, Rare }
data Details { count: int = 1; }
table Item {
    value: int = 100;
    nested: Option<Option<int>>;
    next: Option<&Item>;
    quality: Quality;
    details: Details;
    numbers: [int];
    labels: {string: int};
    indexed: {int: int};
    by_quality: {Quality: int};
    @localized
    name: string;
    label: fstring;
}
"#).unwrap();
    fs::write(root.path().join("data/items.cfd"), r#"
a: Item {
    nested: Some(None), next: &b, quality: Quality::Rare,
    details: { count: 7 }, numbers: [9223372036854775807],
    labels: { "a.b": 1 }, indexed: { -1: 2, 3: 4 }, by_quality: { Rare: 5 },
    name: "base", label: f"value {self.value}"
}
b: Item {
    nested: None, next: None, quality: Quality::Common,
    details: {}, numbers: [], labels: {}, indexed: {}, by_quality: {},
    name: "other", label: f"value {self.value}"
}
"#).unwrap();
    root
}

#[test]
fn query_values_can_be_submitted_without_internal_model_encoding() {
    let root = value_fixture();
    let before = run(root.path(), "data.get", &[("type", "Item"), ("key", "a")], None);
    assert_eq!(before.status, Status::Success, "{before:?}");
    assert_eq!(before.result["fields"]["numbers"]["items"][0], json!({"kind":"int","value":"9223372036854775807"}));
    assert_eq!(before.result["fields"]["nested"], json!({"kind":"some","value":{"kind":"none"}}));
    assert_eq!(before.result["fields"]["next"], json!({"kind":"some","value":{"kind":"ref","type":"Item","key":"b"}}));
    assert_eq!(before.result["fields"]["label"], json!({"kind":"fstring","source":"f\"value {self.value}\""}));
    assert_eq!(before.result["rendered"]["label"], "value 100");
    let operations: Vec<_> = before.result["fields"].as_object().unwrap().iter().map(|(field, value)| json!({
        "op":"set_field","record":{"type":"Item","key":"b"},
        "path":[{"kind":"field","name":field}],"value":{"kind":"model","value":value}
    })).collect();
    let write = run(root.path(), "apply", &[], Some(json!({"protocol_version":1,"kind":"operations","ops":operations})));
    assert_eq!(write.status, Status::Success, "{write:?}");
    let after = run(root.path(), "data.get", &[("type", "Item"), ("key", "b")], None);
    assert_eq!(after.result["fields"], before.result["fields"]);
    assert_eq!(after.result["rendered"], before.result["rendered"]);
}

#[test]
fn setting_a_default_creates_explicit_data_then_preserves_identical_source() {
    let root = value_fixture();
    let file = root.path().join("data/items.cfd");
    let config = fs::read(root.path().join("coflow.yaml")).unwrap();
    let options = [("type", "Item"), ("key", "a"), ("path", "value"), ("value-cfd", "100")];
    let result = run(root.path(), "data.set", &options, None);
    assert_eq!(result.status, Status::Success, "{result:?}");
    assert_eq!(result.result["changed"], true);
    let source = fs::read(&file).unwrap();
    let modified = fs::metadata(&file).unwrap().modified().unwrap();
    let result = run(root.path(), "data.set", &options, None);
    assert_eq!(result.result["changed"], false);
    assert_eq!(fs::read(&file).unwrap(), source);
    assert_eq!(fs::metadata(&file).unwrap().modified().unwrap(), modified);
    assert_eq!(fs::read(root.path().join("coflow.yaml")).unwrap(), config);
}

#[test]
fn typed_dictionary_paths_validate_keys_and_share_text_query_paths() {
    let root = value_fixture();
    for (field, key, text_path) in [
        ("labels", json!({"kind":"string","value":"a.b"}), "labels[\"a.b\"]"),
        ("indexed", json!({"kind":"int","value":"-1"}), "indexed[-1]"),
        ("indexed", json!({"kind":"int","value":"3"}), "indexed[3]"),
        ("by_quality", json!({"kind":"enum","type":"Quality","variant":"Rare"}), "by_quality[Quality::Rare]"),
    ] {
        let write = run(root.path(), "apply", &[], Some(json!({"protocol_version":1,"kind":"operations","ops":[{
            "op":"set_field","record":{"type":"Item","key":"a"},
            "path":[{"kind":"field","name":field},{"kind":"dict_key","key":key}],
            "value":{"kind":"model","value":{"kind":"int","value":"9"}}
        }]})));
        assert_eq!(write.status, Status::Success, "{write:?}");
        let query = run(root.path(), "data.get", &[("type","Item"),("key","a"),("fields",text_path)], None);
        assert_eq!(query.result["fields"][text_path], json!({"kind":"int","value":"9"}), "{query:?}");
    }
}

#[test]
fn malformed_typed_values_fail_without_publishing() {
    let root = value_fixture();
    let before = fs::read(root.path().join("data/items.cfd")).unwrap();
    for (field, value) in [
        ("value", json!({"kind":"int","value":9223372036854775807_i64})),
        ("value", json!({"kind":"int","value":"9223372036854775808"})),
        ("nested", json!({"kind":"none","value":null})),
        ("nested", json!({"kind":"int","value":"1"})),
        ("quality", json!({"kind":"enum","type":"Wrong","variant":"Rare"})),
        ("labels", json!({"kind":"dict","entries":[{"key":{"kind":"int","value":"1"},"value":{"kind":"int","value":"2"}}]})),
        ("label", json!({"kind":"fstring","source":"f\"text\"","rendered":"text"})),
    ] {
        let project = Project::open_schema_only(Some(root.path())).unwrap();
        let snapshot = InputSnapshot::capture(&project).unwrap();
        let request = json!({"protocol_version":1,"kind":"operations","ops":[{
            "op":"set_field","record":{"type":"Item","key":"a"},"path":[{"kind":"field","name":field}],
            "value":{"kind":"model","value":value}
        }]}).to_string();
        let error = execute("apply", &project, &snapshot, &Arguments::default(), Some(&request), root.path()).unwrap_err();
        assert_eq!(error.status, Status::Error, "{error:?}");
        assert_eq!(fs::read(root.path().join("data/items.cfd")).unwrap(), before);
    }
}

#[test]
fn dimension_base_flags_are_distinct_from_a_variant_named_base() {
    let root = value_fixture();
    let write = run(root.path(), "dimension.set", &[("type","Item"),("key","a"),("field","name"),
        ("dimension","language"),("variant","base"),("value-cfd","\"translated\"")], None);
    assert_eq!(write.status, Status::Success, "{write:?}");
    let get = run(root.path(), "dimension.get", &[("type","Item"),("key","a"),("field","name"),
        ("dimension","language"),("variant","base")], None);
    assert_eq!(get.result["base"], json!({"kind":"string","value":"base"}));
    assert_eq!(get.result["override"], json!({"kind":"string","value":"translated"}));
    let diff = run(root.path(), "dimension.diff", &[("type","Item"),("key","a"),("field","name"),
        ("dimension","language"),("from-base","true"),("to","base")], None);
    assert_eq!(diff.result["total"], 1, "{diff:?}");
    assert_eq!(diff.result["items"][0]["before"]["effective"], get.result["base"]);
    assert_eq!(diff.result["items"][0]["after"]["effective"], get.result["override"]);
    let reverse = run(root.path(), "dimension.diff", &[("type","Item"),("key","a"),("field","name"),
        ("dimension","language"),("from","base"),("to-base","true")], None);
    assert_eq!(reverse.result["items"][0]["before"]["effective"], get.result["override"]);
    assert_eq!(reverse.result["items"][0]["after"]["effective"], get.result["base"]);
}

#[test]
fn absolute_candidate_config_uses_candidate_sources_and_preserves_submitted_text() {
    let root = fixture();
    let schema_path = root.path().join("schema.cft");
    let config_path = root.path().join("coflow.yaml");
    let before_schema = fs::read(&schema_path).unwrap();
    let before_config = fs::read(&config_path).unwrap();
    let contents = format!("# Submitted configuration\nschema: '{}'\ndata: '{}'\ncodegen: [{{ language: csharp, dir: generated/ }}]\n", schema_path.display(), root.path().join("data").display());
    let changes = json!({"protocol_version":1,"kind":"files","changes":[
        {"op":"write","file":"schema.cft","content":"table Item { value: string; }\n"},
        {"op":"write","file":"coflow.yaml","content":contents}
    ]});
    let invalid = run(root.path(), "apply", &[], Some(changes.clone()));
    assert_eq!(invalid.status, Status::Invalid, "{invalid:?}");
    assert_eq!(invalid.result["written"], false);
    assert_eq!(fs::read(&schema_path).unwrap(), before_schema);
    assert_eq!(fs::read(&config_path).unwrap(), before_config);
    let encoded = serde_json::to_value(&invalid).unwrap();
    assert!(encoded["diagnostics"].as_array().unwrap().iter().any(|d| d["location"]["path"] == "data/items.cfd"), "{encoded}");

    let mut valid = changes;
    valid["changes"].as_array_mut().unwrap().push(json!({"op":"write","file":"data/items.cfd","content":"a: Item { value: \"updated\" }\n"}));
    let result = run(root.path(), "apply", &[], Some(valid));
    assert_eq!(result.status, Status::Success, "{result:?}");
    assert_eq!(fs::read_to_string(&config_path).unwrap(), contents);
    assert_eq!(run(root.path(), "check", &[], None).status, Status::Success);
}

#[test]
fn candidate_config_imports_new_inputs_without_overwriting_prior_edits() {
    let root = fixture();
    fs::create_dir(root.path().join("extra")).unwrap();
    fs::write(root.path().join("extra/new.cfd"), "c: Item { value: 30 }\n").unwrap();
    let request = json!({"protocol_version":1,"kind":"files","changes":[
        {"op":"write","file":"data/items.cfd","content":"a: Item { value: 40 }\n"},
        {"op":"write","file":"coflow.yaml","content":"schema: schema.cft\ndata: [data/, extra/]\ncodegen: [{ language: csharp, dir: generated/ }]\n"}
    ]});
    let result = run(root.path(), "apply", &[], Some(request));
    assert_eq!(result.status, Status::Success, "{result:?}");
    let a = run(root.path(), "data.get", &[("type","Item"),("key","a")], None);
    assert_eq!(a.result["fields"]["value"]["value"], "40");
    let c = run(root.path(), "data.get", &[("type","Item"),("key","c")], None);
    assert_eq!(c.result["fields"]["value"]["value"], "30");
    assert!(!result.result["written_files"].as_array().unwrap().contains(&json!("extra/new.cfd")));
}

#[test]
fn empty_input_directories_and_source_deletes_preserve_unchanged_config() {
    let root = fixture();
    fs::create_dir(root.path().join("empty")).unwrap();
    let config = "# Keep layout\nschema: schema.cft\ndata: [data/, empty/]\ncodegen: [{ language: csharp, dir: generated/ }]\n";
    fs::write(root.path().join("coflow.yaml"), config).unwrap();
    let noop = run(root.path(), "apply", &[], Some(json!({"protocol_version":1,"kind":"operations","ops":[]})));
    assert_eq!(noop.status, Status::Success, "{noop:?}");
    assert_eq!(noop.result["changed"], false);
    let delete = run(root.path(), "source.delete", &[("file","data/items.cfd")], None);
    assert_eq!(delete.status, Status::Success, "{delete:?}");
    assert_eq!(fs::read_to_string(root.path().join("coflow.yaml")).unwrap(), config);
    assert_eq!(delete.result["written_files"], json!(["data/items.cfd"]));
}

#[test]
fn new_explicit_external_scope_can_be_created_in_the_same_transaction() {
    let root = fixture();
    let external = tempfile::tempdir().unwrap();
    let path = external.path().join("new.cfd");
    let config = format!("schema: schema.cft\ndata: [data/, '{}']\ncodegen: [{{ language: csharp, dir: generated/ }}]\n", path.display());
    let result = run(root.path(), "apply", &[], Some(json!({"protocol_version":1,"kind":"files","changes":[
        {"op":"write","file":"coflow.yaml","content":config},
        {"op":"create","file":path,"content":"c: Item { value: 30 }\n"}
    ]})));
    assert_eq!(result.status, Status::Success, "{result:?}");
    assert_eq!(fs::read_to_string(&path).unwrap(), "c: Item { value: 30 }\n");
    assert_eq!(run(root.path(), "check", &[], None).status, Status::Success);
}

#[test]
fn source_fingerprint_checks_the_submitted_config_not_the_execution_mapping() {
    let root = fixture();
    let source = run(root.path(), "source.read", &[("file","coflow.yaml")], None);
    let fingerprint = source.result["file"]["fingerprint"].as_str().unwrap();
    let contents = "# Exact source\nschema: schema.cft\ndata: data/\ncodegen: [{ language: csharp, dir: generated/ }]\n";
    let project = Project::open_schema_only(Some(root.path())).unwrap();
    let snapshot = InputSnapshot::capture(&project).unwrap();
    let args = Arguments([("file".into(),vec!["coflow.yaml".into()]),("expected-fingerprint".into(),vec![fingerprint.into()])].into());
    let result = execute("source.write", &project, &snapshot, &args, Some(contents), root.path()).unwrap();
    assert_eq!(result.status, Status::Success, "{result:?}");
    assert_eq!(fs::read_to_string(root.path().join("coflow.yaml")).unwrap(), contents);
}

#[test]
fn failed_operations_report_their_index_precondition_and_unexecuted_tail() {
    let root = fixture();
    let path = root.path().join("data/items.cfd");
    let before = fs::read(&path).unwrap();
    let project = Project::open_schema_only(Some(root.path())).unwrap();
    let snapshot = InputSnapshot::capture(&project).unwrap();
    let request = json!({"protocol_version":1,"kind":"operations","ops":[
        {"op":"set_field","record":{"type":"Item","key":"a"},"path":[{"kind":"field","name":"value"}],"value":{"kind":"expression","source":"30"}},
        {"op":"set_field","record":{"type":"Item","key":"b"},"path":[{"kind":"field","name":"value"}],"value":{"kind":"expression","source":"40"},"expected":{"kind":"expression","source":"999"}},
        {"op":"delete_record","record":{"type":"Item","key":"a"}}
    ]}).to_string();
    let error = execute("apply", &project, &snapshot, &Arguments::default(), Some(&request), root.path()).unwrap_err();
    assert_eq!(error.status, Status::Conflict);
    let result = error.result.unwrap();
    assert_eq!(result["written"], false);
    assert_eq!(result["failed_operation"], 1);
    assert_eq!(result["operations"][0]["status"], "success");
    assert_eq!(result["operations"][1]["status"], "failed");
    assert_eq!(result["operations"][2]["status"], "not_executed");
    assert_eq!(result["precondition"]["expected"], json!({"kind":"int","value":"999"}));
    assert_eq!(result["precondition"]["actual"], json!({"kind":"int","value":"20"}));
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn file_failures_report_unexecuted_changes_and_preserve_project_contents() {
    let root = fixture();
    let path = root.path().join("data/items.cfd");
    let before = fs::read(&path).unwrap();
    let project = Project::open_schema_only(Some(root.path())).unwrap();
    let snapshot = InputSnapshot::capture(&project).unwrap();
    let request = json!({"protocol_version":1,"kind":"files","changes":[
        {"op":"write","file":"data/items.cfd","content":"a: Item { value: 30 }\n"},
        {"op":"write","file":"schema.cft","content":"table Item { value: string; }\n","expected_fingerprint":"outdated"},
        {"op":"delete","file":"data/items.cfd"}
    ]}).to_string();
    let error = execute("apply", &project, &snapshot, &Arguments::default(), Some(&request), root.path()).unwrap_err();
    assert_eq!(error.status, Status::Conflict);
    let result = error.result.unwrap();
    assert_eq!(result["written"], false);
    assert_eq!(result["failed_operation"], 1);
    assert_eq!(result["operations"][2]["status"], "not_executed");
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn invalid_schema_candidate_returns_its_diff_and_original_source_location() {
    let root = fixture();
    let before = fs::read(root.path().join("schema.cft")).unwrap();
    let result = run(root.path(), "apply", &[], Some(json!({"protocol_version":1,"kind":"files","changes":[
        {"op":"write","file":"schema.cft","content":"table Item { value: Unknown; }\n"}
    ]})));
    assert_eq!(result.status, Status::Invalid, "{result:?}");
    assert_eq!(result.result["written"], false);
    assert_eq!(result.result["files"][0]["path"], "schema.cft");
    let encoded = serde_json::to_value(&result).unwrap();
    assert!(encoded["diagnostics"].as_array().unwrap().iter().any(|d|d["location"]["path"] == "schema.cft"));
    assert_eq!(fs::read(root.path().join("schema.cft")).unwrap(), before);
}

#[test]
fn format_previews_and_checks_are_read_only_and_publication_reports_files() {
    let root = fixture();
    let path = root.path().join("data/items.cfd");
    fs::write(&path, "a:Item{value:10}\n").unwrap();
    let before = fs::read(&path).unwrap();
    let preview = run(root.path(), "format", &[("file","data/items.cfd"),("dry-run","true")], None);
    assert_eq!(preview.status, Status::Success);
    assert_eq!(preview.result["dry_run"], true);
    assert_eq!(preview.result["written"], false);
    assert_eq!(preview.result["written_files"], json!([]));
    assert_eq!(fs::read(&path).unwrap(), before);
    let check = run(root.path(), "format", &[("file","data/items.cfd"),("check","true")], None);
    assert_eq!(check.status, Status::Invalid);
    assert_eq!(fs::read(&path).unwrap(), before);
    let publish = run(root.path(), "format", &[("file","data/items.cfd")], None);
    assert_eq!(publish.status, Status::Success);
    assert_eq!(publish.result["written"], true);
    assert_eq!(publish.result["written_files"], json!(["data/items.cfd"]));
    let noop = run(root.path(), "format", &[("file","data/items.cfd")], None);
    assert_eq!(noop.result["changed"], false);
    assert_eq!(noop.result["written"], false);
}

#[test]
fn format_conflicts_return_the_planned_changes_without_writing() {
    let root = fixture();
    let path = root.path().join("data/items.cfd");
    fs::write(&path, "a:Item{value:10}\n").unwrap();
    let project = Project::open_schema_only(Some(root.path())).unwrap();
    let snapshot = InputSnapshot::capture(&project).unwrap();
    fs::write(&path, "a: Item { value: 99 }\n").unwrap();
    let external = fs::read(&path).unwrap();
    let args = Arguments([("file".into(),vec!["data/items.cfd".into()])].into());
    let error = execute("format", &project, &snapshot, &args, None, root.path()).unwrap_err();
    assert_eq!(error.status, Status::Conflict);
    let result = error.result.unwrap();
    assert_eq!(result["changed"], true);
    assert_eq!(result["written"], false);
    assert_eq!(result["files"][0]["path"], "data/items.cfd");
    assert_eq!(fs::read(&path).unwrap(), external);
}

#[test]
fn sorting_validates_collection_and_heterogeneous_types_even_without_matches() {
    let root = fixture();
    let project = Project::open_schema_only(Some(root.path())).unwrap();
    let snapshot = InputSnapshot::capture(&project).unwrap();
    let args = Arguments([("type".into(),vec!["Item".into()]),("where".into(),vec!["false".into()]),("sort".into(),vec!["numbers".into()])].into());
    let error = execute("data.list", &project, &snapshot, &args, None, root.path()).unwrap_err();
    assert_eq!(error.status, Status::Error);
    assert!(matches!(error.error_category, Some(coflow_runtime::automation::ErrorCategory::Request)));
    fs::write(root.path().join("schema.cft"), "table Item { value: int; }\ntable Other { value: string; }\n").unwrap();
    let project = Project::open_schema_only(Some(root.path())).unwrap();
    let snapshot = InputSnapshot::capture(&project).unwrap();
    let args = Arguments([("sort".into(),vec!["value".into()])].into());
    let error = execute("data.list", &project, &snapshot, &args, None, root.path()).unwrap_err();
    assert!(error.diagnostics.diagnostics[0].message.contains("incompatible"));
}

#[test]
fn sorting_orders_unreachable_none_and_large_signed_values_without_precision_loss() {
    let root = fixture();
    fs::write(root.path().join("schema.cft"), "data Details { value: Option<int>; }\ntable Item { details: Option<Details>; }\n").unwrap();
    fs::write(root.path().join("data/items.cfd"), "z: Item { details: { value: 9223372036854775807 } }\ny: Item { details: { value: 9223372036854775806 } }\nx: Item { details: { value: None } }\nw: Item { details: None }\n").unwrap();
    let ascending = run(root.path(), "data.list", &[("type","Item"),("sort","details.value")], None);
    let keys: Vec<_> = ascending.result["items"].as_array().unwrap().iter().map(|item|item["key"].as_str().unwrap()).collect();
    assert_eq!(keys,vec!["w","x","y","z"]);
    let descending = run(root.path(), "data.list", &[("type","Item"),("sort","-details.value")], None);
    let keys: Vec<_> = descending.result["items"].as_array().unwrap().iter().map(|item|item["key"].as_str().unwrap()).collect();
    assert_eq!(keys,vec!["z","y","x","w"]);
}

#[test]
fn malformed_field_paths_are_rejected_without_records() {
    let root = fixture();
    fs::write(root.path().join("data/items.cfd"), "").unwrap();
    let project = Project::open_schema_only(Some(root.path())).unwrap();
    let snapshot = InputSnapshot::capture(&project).unwrap();
    for path in ["value.","value..child",".value","numbers.[0]","numbers[]","numbers[0]value"] {
        let args = Arguments([("type".into(),vec!["Item".into()]),("fields".into(),vec![path.into()])].into());
        let error = execute("data.list", &project, &snapshot, &args, None, root.path()).unwrap_err();
        assert!(matches!(error.error_category,Some(coflow_runtime::automation::ErrorCategory::Request)), "{path}: {error:?}");
    }
}

#[test]
fn field_projection_traverses_nested_some_layers_and_preserves_terminal_none() {
    let root = fixture();
    fs::write(root.path().join("schema.cft"), "data Details { value: Option<int>; }\ntable Item { details: Option<Option<Details>>; }\n").unwrap();
    fs::write(root.path().join("data/items.cfd"), "a: Item { details: Some(Some({ value: 7 })) }\nb: Item { details: Some(Some({ value: None })) }\nc: Item { details: Some(None) }\n").unwrap();
    let result = run(root.path(), "data.list", &[("type","Item"),("fields","details.value")], None);
    assert_eq!(result.status, Status::Success, "{result:?}");
    assert_eq!(result.result["items"][0]["fields"]["details.value"],json!({"kind":"some","value":{"kind":"int","value":"7"}}));
    assert_eq!(result.result["items"][1]["fields"]["details.value"],json!({"kind":"none"}));
    assert_eq!(result.result["items"][2]["fields"]["details.value"],Value::Null);
    assert_eq!(result.result["items"][2]["origins"]["details.value"]["reachable"],false);
}

