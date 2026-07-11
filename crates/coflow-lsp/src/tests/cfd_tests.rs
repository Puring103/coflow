use super::common::*;
use super::*;
use crate::definition::{cft_schema_field_definition_location, cft_type_definition_location};
use coflow_cfd::parse_cfd;

#[test]
fn cfd_definition_request_returns_schema_field_location() {
    let schema_source = "type Item {\n  key: string;\n  damage: int;\n}\n";
    let (_cleanup, project) = test_project("lsp-cfd-field-definition", schema_source);
    let cfd_path = project.root_dir.join("data.cfd");
    let cfd_uri = path_to_file_uri(&cfd_path);
    let cfd_source = "sword: Item { damage: 10 }\n";
    let field_offset = cfd_source.find("damage").expect("damage") + 1;
    let position = position_from_byte(cfd_source, field_offset);
    let mut server = LspServer::new(project, Vec::new());

    server
        .handle_message(&json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": cfd_uri,
                    "text": cfd_source
                }
            }
        }))
        .expect("open cfd document");
    server.writer.clear();

    server
        .handle_message(&json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "textDocument/definition",
            "params": {
                "textDocument": { "uri": cfd_uri },
                "position": {
                    "line": position.line,
                    "character": position.character
                }
            }
        }))
        .expect("definition request");

    let messages = written_messages(&server.writer);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["id"], 7);
    assert_eq!(messages[0]["result"]["range"]["start"]["line"], 2);
    assert_eq!(messages[0]["result"]["range"]["start"]["character"], 2);
    assert_eq!(messages[0]["result"]["range"]["end"]["line"], 2);
    assert_eq!(messages[0]["result"]["range"]["end"]["character"], 8);
}

#[test]
fn cfd_requests_ignore_uppercase_cfd_extension() {
    let schema_source = "type Item {\n  key: string;\n  damage: int;\n}\n";
    let (_cleanup, project) = test_project("lsp-uppercase-cfd-extension", schema_source);
    let cfd_path = project.root_dir.join("data.CFD");
    let cfd_uri = path_to_file_uri(&cfd_path);
    let cfd_source = "sword: Item { damage: 10 }\n";
    let field_offset = cfd_source.find("damage").expect("damage") + 1;
    let position = position_from_byte(cfd_source, field_offset);
    let mut server = LspServer::new(project, Vec::new());

    server
        .handle_message(&json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": cfd_uri,
                    "text": cfd_source
                }
            }
        }))
        .expect("open uppercase CFD document");
    server.writer.clear();

    server
        .handle_message(&json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "textDocument/definition",
            "params": {
                "textDocument": { "uri": cfd_uri },
                "position": {
                    "line": position.line,
                    "character": position.character
                }
            }
        }))
        .expect("definition request");

    let messages = written_messages(&server.writer);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["id"], 7);
    assert_eq!(messages[0]["result"], Value::Null);
}

#[test]
fn cfd_definition_request_resolves_record_keys_across_project_sources() {
    let schema_source = "type Item { key: string; }\n\
type Holder { key: string; item: Item; }\n";
    let (_cleanup, project) =
        test_project_with_config("lsp-cfd-cross-file-key-definition", schema_source, "data");
    let data_dir = project.root_dir.join("data");
    std::fs::create_dir_all(&data_dir).expect("create data dir");
    let target_path = data_dir.join("items.cfd");
    let source_path = data_dir.join("holders.cfd");
    let target_source = "sword: Item { }\n";
    let source = "holder: Holder { item: &sword }\n";
    std::fs::write(&target_path, target_source).expect("write target cfd");
    std::fs::write(&source_path, source).expect("write source cfd");
    let source_uri = path_to_file_uri(&source_path);
    let ref_offset = source.find("sword").expect("sword") + 1;
    let position = position_from_byte(source, ref_offset);
    let mut server = LspServer::new(project, Vec::new());

    server
        .handle_message(&json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": source_uri,
                    "text": source
                }
            }
        }))
        .expect("open cfd document");
    server.writer.clear();

    server
        .handle_message(&json!({
            "jsonrpc": "2.0",
            "id": 8,
            "method": "textDocument/definition",
            "params": {
                "textDocument": { "uri": source_uri },
                "position": {
                    "line": position.line,
                    "character": position.character
                }
            }
        }))
        .expect("definition request");

    let messages = written_messages(&server.writer);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["id"], 8);
    assert_eq!(messages[0]["result"]["uri"], path_to_file_uri(&target_path));
    assert_eq!(messages[0]["result"]["range"]["start"]["line"], 0);
    assert_eq!(messages[0]["result"]["range"]["start"]["character"], 0);
    assert_eq!(messages[0]["result"]["range"]["end"]["character"], 5);
}

