#![allow(
    clippy::expect_used,
    clippy::needless_raw_string_hashes,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::too_many_lines,
    clippy::unwrap_used
)]

mod common;
use common::*;

use coflow_api::{Severity, SourceLocation};
use coflow_project::Project;
use coflow_runtime::compile_schema_project_with_overrides;
use std::process::Stdio;

#[test]
fn cli_lsp_and_runtime_share_the_canonical_schema_diagnostic() {
    let project_dir = temp_project_dir("schema-diagnostic-host-golden");
    let _cleanup = TempDirCleanup(project_dir.clone());
    let schema_dir = project_dir.join("schema");
    std::fs::create_dir_all(&schema_dir).expect("create schema dir");
    std::fs::write(project_dir.join("coflow.yaml"), "schema: schema/\n").expect("write config");
    let schema_path = schema_dir.join("main.cft");
    let source = "type 表 {\n  名: Missing;\n}\n";
    std::fs::write(&schema_path, source).expect("write schema");

    let project = Project::open_schema_only(Some(&project_dir)).expect("open project");
    let build = compile_schema_project_with_overrides(&project, &[]).expect("compile schema");
    let expected = build
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "CFT-SCHEMA-006")
        .expect("runtime diagnostic");
    assert_eq!(expected.severity, Severity::Error);
    let expected_range = expected
        .primary
        .as_ref()
        .expect("runtime primary label")
        .location
        .text_range();
    assert!(matches!(
        &expected.primary.as_ref().expect("primary").location,
        SourceLocation::FileSpan { path, .. }
            if coflow_project::normalize_path(path)
                == coflow_project::normalize_path(&schema_path)
    ));

    let cli = coflow()
        .args([
            "cft",
            "check",
            project_dir.to_str().expect("utf8 project path"),
            "--json",
        ])
        .output()
        .expect("run cft check");
    assert!(!cli.status.success());
    let cli_json: Value = serde_json::from_slice(&cli.stdout).expect("parse CLI diagnostics");
    let cli_diagnostic = cli_json["diagnostics"]
        .as_array()
        .expect("CLI diagnostics")
        .iter()
        .find(|diagnostic| diagnostic["code"] == expected.code)
        .expect("CLI canonical diagnostic");

    let mut child = coflow()
        .args(["lsp", project_dir.to_str().expect("utf8 project path")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn coflow lsp");
    let mut stdin = child.stdin.take().expect("lsp stdin");
    let mut stdout = child.stdout.take().expect("lsp stdout");
    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        }),
    );
    read_lsp_response(&mut stdout, 1);
    let canonical_schema = std::fs::canonicalize(&schema_path).expect("canonical schema path");
    let uri = file_uri(&canonical_schema);
    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": uri,
                    "languageId": "cft",
                    "version": 1,
                    "text": source
                }
            }
        }),
    );
    let publication = read_lsp(&mut stdout);
    let lsp_diagnostic = publication["params"]["diagnostics"]
        .as_array()
        .expect("LSP diagnostics")
        .iter()
        .find(|diagnostic| diagnostic["code"] == expected.code)
        .expect("LSP canonical diagnostic");

    assert_eq!(cli_diagnostic["message"], expected.message);
    assert_eq!(cli_diagnostic["stage"], expected.stage);
    assert_eq!(cli_diagnostic["severity"], "error");
    assert_eq!(lsp_diagnostic["message"], expected.message);
    assert_eq!(
        lsp_diagnostic["source"],
        format!("coflow {}", expected.stage)
    );
    assert_eq!(lsp_diagnostic["severity"], 1);
    assert_eq!(cli_diagnostic["startLine"], expected_range.start.line);
    assert_eq!(
        cli_diagnostic["startCharacter"],
        expected_range.start.character
    );
    assert_eq!(
        lsp_diagnostic["range"]["start"]["line"],
        expected_range.start.line
    );
    assert_eq!(
        lsp_diagnostic["range"]["start"]["character"],
        expected_range.start.character
    );
    assert_eq!(
        lsp_diagnostic["range"]["end"]["line"],
        expected_range.end.line
    );
    assert_eq!(
        lsp_diagnostic["range"]["end"]["character"],
        expected_range.end.character
    );

    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "shutdown",
            "params": null
        }),
    );
    read_lsp_response(&mut stdout, 2);
    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "exit",
            "params": null
        }),
    );
    assert_child_exits(&mut child);
}

