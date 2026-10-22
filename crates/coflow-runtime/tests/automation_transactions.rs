#![allow(clippy::unwrap_used, clippy::expect_used)]
use coflow_runtime::{
    automation::{execute, Arguments, InputSnapshot, Status},
    Project,
};
use serde_json::{json, Value};
use std::{fs, path::Path};

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
        "table Item { value: int; }\n",
    )
    .unwrap();
    fs::write(
        root.path().join("data/items.cfd"),
        "a: Item { value: 1 }\nb: Item { value: 2 }\nc: Item { value: 3 }\n",
    )
    .unwrap();
    root
}

fn run(
    root: &Path,
    command: &str,
    options: &[(&str, &str)],
    input: Option<Value>,
) -> coflow_runtime::automation::Result<coflow_runtime::automation::Response> {
    let project = Project::open_schema_only(Some(root)).unwrap();
    let snapshot = InputSnapshot::capture(&project).unwrap();
    let args = Arguments(
        options
            .iter()
            .map(|(name, value)| (name.to_string(), vec![value.to_string()]))
            .collect(),
    );
    execute(
        command,
        &project,
        &snapshot,
        &args,
        input.as_ref().map(Value::to_string).as_deref(),
        root,
    )
}

#[test]
fn revisions_guard_every_write_entry_point_and_report_current_input() {
    let root = fixture();
    let read = run(
        root.path(),
        "data.get",
        &[("type", "Item"), ("id", "a")],
        None,
    )
    .unwrap();
    let before = read.revision.unwrap();
    fs::write(
        root.path().join("data/items.cfd"),
        "a: Item { value: 10 }\n",
    )
    .unwrap();
    for command in [
        "apply",
        "data.create",
        "data.set",
        "data.unset",
        "data.rename",
        "data.delete",
        "data.transfer",
        "dimension.set",
        "dimension.clear",
        "source.write",
        "source.create",
        "source.delete",
        "project.add-input",
        "project.remove-input",
        "format",
        "build",
        "codegen",
    ] {
        let error = run(root.path(), command, &[("if-revision", &before)], None).unwrap_err();
        assert_eq!(error.status, Status::Conflict, "{command}");
        let result = error.result.unwrap();
        assert_eq!(result["written"], false);
        assert_eq!(result["precondition"]["expected"], before);
        assert_ne!(result["precondition"]["actual"], before);
        assert_eq!(result["revision"], result["precondition"]["actual"]);
    }
    assert_eq!(
        fs::read_to_string(root.path().join("data/items.cfd")).unwrap(),
        "a: Item { value: 10 }\n"
    );
}

#[test]
fn successful_write_and_preview_return_revisions_and_old_value_condition() {
    let root = fixture();
    let read = run(
        root.path(),
        "data.get",
        &[("type", "Item"), ("id", "a")],
        None,
    )
    .unwrap();
    let before = read.revision.unwrap();
    let preview = run(
        root.path(),
        "data.set",
        &[
            ("type", "Item"),
            ("id", "a"),
            ("path", "value"),
            ("value-cfd", "4"),
            ("expected-cfd", "1"),
            ("if-revision", &before),
            ("dry-run", "true"),
        ],
        None,
    )
    .unwrap();
    assert_eq!(preview.revision.as_deref(), Some(before.as_str()));
    assert_eq!(preview.result["revision_before"], before);
    assert_eq!(preview.result["revision_after"], before);
    let written = run(
        root.path(),
        "data.set",
        &[
            ("type", "Item"),
            ("id", "a"),
            ("path", "value"),
            ("value-cfd", "4"),
            ("expected-cfd", "1"),
            ("if-revision", &before),
        ],
        None,
    )
    .unwrap();
    assert_eq!(written.result["revision_before"], before);
    assert_ne!(written.revision.as_deref(), Some(before.as_str()));
    assert_eq!(written.result["revision_after"], written.revision.unwrap());
    let failed = run(
        root.path(),
        "data.set",
        &[
            ("type", "Item"),
            ("id", "a"),
            ("path", "value"),
            ("value-cfd", "5"),
            ("expected-cfd", "1"),
        ],
        None,
    )
    .unwrap_err();
    let result = failed.result.unwrap();
    assert_eq!(result["operation_index"], 0);
    assert_eq!(result["target"], json!({"type":"Item","id":"a"}));
    assert_eq!(result["precondition"]["actual"]["value"], "4");
}

