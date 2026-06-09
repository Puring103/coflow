#![allow(
    clippy::expect_used,
    clippy::needless_raw_string_hashes,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::too_many_lines,
    clippy::unwrap_used
)]

use serde_json::Value;
use std::io::{Read, Write};
use std::process::{ChildStdout, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

fn coflow() -> Command {
    Command::new(env!("CARGO_BIN_EXE_coflow"))
}

#[test]
fn cft_check_uses_project_config_and_json_output() {
    let output = coflow()
        .args(["cft", "check", "examples/rpg", "--json"])
        .output()
        .expect("run coflow");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        r#"{"diagnostics":[]}"#
    );
}

#[test]
fn full_project_check_loads_example_excel() {
    let output = coflow()
        .args(["check", "examples/rpg"])
        .output()
        .expect("run coflow");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("Project check passed"),
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn export_json_validates_declared_output_type() {
    let out_dir =
        std::env::temp_dir().join(format!("coflow-json-export-test-{}", std::process::id()));
    if out_dir.exists() {
        std::fs::remove_dir_all(&out_dir).expect("clean old output dir");
    }

    let output = coflow()
        .args([
            "export",
            "json",
            "examples/rpg",
            "--out",
            out_dir.to_str().expect("utf8 temp path"),
        ])
        .output()
        .expect("run coflow");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("JSON data exported to"),
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    let drop_table = std::fs::read_to_string(out_dir.join("DropTable.json"))
        .expect("DropTable.json should be written");
    assert!(drop_table.contains(r#""$type": "ItemReward""#));
    assert!(drop_table.contains(r#""monster_id": "goblin_warrior""#));
    std::fs::remove_dir_all(out_dir).expect("clean output dir");
}

#[test]
fn codegen_csharp_writes_newtonsoft_json_loader() {
    let out_dir =
        std::env::temp_dir().join(format!("coflow-csharp-codegen-test-{}", std::process::id()));
    if out_dir.exists() {
        std::fs::remove_dir_all(&out_dir).expect("clean old output dir");
    }

    let output = coflow()
        .args([
            "codegen",
            "csharp",
            "examples/rpg",
            "--out",
            out_dir.to_str().expect("utf8 temp path"),
        ])
        .output()
        .expect("run coflow");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("C# code generated to"),
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    let game_config =
        std::fs::read_to_string(out_dir.join("GameConfig.cs")).expect("GameConfig.cs");
    assert!(game_config.contains("using Newtonsoft.Json.Linq;"));
    assert!(game_config.contains("DuplicatePropertyNameHandling.Error"));
    assert!(game_config.contains("LoadRewardPolymorphic"));
    assert!(game_config.contains("ResolveRewardRefs(value.Rewards[i]"));

    let item_reward =
        std::fs::read_to_string(out_dir.join("ItemReward.cs")).expect("ItemReward.cs");
    assert!(item_reward.contains("public string ItemId { get; init; }"));
    assert!(item_reward.contains("public Item Item { get; internal set; }"));

    std::fs::remove_dir_all(out_dir).expect("clean output dir");
}

#[test]
fn cft_lsp_publishes_project_diagnostics_for_open_document() {
    let mut child = coflow()
        .args(["cft", "lsp", "examples/rpg"])
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

    let schema_path = std::fs::canonicalize("examples/rpg/schema/rpg.cft").expect("schema path");
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
fn cft_lsp_serves_editor_language_features() {
    let mut child = coflow()
        .args(["cft", "lsp", "examples/rpg"])
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

    let schema_path = std::fs::canonicalize("examples/rpg/schema/rpg.cft").expect("schema path");
    let uri = file_uri(&schema_path);
    let source = std::fs::read_to_string(&schema_path)
        .expect("schema source")
        .replacen(
            "const MAX_LEVEL: int = 100;",
            "const MAX_LEVEL: int = 100; # comment",
            1,
        );
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

    let top_level_completion = request_completion_at(&mut stdin, &mut stdout, 2, &uri, &source, 0);
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
        position_after(&source, "rarity: Rarity = "),
    );
    assert_has_completion(&field_default_completion, "Rarity.Common");
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
    assert_has_completion(&check_completion, "len");
    assert_has_completion(&check_completion, "id");
    assert_missing_completion(&check_completion, "Monster");

    let ref_completion = request_completion_at(
        &mut stdin,
        &mut stdout,
        6,
        &uri,
        &source,
        position_after(&source, "@ref("),
    );
    assert_has_completion(&ref_completion, "Monster");
    assert_missing_completion(&ref_completion, "int");
    assert_missing_completion(&ref_completion, "Rarity");

    let string_completion = request_completion_at(
        &mut stdin,
        &mut stdout,
        7,
        &uri,
        &source,
        position_after(&source, "@display(\"Item"),
    );
    assert_no_completion(&string_completion);

    let enum_dot_completion = request_completion_at(
        &mut stdin,
        &mut stdout,
        8,
        &uri,
        &source,
        position_after(&source, "rarity: Rarity = Rarity."),
    );
    assert_has_completion(&enum_dot_completion, "Common");
    assert_missing_completion(&enum_dot_completion, "len");

    let field_dot_completion = request_completion_at(
        &mut stdin,
        &mut stdout,
        9,
        &uri,
        &source,
        position_after(&source, "stats."),
    );
    assert_has_completion(&field_dot_completion, "hp");
    assert_missing_completion(&field_dot_completion, "Monster");

    let type_predicate_completion = request_completion_at(
        &mut stdin,
        &mut stdout,
        10,
        &uri,
        &source,
        position_after(&source, "reward is "),
    );
    assert_has_completion(&type_predicate_completion, "ItemReward");
    assert_has_completion(&type_predicate_completion, "null");
    assert_missing_completion(&type_predicate_completion, "len");

    let parent_type_completion = request_completion_at(
        &mut stdin,
        &mut stdout,
        11,
        &uri,
        &source,
        position_after(&source, "type ItemReward : "),
    );
    assert_has_completion(&parent_type_completion, "Reward");
    assert_missing_completion(&parent_type_completion, "int");

    let abstract_keyword_completion = request_completion_at(
        &mut stdin,
        &mut stdout,
        12,
        &uri,
        &source,
        position_after(&source, "abstract "),
    );
    assert_has_completion(&abstract_keyword_completion, "type");
    assert_missing_completion(&abstract_keyword_completion, "enum");

    let comment_completion = request_completion_at(
        &mut stdin,
        &mut stdout,
        13,
        &uri,
        &source,
        position_after(&source, "# comment"),
    );
    assert_no_completion(&comment_completion);

    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 14,
            "method": "textDocument/formatting",
            "params": {
                "textDocument": { "uri": uri },
                "options": { "tabSize": 2, "insertSpaces": true }
            }
        }),
    );
    let formatting = read_lsp_response(&mut stdout, 14);
    assert!(
        formatting["result"].is_array(),
        "formatting: {formatting:?}"
    );

    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 15,
            "method": "textDocument/semanticTokens/full",
            "params": {
                "textDocument": { "uri": uri }
            }
        }),
    );
    let semantic = read_lsp_response(&mut stdout, 15);
    assert!(
        semantic["result"]["data"]
            .as_array()
            .is_some_and(|data| !data.is_empty()),
        "semantic: {semantic:?}"
    );

    shutdown_lsp(stdin, &mut stdout, &mut child, 16);
}