#[test]
fn lsp_publishes_project_diagnostics_for_open_document() {
    let suffix = unique_suffix();
    let project_dir = std::env::temp_dir().join(format!("coflow-lsp-project-diagnostics-{suffix}"));
    let _cleanup = TempDirCleanup(project_dir.clone());
    let schema_dir = project_dir.join("schema");
    std::fs::create_dir_all(&schema_dir).expect("create schema dir");
    std::fs::write(project_dir.join("coflow.yaml"), "schema: schema/\n").expect("write config");
    let schema_path = schema_dir.join("main.cft");
    std::fs::write(&schema_path, "type Item { name: string; }\n").expect("write schema");

    let mut child = coflow()
        .args(["lsp", project_dir.to_str().expect("utf8 temp path")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn coflow lsp");

    let mut stdin = child.stdin.take().expect("lsp stdin");
    let mut stdout = child.stdout.take().expect("lsp stdout");

    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        }),
    );
    let initialize = read_lsp_response(&mut stdout, 1);
    assert_eq!(initialize["id"], 1);
    assert!(initialize["result"]["capabilities"]["textDocumentSync"].is_object());

    let schema_path = std::fs::canonicalize(&schema_path).expect("schema path");
    let uri = file_uri(&schema_path);
    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": uri,
                    "languageId": "cft",
                    "version": 1,
                    "text": "type Broken { missing: Missing; }"
                }
            }
        }),
    );
    let publish = read_lsp(&mut stdout);
    assert_eq!(publish["method"], "textDocument/publishDiagnostics");
    assert_eq!(publish["params"]["uri"], uri);
    let diagnostics = publish["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic["code"] == "CFT-SCHEMA-006"),
        "diagnostics: {diagnostics:?}"
    );

    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "shutdown",
            "params": null
        }),
    );
    let shutdown = read_lsp_response(&mut stdout, 2);
    assert_eq!(shutdown["id"], 2);
    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "exit",
            "params": null
        }),
    );
    assert_child_exits(&mut child);
}

#[test]
fn root_lsp_starts_language_server() {
    let suffix = unique_suffix();
    let project_dir = std::env::temp_dir().join(format!("coflow-root-lsp-starts-{suffix}"));
    let _cleanup = TempDirCleanup(project_dir.clone());
    let schema_dir = project_dir.join("schema");
    std::fs::create_dir_all(&schema_dir).expect("create schema dir");
    std::fs::write(project_dir.join("coflow.yaml"), "schema: schema/\n").expect("write config");
    std::fs::write(schema_dir.join("main.cft"), "type Item { key: string; }\n")
        .expect("write schema");

    let mut child = coflow()
        .args(["lsp", project_dir.to_str().expect("utf8 temp path")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn root coflow lsp");

    let mut stdin = child.stdin.take().expect("lsp stdin");
    let mut stdout = child.stdout.take().expect("lsp stdout");

    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        }),
    );
    let initialize = read_lsp_response(&mut stdout, 1);
    let capabilities = &initialize["result"]["capabilities"];
    assert!(capabilities["completionProvider"].is_object());
    assert_eq!(capabilities["definitionProvider"], true);
    assert!(capabilities["semanticTokensProvider"].is_object());

    shutdown_lsp(stdin, &mut stdout, &mut child, 2);
}