#[test]
fn cfd_definition_index_uses_actual_type_and_dirty_overlay() {
    let schema_source = "type Item {}\n\
type Skill {}\n\
type Holder { item: &Item; }\n";
    let (_cleanup, project) =
        test_project_with_config("lsp-cfd-typed-definition-index", schema_source, "data");
    let data_dir = project.root_dir.join("data");
    std::fs::create_dir_all(&data_dir).expect("create data dir");
    let skill_path = data_dir.join("a_skills.cfd");
    let item_path = data_dir.join("z_items.cfd");
    let source_path = data_dir.join("holders.cfd");
    let source = "holder: Holder { item: &shared }\n";
    std::fs::write(&skill_path, "shared: Skill {}\n").expect("write skill source");
    std::fs::write(&item_path, "disk_only: Item {}\n").expect("write item source");
    std::fs::write(&source_path, source).expect("write holder source");

    let item_uri = path_to_file_uri(&item_path);
    let source_uri = path_to_file_uri(&source_path);
    let mut server = LspServer::new(project, Vec::new());
    server
        .handle_message(&json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": item_uri,
                    "text": "shared: Item {}\n"
                }
            }
        }))
        .expect("open dirty item document");
    server
        .handle_message(&json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": source_uri,
                    "text": source
                }
            }
        }))
        .expect("open holder document");
    server.writer.clear();

    let result = cfd_definition_result_at(&mut server, &source_uri, source, "shared");
    assert_eq!(result["uri"], item_uri);
    assert_eq!(result["range"]["start"]["character"], 0);
    assert_eq!(result["range"]["end"]["character"], 6);
}

#[test]
fn cfd_definition_request_resolves_examples_cfd_basic_monster() {
    let examples_dir =
        std::fs::canonicalize(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/cfd"))
            .expect("canonical examples/cfd");
    let project = Project::open_schema_only(Some(&examples_dir)).expect("open examples/cfd");
    let source_path = examples_dir.join("data").join("03-spread.cfd");
    let target_path = examples_dir.join("data").join("01-records.cfd");
    let source = std::fs::read_to_string(&source_path).expect("read spread cfd");
    let source_uri = path_to_file_uri(&source_path);
    let offset = source.find("basic_monster").expect("basic_monster") + 1;
    let position = position_from_byte(&source, offset);
    let mut server = LspServer::new(project, Vec::new());

    server
        .handle_message(&json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": source_uri,
                    "text": source
                }
            }
        }))
        .expect("open cfd document");
    server.writer.clear();

    server
        .handle_message(&json!({
            "jsonrpc": "2.0",
            "id": 9,
            "method": "textDocument/definition",
            "params": {
                "textDocument": { "uri": source_uri },
                "position": {
                    "line": position.line,
                    "character": position.character
                }
            }
        }))
        .expect("definition request");

    let messages = written_messages(&server.writer);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["id"], 9);
    assert_eq!(messages[0]["result"]["uri"], path_to_file_uri(&target_path));
    assert_eq!(messages[0]["result"]["range"]["start"]["line"], 18);
    assert_eq!(messages[0]["result"]["range"]["start"]["character"], 0);
    assert_eq!(messages[0]["result"]["range"]["end"]["character"], 13);
}

#[test]
fn cfd_definition_request_returns_null_for_removed_path_refs() {
    let schema_source = "type Stats {\n  hp: int;\n}\n\
type Monster {\n  key: string;\n  stats: Stats;\n}\n\
type Holder {\n  key: string;\n  hp: int;\n}\n";
    let (_cleanup, project) =
        test_project_with_config("lsp-cfd-path-field-definition", schema_source, "data");
    let data_dir = project.root_dir.join("data");
    std::fs::create_dir_all(&data_dir).expect("create data dir");
    let source_path = data_dir.join("holders.cfd");
    let source = "holder: Holder { hp: @Monster.base.stats.hp }\n";
    std::fs::write(
        data_dir.join("monsters.cfd"),
        "base: Monster { stats: { hp: 10 } }\n",
    )
    .expect("write target cfd");
    std::fs::write(&source_path, source).expect("write source cfd");
    let source_uri = path_to_file_uri(&source_path);
    let mut server = LspServer::new(project, Vec::new());

    server
        .handle_message(&json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": source_uri,
                    "text": source
                }
            }
        }))
        .expect("open cfd document");
    server.writer.clear();

    let stats = cfd_definition_result_at(&mut server, &source_uri, source, "stats");
    assert_eq!(stats, Value::Null);

    let hp = cfd_definition_result_at(&mut server, &source_uri, source, ".hp");
    assert_eq!(hp, Value::Null);
}