fn request_completion(
    stdin: &mut impl Write,
    stdout: &mut ChildStdout,
    id: u64,
    uri: &str,
    line: u64,
    character: u64,
) -> Value {
    write_lsp(
        stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/completion",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }
        }),
    );
    read_lsp_response(stdout, id)["result"].clone()
}

fn request_completion_at(
    stdin: &mut impl Write,
    stdout: &mut ChildStdout,
    id: u64,
    uri: &str,
    source: &str,
    byte_offset: usize,
) -> Value {
    let (line, character) = lsp_position(source, byte_offset);
    request_completion(stdin, stdout, id, uri, line, character)
}

fn position_after(source: &str, needle: &str) -> usize {
    let start = source
        .find(needle)
        .unwrap_or_else(|| panic!("source should contain `{needle}`"));
    start + needle.len()
}

fn lsp_position(source: &str, byte_offset: usize) -> (u64, u64) {
    let target = byte_offset.min(source.len());
    let mut line = 0_u64;
    let mut character = 0_u64;
    for (index, ch) in source.char_indices() {
        if index >= target {
            break;
        }
        if ch == '\n' {
            line += 1;
            character = 0;
        } else {
            character += ch.len_utf16() as u64;
        }
    }
    (line, character)
}

fn assert_has_completion(completion: &Value, label: &str) {
    let items = completion.as_array().expect("completion array");
    assert!(
        items.iter().any(|item| item["label"] == label),
        "expected completion `{label}` in {items:?}"
    );
}