#[test]
fn lsp_feature_request_waits_for_latest_rapid_change_revision() {
    let suffix = unique_suffix();
    let project_dir = std::env::temp_dir().join(format!("coflow-lsp-latest-revision-{suffix}"));
    let _cleanup = TempDirCleanup(project_dir.clone());
    let schema_dir = project_dir.join("schema");
    std::fs::create_dir_all(&schema_dir).expect("create schema dir");
    std::fs::write(project_dir.join("coflow.yaml"), "schema: schema/\n").expect("write config");
    let schema_path = schema_dir.join("main.cft");
    std::fs::write(&schema_path, "type First {}\n").expect("write schema");

    let mut child = coflow()
        .args(["lsp", project_dir.to_str().expect("utf8 temp path")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn coflow lsp");
    let mut stdin = child.stdin.take().expect("lsp stdin");
    let mut stdout = child.stdout.take().expect("lsp stdout");
    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        }),
    );
    read_lsp_response(&mut stdout, 1);

    let uri = file_uri(&std::fs::canonicalize(&schema_path).expect("schema path"));
    let latest = "type Third {}\ntype Holder { target: Third; }\n";
    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": { "textDocument": {
                "uri": uri, "languageId": "cft", "version": 1, "text": "type First {}\n"
            }}
        }),
    );
    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didChange",
            "params": {
                "textDocument": { "uri": uri, "version": 2 },
                "contentChanges": [{ "text": "type Broken { missing: Missing; }\n" }]
            }
        }),
    );
    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didChange",
            "params": {
                "textDocument": { "uri": uri, "version": 3 },
                "contentChanges": [{ "text": latest }]
            }
        }),
    );

    let definition = request_definition_at(
        &mut stdin,
        &mut stdout,
        2,
        &uri,
        latest,
        position_after(latest, "target: Third"),
    );
    let location = definition
        .as_array()
        .and_then(|locations| locations.first())
        .expect("latest definition location");
    assert_eq!(location["uri"], uri);
    assert_eq!(location["range"]["start"]["line"], 0);
    assert_eq!(location["range"]["start"]["character"], 5);
    assert_eq!(location["range"]["end"]["character"], 10);

    shutdown_lsp(stdin, &mut stdout, &mut child, 3);
}

#[test]
fn lsp_prefers_open_document_uri_for_project_diagnostics() {
    let suffix = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos()
    );
    let project_dir = std::env::temp_dir().join(format!("coflow lsp uri alias test {suffix}"));
    let schema_dir = project_dir.join("schema");
    if project_dir.exists() {
        std::fs::remove_dir_all(&project_dir).expect("clean old temp dir");
    }
    std::fs::create_dir_all(&schema_dir).expect("create schema dir");
    std::fs::write(project_dir.join("coflow.yaml"), "schema: schema/\n").expect("write config");
    let schema_path = schema_dir.join("main.cft");
    std::fs::write(&schema_path, "type Item { key: string; }\n").expect("write schema");

    let mut child = coflow()
        .args(["lsp", project_dir.to_str().expect("utf8 temp path")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn coflow lsp");

    let mut stdin = child.stdin.take().expect("lsp stdin");
    let mut stdout = child.stdout.take().expect("lsp stdout");

    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        }),
    );
    let initialize = read_lsp_response(&mut stdout, 1);
    assert_eq!(initialize["id"], 1);

    let schema_path = std::fs::canonicalize(&schema_path).expect("schema path");
    let uri = file_uri(&schema_path);
    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": uri,
                    "languageId": "cft",
                    "version": 1,
                    "text": "type Broken { missing: Missing; }"
                }
            }
        }),
    );

    let publish = read_lsp(&mut stdout);
    assert_eq!(publish["method"], "textDocument/publishDiagnostics");
    assert_eq!(publish["params"]["uri"], uri);
    let diagnostics = publish["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic["code"] == "CFT-SCHEMA-006"),
        "diagnostics: {diagnostics:?}"
    );

    shutdown_lsp(stdin, &mut stdout, &mut child, 2);
    std::fs::remove_dir_all(project_dir).expect("clean temp dir");
}

