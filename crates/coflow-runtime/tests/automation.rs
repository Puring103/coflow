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
    assert_eq!(record.result["fields"]["numbers"]["value"][0]["value"],"2");
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