fn assert_missing_completion(completion: &Value, label: &str) {
    let items = completion.as_array().expect("completion array");
    assert!(
        !items.iter().any(|item| item["label"] == label),
        "unexpected completion `{label}` in {items:?}"
    );
}

fn assert_no_completion(completion: &Value) {
    let items = completion.as_array().expect("completion array");
    assert!(items.is_empty(), "expected no completion in {items:?}");
}

fn write_lsp(stdin: &mut impl Write, value: &Value) {
    let body = serde_json::to_vec(value).expect("serialize lsp message");
    write!(stdin, "Content-Length: {}\r\n\r\n", body.len()).expect("write lsp header");
    stdin.write_all(&body).expect("write lsp body");
    stdin.flush().expect("flush lsp");
}

fn shutdown_lsp(
    mut stdin: impl Write,
    stdout: &mut ChildStdout,
    child: &mut std::process::Child,
    id: u64,
) {
    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "shutdown",
            "params": null
        }),
    );
    let shutdown = read_lsp_response(stdout, id);
    assert_eq!(shutdown["id"], id);
    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "exit",
            "params": null
        }),
    );
    stdin.flush().expect("flush exit");
    assert_child_exits(child);
}

fn assert_child_exits(child: &mut std::process::Child) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().expect("poll lsp") {
            assert!(status.success(), "lsp exit status: {status}");
            return;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("lsp did not exit after exit notification");
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn read_lsp_response(stdout: &mut ChildStdout, id: u64) -> Value {
    loop {
        let message = read_lsp(stdout);
        if message["id"] == id {
            return message;
        }
    }
}

fn read_lsp(stdout: &mut ChildStdout) -> Value {
    let mut header = Vec::new();
    let mut byte = [0; 1];

    while !header.ends_with(b"\r\n\r\n") {
        stdout.read_exact(&mut byte).expect("read lsp header");
        header.push(byte[0]);
    }

    let header = String::from_utf8(header).expect("utf8 header");
    let content_length = header
        .lines()
        .find_map(|line| line.strip_prefix("Content-Length:"))
        .expect("content length")
        .trim()
        .parse::<usize>()
        .expect("parse content length");
    let mut body = vec![0; content_length];
    stdout.read_exact(&mut body).expect("read lsp body");
    serde_json::from_slice(&body).expect("parse lsp body")
}

fn file_uri(path: &std::path::Path) -> String {
    let mut path = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        if let Some(stripped) = path.strip_prefix("//?/") {
            path = stripped.to_string();
        }
    }
    if cfg!(windows) && path.len() >= 2 && path.as_bytes()[1] == b':' {
        path.insert(0, '/');
    }
    format!("file://{path}")
}
