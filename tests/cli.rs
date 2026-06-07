use serde_json::Value;
use std::io::{Read, Write};
use std::process::{ChildStdout, Command, Stdio};

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
    let initialize = read_lsp(&mut stdout);
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
    let shutdown = read_lsp(&mut stdout);
    assert_eq!(shutdown["id"], 2);
    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "exit",
            "params": null
        }),
    );
    drop(stdin);
    assert!(child.wait().expect("wait lsp").success());
}

fn write_lsp(stdin: &mut impl Write, value: &Value) {
    let body = serde_json::to_vec(value).expect("serialize lsp message");
    write!(stdin, "Content-Length: {}\r\n\r\n", body.len()).expect("write lsp header");
    stdin.write_all(&body).expect("write lsp body");
    stdin.flush().expect("flush lsp");
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