#[test]
fn change_set_revision_and_request_paths_are_enforced_before_execution() {
    let root = fixture();
    let request = json!({"protocol_version":1,"kind":"operations","if_revision":"stale","ops":[]});
    assert_eq!(
        run(root.path(), "apply", &[], Some(request))
            .unwrap_err()
            .status,
        Status::Conflict
    );
    let request = json!({"protocol_version":1,"kind":"operations","ops":[
        {"op":"delete_record","record":{"type":"Item","id":"a"}},
        {"op":"set_field","record":{"type":"Item","id":"b"},"path":[],"value":{"kind":"expression","source":17}}
    ]});
    let error = run(root.path(), "apply", &[], Some(request)).unwrap_err();
    assert_eq!(error.result.unwrap()["request_path"], "ops[1]");
    assert!(error.diagnostics.diagnostics[0].message.contains("string"));
    assert!(fs::read_to_string(root.path().join("data/items.cfd"))
        .unwrap()
        .contains("a: Item"));
}

#[test]
fn cursors_bind_revision_but_allow_page_size_and_presentation_changes() {
    let root = fixture();
    let first = run(root.path(), "data.list", &[("limit", "1")], None).unwrap();
    let cursor = first.result["next_cursor"].as_str().unwrap();
    let second = run(
        root.path(),
        "data.list",
        &[
            ("limit", "20"),
            ("cursor", cursor),
            ("pretty", "true"),
            ("details", "true"),
        ],
        None,
    )
    .unwrap();
    assert_eq!(second.result["items"].as_array().unwrap().len(), 2);
    fs::write(root.path().join("data/new.cfd"), "aa: Item { value: 4 }\n").unwrap();
    let error = run(
        root.path(),
        "data.list",
        &[("limit", "20"), ("cursor", cursor)],
        None,
    )
    .unwrap_err();
    assert_eq!(error.status, Status::Conflict);
    assert!(error.diagnostics.diagnostics[0]
        .message
        .contains("without --cursor"));
}

#[test]
fn revisions_are_stable_for_identical_relative_projects_and_include_new_inputs() {
    let left = fixture();
    let right = fixture();
    let revision = |root: &Path| {
        InputSnapshot::capture(&Project::open_schema_only(Some(root)).unwrap())
            .unwrap()
            .revision()
    };
    assert_eq!(revision(left.path()), revision(right.path()));
    fs::write(left.path().join("data/new.cfd"), "extra: Item { value: 5 }").unwrap();
    assert_ne!(revision(left.path()), revision(right.path()));
}

