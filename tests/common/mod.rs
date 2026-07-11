#![allow(dead_code)]

pub use serde_json::Value;
use std::io::{Read, Write};
use std::process::ChildStdout;
pub use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

pub const TEST_SEM_ENUM: u64 = 2;
pub const TEST_SEM_ENUM_MEMBER: u64 = 3;

pub fn coflow() -> Command {
    Command::new(env!("CARGO_BIN_EXE_coflow"))
}

pub fn unique_suffix() -> String {
    format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos()
    )
}

pub fn temp_project_dir(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("coflow-{name}-{}", unique_suffix()));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("clean old temp dir");
    }
    root
}

pub fn active_artifact_manifest(project_root: &std::path::Path) -> Value {
    let path = project_root.join(".coflow/artifacts/active.json");
    let contents = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read active artifact manifest `{}`: {err}", path.display()));
    serde_json::from_str(&contents)
        .unwrap_or_else(|err| panic!("parse active artifact manifest `{}`: {err}", path.display()))
}

pub fn active_artifact_dir(project_root: &std::path::Path, slot: &str) -> std::path::PathBuf {
    let manifest = active_artifact_manifest(project_root);
    let path = manifest["outputs"][slot]["generation_dir"]
        .as_str()
        .unwrap_or_else(|| panic!("active artifact manifest has no `{slot}` generation"));
    std::path::PathBuf::from(path)
}

pub fn active_enum_lock(project_root: &std::path::Path) -> Value {
    active_artifact_manifest(project_root)["enum_lock"].clone()
}

pub fn write_active_enum_lock(project_root: &std::path::Path, lock: &Value) {
    let state_dir = project_root.join(".coflow/artifacts");
    std::fs::create_dir_all(&state_dir).expect("create artifact state directory");
    std::fs::write(
        state_dir.join("active.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "version": 1,
            "revision": "test-fixture",
            "outputs": {},
            "enum_lock": lock,
        }))
        .expect("serialize active artifact fixture"),
    )
    .expect("write active artifact fixture");
}

pub fn write_invalid_check_project(
    root: &std::path::Path,
) -> Result<(), rust_xlsxwriter::XlsxError> {
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        r"
            type Item {
                level: int;
                check { level > 0; }
            }
        ",
    )
    .expect("write schema");
    let workbook_path = root.join("data").join("configs.xlsx");
    let mut workbook = rust_xlsxwriter::Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.set_name("Item")?;
    sheet.write_string(0, 0, "id")?;
    sheet.write_string(0, 1, "level")?;
    sheet.write_string(1, 0, "item_1")?;
    sheet.write_number(1, 1, 0.0)?;
    workbook.save(&workbook_path)?;
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - path: data/configs.xlsx
    sheets:
      - sheet: Item
        columns:
          id: id
          level: level
outputs:
  data:
    type: json
    dir: generated/data
",
    )
    .expect("write config");
    Ok(())
}

pub fn write_acyclic_csharp_project(root: &std::path::Path, data_format: &str) {
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        r"
            type Reward {
                amount: int;
            }

            type Item {
                display_name: string;
                reward: &Reward;
                tags: [string] = [];
            }

            type Bundle {
                item: &Item;
                maybe_reward: &Reward?;
            }

            type EmptyThing {
                value: int;
            }
        ",
    )
    .expect("write schema");
    std::fs::write(
        root.join("data").join("records.cfd"),
        r#"
            reward_small: Reward {
                amount: 25,
            }

            potion: Item {
                display_name: "Potion",
                reward: &reward_small,
                tags: ["consumable"],
            }

            starter: Bundle {
                item: &potion,
                maybe_reward: &reward_small,
            }
        "#,
    )
    .expect("write data");
    std::fs::write(
        root.join("coflow.yaml"),
        format!(
            r"schema: schema.cft
sources:
  - path: data
outputs:
  data:
    type: {data_format}
    dir: generated/data
  code:
    type: csharp
    dir: generated/csharp
    namespace: Game.Config
"
        ),
    )
    .expect("write config");
}

pub struct TempDirCleanup(pub std::path::PathBuf);

impl Drop for TempDirCleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn request_completion(
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

pub fn request_completion_at(
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

pub fn request_definition(
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
            "method": "textDocument/definition",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }
        }),
    );
    read_lsp_response(stdout, id)["result"].clone()
}

pub fn request_definition_at(
    stdin: &mut impl Write,
    stdout: &mut ChildStdout,
    id: u64,
    uri: &str,
    source: &str,
    byte_offset: usize,
) -> Value {
    let (line, character) = lsp_position(source, byte_offset);
    request_definition(stdin, stdout, id, uri, line, character)
}

