use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) struct TempProject(PathBuf);

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(super) fn test_project(name: &str, source: &str) -> (TempProject, Project) {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("coflow-{name}-{suffix}"));
    let schema = root.join("schema");
    std::fs::create_dir_all(&schema).expect("create schema dir");
    std::fs::write(root.join("coflow.yaml"), "schema: schema/\n").expect("write config");
    std::fs::write(schema.join("main.cft"), source).expect("write schema");
    let project = Project::open_schema_only(Some(&root)).expect("open project");
    (TempProject(root), project)
}

pub(super) fn test_project_with_config(
    name: &str,
    source: &str,
    source_dir: &str,
) -> (TempProject, Project) {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("coflow-{name}-{suffix}"));
    let schema = root.join("schema");
    std::fs::create_dir_all(&schema).expect("create schema dir");
    std::fs::write(
        root.join("coflow.yaml"),
        format!("schema: schema/\nsources:\n  - path: {source_dir}\n"),
    )
    .expect("write config");
    std::fs::write(schema.join("main.cft"), source).expect("write schema");
    let project = Project::open_schema_only(Some(&root)).expect("open project");
    (TempProject(root), project)
}

pub(super) fn test_lsp_build(name: &str, source: &str) -> (TempProject, LspBuild) {
    let (cleanup, project) = test_project(name, source);
    let build = LspBuild::new(
        compile_schema_project_with_overrides(&project, &[]).expect("compile schema"),
        None,
    );
    (cleanup, build)
}

pub(super) fn first_document(build: &LspBuild) -> &LspDocument {
    build
        .documents
        .values()
        .next()
        .expect("document should exist")
}

pub(super) fn completion_labels(items: Vec<Value>) -> Vec<String> {
    items
        .into_iter()
        .filter_map(|item| {
            item.get("label")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect()
}

pub(super) fn s(value: &str) -> String {
    value.to_string()
}

pub(super) fn cfd_definition_result_at(
    server: &mut LspServer<Vec<u8>>,
    uri: &str,
    source: &str,
    needle: &str,
) -> Value {
    server.writer.clear();
    let offset = source.find(needle).expect("needle") + needle.len().saturating_sub(1);
    let position = position_from_byte(source, offset);
    server
        .handle_message(&json!({
            "jsonrpc": "2.0",
            "id": 100,
            "method": "textDocument/definition",
            "params": {
                "textDocument": { "uri": uri },
                "position": {
                    "line": position.line,
                    "character": position.character
                }
            }
        }))
        .expect("definition request");
    written_messages(&server.writer)
        .into_iter()
        .next()
        .expect("response")["result"]
        .clone()
}

pub(super) fn cfd_definition_result_at_context(
    server: &mut LspServer<Vec<u8>>,
    uri: &str,
    source: &str,
    context: &str,
    needle: &str,
) -> Value {
    server.writer.clear();
    let offset = position_inside(source, context, needle, needle.len().saturating_sub(1));
    let position = position_from_byte(source, offset);
    server
        .handle_message(&json!({
            "jsonrpc": "2.0",
            "id": 101,
            "method": "textDocument/definition",
            "params": {
                "textDocument": { "uri": uri },
                "position": {
                    "line": position.line,
                    "character": position.character
                }
            }
        }))
        .expect("definition request");
    written_messages(&server.writer)
        .into_iter()
        .next()
        .expect("response")["result"]
        .clone()
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct DecodedSemanticToken {
    pub(super) text: String,
    pub(super) token_type: u32,
    pub(super) modifiers: u32,
}

pub(super) fn decode_semantic_tokens(source: &str, data: &Value) -> Vec<DecodedSemanticToken> {
    let mut line = 0usize;
    let mut character = 0usize;
    let mut tokens = Vec::new();
    let Some(data) = data.as_array() else {
        return tokens;
    };

    for chunk in data.chunks(5) {
        if chunk.len() < 5 {
            break;
        }
        let delta_line = usize::try_from(chunk[0].as_u64().unwrap_or(0)).unwrap_or(usize::MAX);
        let delta_start = usize::try_from(chunk[1].as_u64().unwrap_or(0)).unwrap_or(usize::MAX);
        let length = usize::try_from(chunk[2].as_u64().unwrap_or(0)).unwrap_or(usize::MAX);
        line += delta_line;
        character = if delta_line == 0 {
            character + delta_start
        } else {
            delta_start
        };
        if let Some(text) = text_at_utf16_range(source, line, character, length) {
            tokens.push(DecodedSemanticToken {
                text,
                token_type: u32::try_from(chunk[3].as_u64().unwrap_or(0)).unwrap_or(u32::MAX),
                modifiers: u32::try_from(chunk[4].as_u64().unwrap_or(0)).unwrap_or(u32::MAX),
            });
        }
    }
    tokens
}

pub(super) fn text_at_utf16_range(
    source: &str,
    target_line: usize,
    start_character: usize,
    length: usize,
) -> Option<String> {
    let start = byte_offset_from_position(
        source,
        LspPosition {
            line: target_line,
            character: start_character,
        },
    );
    let end = byte_offset_from_position(
        source,
        LspPosition {
            line: target_line,
            character: start_character + length,
        },
    );
    source.get(start..end).map(str::to_string)
}

pub(super) fn has_semantic_token(
    source: &str,
    tokens: &[RawSemanticToken],
    context: &str,
    needle: &str,
    token_type: u32,
    token_modifiers: u32,
) -> bool {
    let offset = position_inside(source, context, needle, 0);
    let position = position_from_byte(source, offset);
    tokens.iter().any(|token| {
        token.line == position.line
            && token.character == position.character
            && token.length == needle.encode_utf16().count()
            && token.token_type == token_type
            && token.token_modifiers == token_modifiers
    })
}

pub(super) fn written_messages(bytes: &[u8]) -> Vec<Value> {
    let text = String::from_utf8(bytes.to_vec()).expect("utf8 output");
    let mut messages = Vec::new();
    let mut rest = text.as_str();
    while let Some(header_end) = rest.find("\r\n\r\n") {
        let (header, after_header) = rest.split_at(header_end);
        let body_start = header_end + 4;
        let content_length = header
            .lines()
            .find_map(|line| line.strip_prefix("Content-Length:"))
            .expect("content length")
            .trim()
            .parse::<usize>()
            .expect("parse content length");
        let body = &rest[body_start..body_start + content_length];
        messages.push(serde_json::from_str(body).expect("parse response"));
        rest = &after_header[4 + content_length..];
    }
    messages
}

pub(super) fn position_inside(
    source: &str,
    context: &str,
    needle: &str,
    character_offset: usize,
) -> usize {
    let context_start = source.find(context).expect("context");
    let needle_start = context.find(needle).expect("needle");
    context_start + needle_start + character_offset
}

// ── CFD provider tests ───────────────────────────────────────────────────