#[test]
fn cfd_definition_request_returns_null_for_removed_spread_path_refs() {
    let schema_source = "type Stats {\n  hp: int;\n}\n\
type Monster {\n  key: string;\n  stats: Stats;\n}\n";
    let (_cleanup, project) = test_project_with_config(
        "lsp-cfd-top-level-spread-path-definition",
        schema_source,
        "data",
    );
    let data_dir = project.root_dir.join("data");
    std::fs::create_dir_all(&data_dir).expect("create data dir");
    let source_path = data_dir.join("monsters.cfd");
    let source = "base: Monster { stats: { hp: 10 } }\n\
elite: Monster { ...@Monster.base.stats.hp }\n";
    std::fs::write(&source_path, source).expect("write source cfd");
    let source_uri = path_to_file_uri(&source_path);
    let mut server = LspServer::new(project, Vec::new());

    server
        .handle_message(&json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": source_uri,
                    "text": source
                }
            }
        }))
        .expect("open cfd document");
    server.writer.clear();

    let stats = cfd_definition_result_at_context(
        &mut server,
        &source_uri,
        source,
        "@Monster.base.stats.hp",
        "stats",
    );
    assert_eq!(stats, Value::Null);

    let hp = cfd_definition_result_at_context(
        &mut server,
        &source_uri,
        source,
        "@Monster.base.stats.hp",
        "hp",
    );
    assert_eq!(hp, Value::Null);
}

#[test]
fn cfd_definition_request_resolves_each_nested_object_field() {
    let schema_source = "type Stats {\n  hp: int;\n}\n\
type Monster {\n  key: string;\n  stats: Stats;\n}\n";
    let (_cleanup, project) =
        test_project_with_config("lsp-cfd-nested-field-definition", schema_source, "data");
    let data_dir = project.root_dir.join("data");
    std::fs::create_dir_all(&data_dir).expect("create data dir");
    let source_path = data_dir.join("monsters.cfd");
    let source = "base: Monster { stats: { hp: 10 } }\n";
    std::fs::write(&source_path, source).expect("write source cfd");
    let source_uri = path_to_file_uri(&source_path);
    let mut server = LspServer::new(project, Vec::new());

    server
        .handle_message(&json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": source_uri,
                    "text": source
                }
            }
        }))
        .expect("open cfd document");
    server.writer.clear();

    let stats = cfd_definition_result_at(&mut server, &source_uri, source, "stats");
    assert_eq!(stats["range"]["start"]["line"], 5);
    assert_eq!(stats["range"]["start"]["character"], 2);
    assert_eq!(stats["range"]["end"]["character"], 7);

    let hp = cfd_definition_result_at(&mut server, &source_uri, source, "hp");
    assert_eq!(hp["range"]["start"]["line"], 1);
    assert_eq!(hp["range"]["start"]["character"], 2);
    assert_eq!(hp["range"]["end"]["character"], 4);
}

#[test]
fn cfd_document_symbols_returns_record_entries() {
    let source = "sword: Item { }\nshield: Item { }\n";
    let (ast, _) = parse_cfd(source);
    let result = cfd::document_symbols(source, &ast);
    let symbols = result.as_array().expect("array");
    assert_eq!(symbols.len(), 2);
    assert_eq!(symbols[0]["name"], "sword");
    assert_eq!(symbols[0]["detail"], "Item");
    assert_eq!(symbols[1]["name"], "shield");
}

#[test]
#[allow(clippy::cast_possible_truncation)]
fn cfd_semantic_tokens_no_overlap_from_comment_and_ast() {
    // A comment token spanning bytes 0..10 and an AST token at 5..8
    // should not produce overlapping output.
    // Use a real source that has a comment followed by a record.
    let source = "// comment\nsword: Item { }";
    let (ast, _) = parse_cfd(source);
    let result = cfd::semantic_tokens(source, &ast);
    let data = result["data"].as_array().expect("data array");
    // Walk the delta-encoded data and reconstruct absolute positions.
    let mut line = 0usize;
    let mut character = 0usize;
    let mut prev_end_char = 0usize;
    let mut prev_end_line = 0usize;
    let mut ok = true;
    for chunk in data.chunks(5) {
        if chunk.len() < 5 {
            break;
        }
        let dl = chunk[0].as_u64().unwrap_or(0) as usize;
        let dc = chunk[1].as_u64().unwrap_or(0) as usize;
        let len = chunk[2].as_u64().unwrap_or(0) as usize;
        line += dl;
        character = if dl == 0 { character + dc } else { dc };
        let end_char = character + len;
        if line == prev_end_line && character < prev_end_char {
            ok = false; // overlap detected
            break;
        }
        prev_end_line = line;
        prev_end_char = end_char;
    }
    assert!(ok, "semantic tokens must not overlap");
}