#[test]
fn lsp_definitions_survive_unrelated_schema_diagnostics() {
    let suffix = unique_suffix();
    let project_dir = std::env::temp_dir().join(format!("coflow-lsp-definition-test-{suffix}"));
    let _cleanup = TempDirCleanup(project_dir.clone());
    let schema_dir = project_dir.join("schema");
    std::fs::create_dir_all(&schema_dir).expect("create schema dir");
    std::fs::write(project_dir.join("coflow.yaml"), "schema: schema/\n").expect("write config");
    let source_path = schema_dir.join("source.cft");
    let target_path = schema_dir.join("target.cft");
    let broken_path = schema_dir.join("broken.cft");
    let source = "type UsesTarget { target: Target; }\n";
    let target = "type Target { key: string; }\n";
    std::fs::write(&source_path, source).expect("write source schema");
    std::fs::write(&target_path, target).expect("write target schema");
    std::fs::write(&broken_path, "type Broken { missing: Missing; }\n")
        .expect("write broken schema");

    let mut child = coflow()
        .args(["lsp", project_dir.to_str().expect("utf8 temp path")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn coflow lsp");

    let mut stdin = child.stdin.take().expect("lsp stdin");
    let mut stdout = child.stdout.take().expect("lsp stdout");

    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        }),
    );
    let initialize = read_lsp_response(&mut stdout, 1);
    assert_eq!(initialize["id"], 1);

    let source_path = std::fs::canonicalize(&source_path).expect("source path");
    let source_uri = file_uri(&source_path);
    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": source_uri,
                    "languageId": "cft",
                    "version": 1,
                    "text": source
                }
            }
        }),
    );
    let publish = read_lsp(&mut stdout);
    assert_eq!(publish["method"], "textDocument/publishDiagnostics");

    let definitions = request_definition_at(
        &mut stdin,
        &mut stdout,
        2,
        &source_uri,
        source,
        position_after(source, "target: Target"),
    );
    let definitions = definitions.as_array().expect("definition array");
    assert!(
        definitions.iter().any(|location| {
            location["uri"]
                .as_str()
                .is_some_and(|uri| uri.ends_with("target.cft"))
        }),
        "definitions: {definitions:?}"
    );

    shutdown_lsp(stdin, &mut stdout, &mut child, 3);
}

#[test]
fn lsp_enum_variant_definitions_survive_unrelated_schema_diagnostics() {
    let suffix = unique_suffix();
    let project_dir =
        std::env::temp_dir().join(format!("coflow-lsp-enum-definition-test-{suffix}"));
    let _cleanup = TempDirCleanup(project_dir.clone());
    let schema_dir = project_dir.join("schema");
    std::fs::create_dir_all(&schema_dir).expect("create schema dir");
    std::fs::write(project_dir.join("coflow.yaml"), "schema: schema/\n").expect("write config");
    let source_path = schema_dir.join("source.cft");
    let broken_path = schema_dir.join("broken.cft");
    let source = r#"enum ExampleRarity {
  Common = 0,
  Rare = 10,
}

type UsesEnum {
  rarity: ExampleRarity = ExampleRarity.Common;
  check {
    rarity >= ExampleRarity.Common;
  }
}
"#;
    std::fs::write(&source_path, source).expect("write source schema");
    std::fs::write(&broken_path, "type Broken { missing: Missing; }\n")
        .expect("write broken schema");

    let mut child = coflow()
        .args(["lsp", project_dir.to_str().expect("utf8 temp path")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn coflow lsp");

    let mut stdin = child.stdin.take().expect("lsp stdin");
    let mut stdout = child.stdout.take().expect("lsp stdout");

    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        }),
    );
    let initialize = read_lsp_response(&mut stdout, 1);
    assert_eq!(initialize["id"], 1);

    let source_path = std::fs::canonicalize(&source_path).expect("source path");
    let source_uri = file_uri(&source_path);
    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": source_uri,
                    "languageId": "cft",
                    "version": 1,
                    "text": source
                }
            }
        }),
    );
    let publish = read_lsp(&mut stdout);
    assert_eq!(publish["method"], "textDocument/publishDiagnostics");

    let definitions = request_definition_at(
        &mut stdin,
        &mut stdout,
        2,
        &source_uri,
        source,
        position_after(source, "rarity >= ExampleRarity.Common"),
    );
    let definitions = definitions.as_array().expect("definition array");
    assert!(
        definitions.iter().any(|location| {
            location["uri"] == source_uri
                && location["range"]
                    == serde_json::json!({
                        "start": { "line": 1, "character": 2 },
                        "end": { "line": 1, "character": 8 }
                    })
        }),
        "definitions: {definitions:?}"
    );

    shutdown_lsp(stdin, &mut stdout, &mut child, 3);
}