pub fn request_semantic_tokens(
    stdin: &mut impl Write,
    stdout: &mut ChildStdout,
    id: u64,
    uri: &str,
) -> Vec<TestSemanticToken> {
    write_lsp(
        stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/semanticTokens/full",
            "params": {
                "textDocument": { "uri": uri }
            }
        }),
    );
    let response = read_lsp_response(stdout, id);
    let data = response["result"]["data"]
        .as_array()
        .unwrap_or_else(|| panic!("semantic token data: {response:?}"));
    assert_eq!(data.len() % 5, 0, "semantic token data: {data:?}");

    let mut line = 0_u64;
    let mut character = 0_u64;
    let mut tokens = Vec::new();
    for chunk in data.chunks(5) {
        let delta_line = chunk[0].as_u64().expect("delta line");
        let delta_start = chunk[1].as_u64().expect("delta start");
        line += delta_line;
        if delta_line == 0 {
            character += delta_start;
        } else {
            character = delta_start;
        }
        tokens.push(TestSemanticToken {
            line,
            character,
            length: chunk[2].as_u64().expect("length"),
            token_type: chunk[3].as_u64().expect("token type"),
        });
    }
    tokens
}

#[derive(Debug)]
pub struct TestSemanticToken {
    pub line: u64,
    pub character: u64,
    pub length: u64,
    pub token_type: u64,
}

pub fn assert_semantic_token_at(
    tokens: &[TestSemanticToken],
    source: &str,
    byte_offset: usize,
    token_type: u64,
) {
    let (line, character) = lsp_position(source, byte_offset);
    assert!(
        tokens.iter().any(|token| {
            token.line == line
                && token.character <= character
                && character <= token.character + token.length
                && token.token_type == token_type
        }),
        "expected token type {token_type} at {line}:{character} in {tokens:?}"
    );
}

pub fn assert_definition_uri_matches_path(definitions: &Value, path: &str) {
    let path = std::fs::canonicalize(path).expect("definition target path");
    let expected = file_uri(&path);
    let definitions = definitions.as_array().expect("definition array");
    assert!(
        definitions
            .iter()
            .any(|location| location["uri"].as_str() == Some(expected.as_str())),
        "expected definition URI `{expected}` in {definitions:?}"
    );
}

pub fn position_after(source: &str, needle: &str) -> usize {
    find_line_ending_insensitive(source, needle)
        .unwrap_or_else(|| panic!("source should contain `{needle}`"))
}

pub fn position_inside(
    source: &str,
    context: &str,
    needle: &str,
    character_offset: usize,
) -> usize {
    let context_end = position_after(source, context);
    let context_start = context_end - context.len();
    let relative = context
        .find(needle)
        .unwrap_or_else(|| panic!("context `{context}` should contain `{needle}`"));
    context_start + relative + character_offset.min(needle.len())
}

pub fn find_line_ending_insensitive(source: &str, needle: &str) -> Option<usize> {
    let source_bytes = source.as_bytes();
    for start in source.char_indices().map(|(index, _)| index) {
        let mut source_index = start;
        let mut needle_index = 0;
        while needle_index < needle.len() {
            let needle_char = needle[needle_index..].chars().next()?;
            if needle_char == '\n'
                && source_bytes.get(source_index) == Some(&b'\r')
                && source_bytes.get(source_index + 1) == Some(&b'\n')
            {
                source_index += 2;
                needle_index += 1;
                continue;
            }
            let source_char = source[source_index..].chars().next()?;
            if source_char != needle_char {
                break;
            }
            source_index += source_char.len_utf8();
            needle_index += needle_char.len_utf8();
        }
        if needle_index == needle.len() {
            return Some(source_index);
        }
    }
    None
}

pub fn lsp_position(source: &str, byte_offset: usize) -> (u64, u64) {
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

pub fn assert_has_completion(completion: &Value, label: &str) {
    let items = completion.as_array().expect("completion array");
    assert!(
        items.iter().any(|item| item["label"] == label),
        "expected completion `{label}` in {items:?}"
    );
}

pub fn assert_missing_completion(completion: &Value, label: &str) {
    let items = completion.as_array().expect("completion array");
    assert!(
        !items.iter().any(|item| item["label"] == label),
        "unexpected completion `{label}` in {items:?}"
    );
}

pub fn assert_no_completion(completion: &Value) {
    let items = completion.as_array().expect("completion array");
    assert!(items.is_empty(), "expected no completion in {items:?}");
}

pub fn write_lsp(stdin: &mut impl Write, value: &Value) {
    let body = serde_json::to_vec(value).expect("serialize lsp message");
    write!(stdin, "Content-Length: {}\r\n\r\n", body.len()).expect("write lsp header");
    stdin.write_all(&body).expect("write lsp body");
    stdin.flush().expect("flush lsp");
}

pub fn shutdown_lsp(
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

pub fn assert_child_exits(child: &mut std::process::Child) {
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

pub fn read_lsp_response(stdout: &mut ChildStdout, id: u64) -> Value {
    loop {
        let message = read_lsp(stdout);
        if message["id"] == id {
            return message;
        }
    }
}

pub fn read_lsp(stdout: &mut ChildStdout) -> Value {
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

pub fn file_uri(path: &std::path::Path) -> String {
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

pub fn copy_dir_recursive(
    source: &std::path::Path,
    target: &std::path::Path,
) -> std::io::Result<()> {
    std::fs::create_dir_all(target)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        if source_path.is_dir() {
            copy_dir_recursive(&source_path, &target_path)?;
        } else {
            std::fs::copy(source_path, target_path)?;
        }
    }
    Ok(())
}