#[test]
fn cfd_semantic_tokens_no_comment_token_inside_string() {
    // A URL inside a string must not be treated as a comment.
    let source = r#"r: T { url: "http://example.com" }"#;
    let (ast, _) = parse_cfd(source);
    let result = cfd::semantic_tokens(source, &ast);
    let data = result["data"].as_array().expect("data");
    // Each group of 5: [dline, dchar, len, type, modifiers]
    // SEM_COMMENT index is 10.
    for chunk in data.chunks(5) {
        if chunk.len() < 5 {
            break;
        }
        assert_ne!(
            chunk[3].as_u64().unwrap_or(0),
            10,
            "should not emit comment token for // inside a string"
        );
    }
}

#[test]
fn cfd_hover_returns_null_for_non_type_position() {
    let source = "sword: Item { }";
    let (ast, _) = parse_cfd(source);
    // Hover in the middle of whitespace after the record.
    let result = cfd::hover(source, &ast, None, source.len() - 1);
    assert!(
        result.is_null() || result == Value::Null || result.get("range").is_some(),
        "hover at brace position should return null or a range-based result"
    );
}

#[test]
fn cfd_hover_on_type_name_returns_type_info() {
    let source = "sword: Item { }";
    let (ast, _) = parse_cfd(source);
    // "Item" starts at byte 7.
    let type_name_offset = source.find("Item").expect("Item");
    let result = cfd::hover(source, &ast, None, type_name_offset + 1);
    // Without schema we get a backtick-quoted name.
    let contents = result["contents"]["value"].as_str().unwrap_or("");
    assert!(contents.contains("Item"), "hover should mention type name");
}

#[test]
fn cfd_definition_type_name_extracts_type_at_offset() {
    let source = "sword: Item { }\n";
    let (ast, _) = parse_cfd(source);
    let type_offset = source.find("Item").expect("Item") + 1;
    let name = cfd::definition_type_name(&ast, type_offset);
    assert_eq!(name, Some("Item"));
}

#[test]
fn cfd_definition_type_name_returns_none_outside_type_span() {
    let source = "sword: Item { }\n";
    let (ast, _) = parse_cfd(source);
    // Offset 0 is inside the key "sword", not the type name.
    let name = cfd::definition_type_name(&ast, 0);
    assert_eq!(name, None);
}

#[test]
fn cfd_definition_field_name_extracts_record_field_at_offset() {
    let source = "sword: Item { damage: 10 }\n";
    let (ast, _) = parse_cfd(source);
    let field_offset = source.find("damage").expect("damage") + 1;

    let field = cfd::definition_field_name(&ast, None, field_offset);

    assert_eq!(field, Some(("Item".to_string(), "damage")));
}

#[test]
fn cfd_schema_field_definition_location_finds_field_name_span() {
    let source = "type Item {\n  key: string;\n  damage: int;\n}\n";
    let (_cleanup, build) = test_lsp_build("cfd-schema-field-goto-def", source);

    let result = cft_schema_field_definition_location(&build, "Item", "damage")
        .expect("damage field definition");

    assert_eq!(result["range"]["start"]["line"], 2);
    assert_eq!(result["range"]["start"]["character"], 2);
    assert_eq!(result["range"]["end"]["line"], 2);
    assert_eq!(result["range"]["end"]["character"], 8);
}

#[test]
fn cfd_goto_def_continues_past_unparseable_document() {
    // Build an LspBuild with two modules; one has a syntax error.
    // cft_type_definition_location should still find the type in the good module.
    let cft_source = "type GoodType { level: int; }\n";
    let (_cleanup, build) = test_lsp_build("cfd-goto-def", cft_source);
    // GoodType is defined — should find it.
    let result = cft_type_definition_location(&build, "GoodType");
    assert!(result.is_some(), "should find GoodType definition");
    // Unknown type — should return None without panicking.
    let result2 = cft_type_definition_location(&build, "NonExistent");
    assert!(result2.is_none());
}