#[test]
fn lsp_semantic_tokens_classify_check_enum_values() {
    let suffix = unique_suffix();
    let project_dir = std::env::temp_dir().join(format!("coflow-lsp-semantic-test-{suffix}"));
    let _cleanup = TempDirCleanup(project_dir.clone());
    let schema_dir = project_dir.join("schema");
    std::fs::create_dir_all(&schema_dir).expect("create schema dir");
    std::fs::write(project_dir.join("coflow.yaml"), "schema: schema/\n").expect("write config");
    let source_path = schema_dir.join("source.cft");
    let source = r#"enum ExampleRarity {
  Common = 0,
}

enum ExampleDamageType {
  Physical = 0,
}

@flag
enum ExamplePermission {
  Read = 1,
}

type UsesEnum {
  rarity: ExampleRarity = ExampleRarity.Common;
  damage_type: ExampleDamageType = ExampleDamageType.Physical;
  permissions: ExamplePermission = ExamplePermission.Read;
  check {
    rarity >= ExampleRarity.Common;
    damage_type != ExampleDamageType.Physical;
    (permissions & ExamplePermission.Read) != ExamplePermission(0);
  }
}
"#;
    std::fs::write(&source_path, source).expect("write source schema");

    let mut child = coflow()
        .args(["lsp", project_dir.to_str().expect("utf8 temp path")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn coflow lsp");

    let mut stdin = child.stdin.take().expect("lsp stdin");
    let mut stdout = child.stdout.take().expect("lsp stdout");

    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        }),
    );
    let initialize = read_lsp_response(&mut stdout, 1);
    assert_eq!(initialize["id"], 1);

    let source_path = std::fs::canonicalize(&source_path).expect("source path");
    let source_uri = file_uri(&source_path);
    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": source_uri,
                    "languageId": "cft",
                    "version": 1,
                    "text": source
                }
            }
        }),
    );
    let publish = read_lsp(&mut stdout);
    assert_eq!(publish["method"], "textDocument/publishDiagnostics");

    let tokens = request_semantic_tokens(&mut stdin, &mut stdout, 2, &source_uri);
    assert_semantic_token_at(
        &tokens,
        source,
        position_after(source, "rarity >= ExampleRarity"),
        TEST_SEM_ENUM,
    );
    assert_semantic_token_at(
        &tokens,
        source,
        position_after(source, "rarity >= ExampleRarity.Common"),
        TEST_SEM_ENUM_MEMBER,
    );
    assert_semantic_token_at(
        &tokens,
        source,
        position_after(source, "damage_type != ExampleDamageType"),
        TEST_SEM_ENUM,
    );
    assert_semantic_token_at(
        &tokens,
        source,
        position_after(source, "damage_type != ExampleDamageType.Physical"),
        TEST_SEM_ENUM_MEMBER,
    );
    assert_semantic_token_at(
        &tokens,
        source,
        position_after(source, "permissions & ExamplePermission.Read"),
        TEST_SEM_ENUM_MEMBER,
    );
    assert_semantic_token_at(
        &tokens,
        source,
        position_after(source, "!= ExamplePermission"),
        TEST_SEM_ENUM,
    );

    shutdown_lsp(stdin, &mut stdout, &mut child, 3);
}