#[test]
fn failed_batches_and_schema_previews_report_patches_without_whole_files() {
    let root = fixture();
    let original = (0..500)
        .map(|index| format!("item{index}: Item {{ value: 1 }}\n"))
        .collect::<String>();
    fs::write(root.path().join("data/items.cfd"), &original).unwrap();
    let failed = run(root.path(), "apply", &[], Some(json!({"protocol_version":1,"kind":"operations","ops":[
        {"op":"set_field","record":{"type":"Item","id":"item0"},"path":[{"kind":"field","name":"value"}],"value":{"kind":"expression","source":"2"}},
        {"op":"delete_record","record":{"type":"Item","id":"missing"}}
    ]}))).unwrap_err();
    let result = failed.result.unwrap();
    let file = &result["files"][0];
    assert!(file.get("before").is_none() && file.get("after").is_none());
    assert!(file["patch"]
        .as_str()
        .unwrap()
        .contains("+item0: Item { value: 2 }"));
    assert!(file.to_string().len() < 1_000);
    assert_eq!(result["failed_operation"], 1);
    assert_eq!(result["target"]["id"], "missing");
    assert_eq!(result["written"], false);
    assert_eq!(
        fs::read_to_string(root.path().join("data/items.cfd")).unwrap(),
        original
    );

    let changed = original.replacen("value: 1", "value: 2", 1);
    let request = json!({"protocol_version":1,"kind":"files","changes":[{"op":"write","file":"data/items.cfd","content":changed}]});
    let preview = run(root.path(), "schema.check", &[], Some(request.clone())).unwrap();
    let file = &preview.result["files"][0];
    assert!(file.get("before").is_none() && file.get("after").is_none());
    assert!(file["patch"].is_string());
    assert!(file.to_string().len() < 1_000);
    let explicit = run(
        root.path(),
        "schema.check",
        &[("include-source", "true")],
        Some(request),
    )
    .unwrap();
    assert_eq!(explicit.result["files"][0]["before"], original);
    assert_eq!(explicit.result["files"][0]["after"], changed);
}

#[test]
fn codegen_preserves_the_callers_revision_until_publication_and_keeps_conflict_details() {
    let root = fixture();
    let project = Project::open_schema_only(Some(root.path())).unwrap();
    let snapshot = InputSnapshot::capture(&project).unwrap();
    let expected = snapshot.revision();
    // 模拟入口捕获修订之后、产物发布之前发生的外部编辑。
    fs::write(
        root.path().join("data/items.cfd"),
        "a: Item { value: 99 }\n",
    )
    .unwrap();
    let actual = InputSnapshot::capture(&project).unwrap().revision();
    let args = Arguments([("if-revision".into(), vec![expected.clone()])].into());
    let failure = execute("codegen", &project, &snapshot, &args, None, root.path()).unwrap_err();
    assert_eq!(failure.status, Status::Conflict);
    let result = failure.result.unwrap();
    assert_eq!(result["revision"], actual);
    assert_eq!(
        result["precondition"],
        json!({"kind":"revision","expected":expected,"actual":actual})
    );
    assert_eq!(result["written"], false);
    assert_eq!(result["artifacts"]["written"], false);
    assert_eq!(result["artifacts"]["written_files"], json!([]));
    assert!(!result["artifacts"]["files"].as_array().unwrap().is_empty());
    assert!(!root.path().join("generated").exists());
    let typed: coflow_runtime::automation::CodegenResult = serde_json::from_value(result).unwrap();
    assert_eq!(typed.revision.as_deref(), Some(actual.as_str()));
    assert!(typed.precondition.is_some());
    let schema = serde_json::to_value(coflow_runtime::automation::response_schema_for(Some(
        "codegen",
    )))
    .unwrap();
    assert!(schema["definitions"]["CodegenResult"]["properties"]
        .get("precondition")
        .is_some());
}

#[test]
fn deleting_a_schema_file_inside_a_directory_invalidates_the_cached_schema() {
    let root = fixture();
    fs::create_dir(root.path().join("schema")).unwrap();
    fs::rename(
        root.path().join("schema.cft"),
        root.path().join("schema/items.cft"),
    )
    .unwrap();
    fs::write(
        root.path().join("coflow.yaml"),
        "schema: schema/\ndata: data/\ncodegen:\n  - language: csharp\n    dir: generated/\n",
    )
    .unwrap();
    let result = run(root.path(), "apply", &[], Some(json!({"protocol_version":1,"kind":"files","changes":[{"op":"delete","file":"schema/items.cft"}]}))).unwrap();
    assert_eq!(result.status, Status::Invalid);
    assert_eq!(result.result["written"], false);
    assert!(root.path().join("schema/items.cft").exists());
    assert_eq!(result.result["validation"]["structure"], "failed");
}