#[test]
fn lsp_definitions_resolve_example_cross_file_enum_references() {
    let mut child = coflow()
        .args(["lsp", "examples/cft"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn coflow lsp");

    let mut stdin = child.stdin.take().expect("lsp stdin");
    let mut stdout = child.stdout.take().expect("lsp stdout");

    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        }),
    );
    let initialize = read_lsp_response(&mut stdout, 1);
    assert_eq!(initialize["id"], 1);

    let schema_path =
        std::fs::canonicalize("examples/cft/03_types_fields_defaults.cft").expect("schema path");
    let uri = file_uri(&schema_path);
    let source = std::fs::read_to_string(&schema_path).expect("schema source");
    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": uri,
                    "languageId": "cft",
                    "version": 1,
                    "text": source
                }
            }
        }),
    );
    let publish = read_lsp(&mut stdout);
    assert_eq!(publish["method"], "textDocument/publishDiagnostics");

    let enum_type_definition = request_definition_at(
        &mut stdin,
        &mut stdout,
        2,
        &uri,
        &source,
        position_after(&source, "rarity: ExampleRarity"),
    );
    assert_definition_uri_matches_path(
        &enum_type_definition,
        "examples/cft/02_enums_and_flags.cft",
    );

    let enum_variant_definition = request_definition_at(
        &mut stdin,
        &mut stdout,
        3,
        &uri,
        &source,
        position_after(&source, "ExampleRarity.Common"),
    );
    assert_definition_uri_matches_path(
        &enum_variant_definition,
        "examples/cft/02_enums_and_flags.cft",
    );

    let enum_type_definition_from_middle = request_definition_at(
        &mut stdin,
        &mut stdout,
        4,
        &uri,
        &source,
        position_inside(&source, "rarity: ExampleRarity", "ExampleRarity", 4),
    );
    assert_definition_uri_matches_path(
        &enum_type_definition_from_middle,
        "examples/cft/02_enums_and_flags.cft",
    );

    let enum_variant_definition_from_middle = request_definition_at(
        &mut stdin,
        &mut stdout,
        5,
        &uri,
        &source,
        position_inside(&source, "ExampleRarity.Common", "Common", 2),
    );
    assert_definition_uri_matches_path(
        &enum_variant_definition_from_middle,
        "examples/cft/02_enums_and_flags.cft",
    );

    shutdown_lsp(stdin, &mut stdout, &mut child, 6);
}

#[test]
fn lsp_serves_editor_language_features() {
    let mut child = coflow()
        .args(["lsp", "examples/rpg"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn coflow lsp");

    let mut stdin = child.stdin.take().expect("lsp stdin");
    let mut stdout = child.stdout.take().expect("lsp stdout");

    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        }),
    );
    let initialize = read_lsp_response(&mut stdout, 1);
    let capabilities = &initialize["result"]["capabilities"];
    assert!(capabilities["completionProvider"].is_object());
    assert_eq!(capabilities["documentFormattingProvider"], true);
    assert!(capabilities["semanticTokensProvider"].is_object());

    let schema_path =
        std::fs::canonicalize("examples/rpg/schema/30_monsters_drops.cft").expect("schema path");
    let uri = file_uri(&schema_path);
    let source = std::fs::read_to_string(&schema_path)
        .expect("schema source")
        .replacen("type Monster {", "type Monster { # comment", 1);
    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": uri,
                    "languageId": "cft",
                    "version": 1,
                    "text": source
                }
            }
        }),
    );
    let publish = read_lsp(&mut stdout);
    assert_eq!(publish["method"], "textDocument/publishDiagnostics");

    let top_level_completion =
        request_completion_at(&mut stdin, &mut stdout, 2, &uri, &source, source.len());
    assert_has_completion(&top_level_completion, "type");
    assert_missing_completion(&top_level_completion, "Monster");
    assert_missing_completion(&top_level_completion, "len");

    let type_ref_completion = request_completion_at(
        &mut stdin,
        &mut stdout,
        3,
        &uri,
        &source,
        position_after(&source, "stats: "),
    );
    assert_has_completion(&type_ref_completion, "Monster");
    assert_has_completion(&type_ref_completion, "int");
    assert_missing_completion(&type_ref_completion, "len");

    let field_default_completion = request_completion_at(
        &mut stdin,
        &mut stdout,
        4,
        &uri,
        &source,
        position_after(&source, "flags: SkillTag = "),
    );
    assert_has_completion(&field_default_completion, "SkillTag.Damage");
    assert_missing_completion(&field_default_completion, "true");
    assert_missing_completion(&field_default_completion, "MAX_LEVEL");
    assert_missing_completion(&field_default_completion, "len");

    let check_completion = request_completion_at(
        &mut stdin,
        &mut stdout,
        5,
        &uri,
        &source,
        position_after(&source, "    id != \"\";\n    "),
    );
    assert_missing_completion(&check_completion, "len");
    assert_has_completion(&check_completion, "id");
    assert_missing_completion(&check_completion, "Monster");

    let method_completion = request_completion_at(
        &mut stdin,
        &mut stdout,
        13,
        &uri,
        &source,
        position_after(&source, "resistances."),
    );
    assert_has_completion(&method_completion, "len");
    assert_has_completion(&method_completion, "contains");

    let string_completion = request_completion_at(
        &mut stdin,
        &mut stdout,
        14,
        &uri,
        &source,
        position_after(&source, "\"^[a-z"),
    );
    assert_no_completion(&string_completion);

    let enum_dot_completion = request_completion_at(
        &mut stdin,
        &mut stdout,
        15,
        &uri,
        &source,
        position_after(&source, "flags: SkillTag = SkillTag."),
    );
    assert_has_completion(&enum_dot_completion, "Damage");
    assert_missing_completion(&enum_dot_completion, "len");

    let field_dot_completion = request_completion_at(
        &mut stdin,
        &mut stdout,
        16,
        &uri,
        &source,
        position_after(&source, "stats."),
    );
    assert_has_completion(&field_dot_completion, "hp");
    assert_missing_completion(&field_dot_completion, "Monster");

    let type_predicate_completion = request_completion_at(
        &mut stdin,
        &mut stdout,
        17,
        &uri,
        &source,
        position_after(&source, "reward is "),
    );
    assert_has_completion(&type_predicate_completion, "ItemReward");
    assert_has_completion(&type_predicate_completion, "null");
    assert_missing_completion(&type_predicate_completion, "len");

    let modifier_keyword_completion = request_completion_at(
        &mut stdin,
        &mut stdout,
        18,
        &uri,
        &source,
        position_after(&source, "sealed "),
    );
    assert_has_completion(&modifier_keyword_completion, "type");
    assert_missing_completion(&modifier_keyword_completion, "enum");

    let comment_completion = request_completion_at(
        &mut stdin,
        &mut stdout,
        19,
        &uri,
        &source,
        position_after(&source, "# comment"),
    );
    assert_no_completion(&comment_completion);

    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 20,
            "method": "textDocument/formatting",
            "params": {
                "textDocument": { "uri": uri },
                "options": { "tabSize": 2, "insertSpaces": true }
            }
        }),
    );
    let formatting = read_lsp_response(&mut stdout, 20);
    assert!(
        formatting["result"].is_array(),
        "formatting: {formatting:?}"
    );

    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 21,
            "method": "textDocument/semanticTokens/full",
            "params": {
                "textDocument": { "uri": uri }
            }
        }),
    );
    let semantic = read_lsp_response(&mut stdout, 21);
    assert!(
        semantic["result"]["data"]
            .as_array()
            .is_some_and(|data| !data.is_empty()),
        "semantic: {semantic:?}"
    );

    shutdown_lsp(stdin, &mut stdout, &mut child, 22);
}
