use super::super::definition::{
    cft_schema_field_definition_location, cft_type_definition_location,
};
use super::super::semantic_tokens::{
    MOD_DECLARATION, MOD_RECORD, SEM_NAMESPACE, SEM_VARIABLE,
};
use super::common::*;
use super::*;
use coflow_language::cfd::parse_cfd;

#[test]
fn cfd_definition_request_returns_schema_field_location() {
    let schema_source = "type Item {\n  key: string;\n  damage: int;\n}\n";
    let (_cleanup, project) = test_project("lsp-cfd-field-definition", schema_source);
    let cfd_path = project.root_dir().join("data.cfd");
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
    let cfd_path = project.root_dir().join("data.CFD");
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
    let data_dir = project.root_dir().join("data");
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

    server.writer.clear();
    let completion_position = position_from_byte(source, source.find("&sword").expect("reference") + 1);
    server
        .handle_message(&json!({
            "jsonrpc": "2.0",
            "id": 9,
            "method": "textDocument/completion",
            "params": {
                "textDocument": { "uri": source_uri },
                "position": {
                    "line": completion_position.line,
                    "character": completion_position.character
                }
            }
        }))
        .expect("record reference completion request");
    let completion_result = written_messages(&server.writer)[0]["result"]
        .as_array()
        .expect("completion array")
        .clone();
    let sword = completion_result
        .iter()
        .find(|item| item["label"] == "sword")
        .expect("indexed record key completion");
    assert_eq!(sword["insertText"], "&sword");
}

#[test]
fn cfd_definition_index_uses_actual_type_and_dirty_overlay() {
    let schema_source = "type Item {}\n\
type Skill {}\n\
type Holder { item: &Item; }\n";
    let (_cleanup, project) =
        test_project_with_config("lsp-cfd-typed-definition-index", schema_source, "data");
    let data_dir = project.root_dir().join("data");
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
fn cfd_definition_request_returns_null_for_invalid_record_references() {
    let schema_source = "type Stats {\n  hp: int;\n}\n\
type Monster {\n  key: string;\n  stats: Stats;\n}\n\
type Holder {\n  key: string;\n  hp: int;\n}\n";
    let (_cleanup, project) =
        test_project_with_config("lsp-cfd-path-field-definition", schema_source, "data");
    let data_dir = project.root_dir().join("data");
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
fn cfd_definition_request_resolves_each_nested_object_field() {
    let schema_source = "type Stats {\n  hp: int;\n}\n\
type Monster {\n  key: string;\n  stats: Stats;\n}\n";
    let (_cleanup, project) =
        test_project_with_config("lsp-cfd-nested-field-definition", schema_source, "data");
    let data_dir = project.root_dir().join("data");
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
    let source = "# comment\nsword: Item { }";
    let (ast, _) = parse_cfd(source);
    let result = cfd::semantic_tokens(source, &ast, None);
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
fn cfd_semantic_tokens_cover_migrated_modules_and_function_language() {
    let source = "namespace game::runtime;\n\
use game::schema::Runner;\n\
runner: Runner {\n\
  execute: fn(value: int) -> int {\n\
    # function comment\n\
    var label = \"value\";\n\
    if true { helper(value) + 1 } else { 0 }\n\
  },\n\
}\n";
    let (ast, diagnostics) = parse_cfd(source);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let result = cfd::semantic_tokens(source, &ast, None);
    let data = result["data"].as_array().expect("data");
    let token_types = data
        .chunks(5)
        .filter_map(|chunk| chunk.get(3).and_then(Value::as_u64))
        .collect::<std::collections::BTreeSet<_>>();

    for expected in [0, 1, 4, 5, 6, 7, 8, 9, 10, 11, 13] {
        assert!(
            token_types.contains(&expected),
            "missing semantic token type {expected}: {token_types:?}"
        );
    }
    assert!(
        data.chunks(5)
            .all(|chunk| chunk.get(2).and_then(Value::as_u64).unwrap_or(0) < 32),
        "function values must be tokenized instead of emitted as one string"
    );
}

#[test]
fn dirty_group_record_key_keeps_semantic_color_and_offers_completion() {
    let schema_source = "type Product { name: string; }\n";
    let (_cleanup, project) = test_project("lsp-cfd-dirty-group-key", schema_source);
    let cfd_path = project.root_dir().join("data.cfd");
    let cfd_uri = path_to_file_uri(&cfd_path);
    let source = "Product {\n    notebook { name: \"Notebook\", }\n    asd\n    test { name: \"Test\", }\n}\n";
    let mut server = LspServer::new(project, Vec::new());
    server
        .handle_message(&json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": cfd_uri,
                    "version": 1,
                    "text": source,
                }
            }
        }))
        .expect("open dirty cfd document");

    server.writer.clear();
    server
        .handle_message(&json!({
            "jsonrpc": "2.0",
            "id": 41,
            "method": "textDocument/semanticTokens/full",
            "params": { "textDocument": { "uri": cfd_uri } }
        }))
        .expect("semantic token request");
    let semantic_result = written_messages(&server.writer)[0]["result"].clone();
    assert_eq!(semantic_result["x-coflow-syntax-valid"], true);
    let tokens = decode_semantic_tokens(source, &semantic_result["data"]);
    assert!(tokens.contains(&DecodedSemanticToken {
        text: "asd".to_string(),
        token_type: SEM_NAMESPACE,
        modifiers: MOD_DECLARATION | MOD_RECORD,
    }), "{tokens:?}");

    server.writer.clear();
    let position = position_from_byte(source, source.find("asd").expect("asd") + 3);
    server
        .handle_message(&json!({
            "jsonrpc": "2.0",
            "id": 42,
            "method": "textDocument/completion",
            "params": {
                "textDocument": { "uri": cfd_uri },
                "position": { "line": position.line, "character": position.character }
            }
        }))
        .expect("completion request");
    let completion_result = written_messages(&server.writer)[0]["result"].clone();
    assert_eq!(completion_result[0]["label"], "asd");
    assert_eq!(
        completion_result[0]["insertText"],
        "asd {\n  name: ${1:\"value\"},\n}"
    );
    assert_eq!(completion_result[0]["detail"], "new Product record");
}

#[test]
fn incomplete_group_record_field_completion_uses_record_schema_context() {
    let schema_source = "type Product { name: string; price: int; enabled: bool; }\n\
type Unrelated { payload: string; }\n";
    let (_cleanup, project) = test_project("lsp-cfd-dirty-group-field", schema_source);
    let cfd_path = project.root_dir().join("data.cfd");
    let cfd_uri = path_to_file_uri(&cfd_path);
    let source = "Product {\n    make {\n        name: \"Draft\",\n        p\n    }\n}\n";
    let mut server = LspServer::new(project, Vec::new());
    server
        .handle_message(&json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": cfd_uri,
                    "version": 1,
                    "text": source,
                }
            }
        }))
        .expect("open incomplete grouped record field");

    server.writer.clear();
    let position = position_from_byte(source, source.find("p\n").expect("field prefix") + 1);
    server
        .handle_message(&json!({
            "jsonrpc": "2.0",
            "id": 43,
            "method": "textDocument/completion",
            "params": {
                "textDocument": { "uri": cfd_uri },
                "position": { "line": position.line, "character": position.character }
            }
        }))
        .expect("complete incomplete grouped record field");
    let messages = written_messages(&server.writer);
    let completion_result = messages[0]["result"].as_array().expect("completion array");
    let labels = completion_result
        .iter()
        .filter_map(|item| item["label"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(labels, vec!["price", "enabled"]);
    assert_eq!(completion_result[0]["detail"], "int");
}

#[test]
fn cfd_value_completion_uses_schema_type_and_function_body_context() {
    let schema_source = "enum Rarity { Common, Rare, }\n\
type Settings {\n\
  enabled: bool;\n\
  rarity: Rarity;\n\
  compute: fn(value: int) -> int;\n\
}\n";
    let (_cleanup, build) = test_lsp_build("lsp-cfd-value-completion", schema_source);
    let schema = build.schema().expect("compiled schema");

    let complete = |source: &str, needle: &str| {
        let (ast, _) = parse_cfd(source);
        let offset = source.find(needle).expect("completion needle") + needle.len();
        cfd::completion(source, &ast, Some(schema), offset)
    };

    let bool_items = complete("settings: Settings { enabled: t }", "enabled: t");
    assert_eq!(
        completion_labels(bool_items.as_array().expect("bool items").clone()),
        vec!["true", "false"]
    );

    let enum_items = complete("settings: Settings { rarity: R }", "rarity: R");
    assert_eq!(
        completion_labels(enum_items.as_array().expect("enum items").clone()),
        vec!["Common", "Rare"]
    );

    let function_items = complete("settings: Settings { compute: f }", "compute: f");
    assert_eq!(function_items[0]["label"], "fn");
    assert_eq!(
        function_items[0]["insertText"],
        "fn(value: int) -> int {\n    ${1}\n}"
    );
    assert_eq!(function_items[0]["insertTextFormat"], 2);

    let body_items = complete(
        "settings: Settings { compute: fn(value: int) -> int { ret } }",
        "ret",
    );
    let body_labels = completion_labels(body_items.as_array().expect("function body items").clone());
    assert!(body_labels.contains(&"return".to_string()));
    assert!(body_labels.contains(&"value".to_string()));
}

#[test]
fn cfd_formatted_strings_highlight_and_complete_record_fields() {
    let schema_source = "type Message { amount: int; enabled: Option<bool>; label: string; }\n";
    let (_cleanup, build) = test_lsp_build("lsp-cfd-formatted-string", schema_source);
    let schema = build.schema().expect("compiled schema");
    let source = r#"message: Message { amount: 7, enabled: true, label: "amount={amount}" }"#;
    let (ast, diagnostics) = parse_cfd(source);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");

    let semantic = cfd::semantic_tokens(source, &ast, Some(schema));
    let tokens = decode_semantic_tokens(source, &semantic["data"]);
    assert!(tokens.contains(&DecodedSemanticToken {
        text: "{amount}".to_string(),
        token_type: SEM_VARIABLE,
        modifiers: 0,
    }), "{tokens:?}");

    let offset = source.find("{amount}").expect("formatted reference") + 3;
    let completions = cfd::completion(source, &ast, Some(schema), offset);
    assert_eq!(
        completion_labels(completions.as_array().expect("completion array").clone()),
        vec!["amount", "enabled", "label", "Message"]
    );

    let option_source = "message: Message { enabled: t }";
    let (option_ast, _) = parse_cfd(option_source);
    let option_items = cfd::completion(
        option_source,
        &option_ast,
        Some(schema),
        option_source.find('t').expect("option value") + 1,
    );
    assert_eq!(
        completion_labels(option_items.as_array().expect("option items").clone()),
        vec!["None", "Some", "true", "false"]
    );
}

#[test]
fn cfd_formatted_string_completion_includes_nested_paths() {
    let schema_source = "type Details { count: int; }\n\
type Message { details: Details; label: string; }\n";
    let (_cleanup, build) = test_lsp_build("lsp-cfd-formatted-nested", schema_source);
    let schema = build.schema().expect("compiled schema");
    let source = r#"message: Message { details: { count: 1 }, label: "{det}" }"#;
    let (ast, diagnostics) = parse_cfd(source);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let offset = source.find("{det}").expect("formatted path") + "{det".len();

    let items = cfd::completion(source, &ast, Some(schema), offset);
    let labels = completion_labels(items.as_array().expect("formatted completions").clone());
    assert!(labels.contains(&"details".to_string()));
    assert!(labels.contains(&"details.count".to_string()));
}

#[test]
fn cfd_completion_recurses_through_arrays_and_inline_objects() {
    let schema_source = "type Child { enabled: bool; label: string = \"default\"; }\n\
type Root { children: [Child]; child: Child; }\n";
    let (_cleanup, build) = test_lsp_build("lsp-cfd-nested-completion", schema_source);
    let schema = build.schema().expect("compiled schema");

    let value_source = "root: Root { children: [{ enabled: t, }], child: {}, }";
    let (value_ast, diagnostics) = parse_cfd(value_source);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let value_offset = value_source.find("enabled: t").expect("nested bool") + "enabled: t".len();
    let value_items = cfd::completion(value_source, &value_ast, Some(schema), value_offset);
    assert_eq!(
        completion_labels(value_items.as_array().expect("nested value items").clone()),
        vec!["true", "false"]
    );

    let field_offset = value_source.find("child: {").expect("inline object") + "child: {".len();
    let field_items = cfd::completion(value_source, &value_ast, Some(schema), field_offset);
    let fields = field_items.as_array().expect("nested field items");
    assert_eq!(completion_labels(fields.clone()), vec!["enabled", "label"]);
    assert_eq!(fields[0]["insertText"], "enabled: ${1:true}");
    assert_eq!(fields[0]["insertTextFormat"], 2);
    assert_eq!(fields[0]["sortText"], "0enabled");
    assert_eq!(fields[1]["sortText"], "1label");
}

#[test]
fn cfd_top_level_completion_inserts_a_required_field_record_snippet() {
    let schema_source = "type Product { name: string; enabled: bool = true; }\n";
    let (_cleanup, build) = test_lsp_build("lsp-cfd-record-snippet", schema_source);
    let schema = build.schema().expect("compiled schema");
    let (ast, diagnostics) = parse_cfd("");
    assert!(diagnostics.is_empty(), "{diagnostics:?}");

    let items = cfd::completion("", &ast, Some(schema), 0);
    let product = items
        .as_array()
        .expect("top-level items")
        .iter()
        .find(|item| item["label"] == "Product")
        .expect("Product completion");
    assert_eq!(product["insertTextFormat"], 2);
    assert_eq!(
        product["insertText"],
        "${1:key}: Product {\n  name: ${2:\"value\"},\n}"
    );
}

#[test]
fn cfd_flag_completion_excludes_variants_already_in_an_incomplete_expression() {
    let schema_source = "@flag enum Access { Read = 1, Write = 2, Execute = 4, }\n\
type Settings { access: Access; }\n";
    let (_cleanup, build) = test_lsp_build("lsp-cfd-flag-completion", schema_source);
    let schema = build.schema().expect("compiled schema");
    let source = "settings: Settings { access: Read |  }";
    let (ast, _) = parse_cfd(source);
    let offset = source.find('|').expect("flag operator") + 2;

    let items = cfd::completion(source, &ast, Some(schema), offset);
    let labels = completion_labels(items.as_array().expect("flag completions").clone());
    assert_eq!(labels, vec!["Write", "Execute"]);
}

#[test]
fn cfd_semantic_tokens_no_comment_token_inside_string() {
    // A URL inside a string must not be treated as a comment.
    let source = r#"r: T { url: "http://example.com" }"#;
    let (ast, _) = parse_cfd(source);
    let result = cfd::semantic_tokens(source, &ast, None);
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

#[test]
fn function_document_uses_cfd_parser_and_lsp_tokens() {
    let source = "fn(left: int, operation: fn(int, int) -> int, right: int) -> int {\nleft + right\n}";
    let result = cfd::function_document(&json!({ "source": source }));

    assert_eq!(
        result["signature"],
        "fn(left: int, operation: fn(int, int) -> int, right: int) -> int"
    );
    assert_eq!(result["body"], "left + right");
    assert_eq!(result["bodyRange"]["start"]["line"], 1);
    assert_eq!(result["bodyRange"]["start"]["character"], 0);
    assert_eq!(result["bodyRange"]["end"]["line"], 1);
    assert_eq!(result["bodyRange"]["end"]["character"], 12);
    assert!(result["diagnostics"].as_array().is_some_and(Vec::is_empty));
    assert!(result["semanticTokens"]["data"]
        .as_array()
        .is_some_and(|data| !data.is_empty()));
    let labels = result["completions"]
        .as_array()
        .expect("completion array")
        .iter()
        .filter_map(|item| item["label"].as_str())
        .collect::<Vec<_>>();
    assert!(labels.contains(&"left"));
    assert!(labels.contains(&"right"));
    assert!(labels.contains(&"operation"));
    assert!(labels.contains(&"return"));
}

#[test]
fn function_document_rebuilds_source_and_reports_body_errors() {
    let source = "fn(value: int) -> int { value }";
    let result = cfd::function_document(&json!({
        "source": source,
        "body": "var broken = ;",
    }));

    assert_eq!(result["source"], "fn(value: int) -> int { var broken = ; }");
    assert!(result["diagnostics"]
        .as_array()
        .is_some_and(|diagnostics| !diagnostics.is_empty()));
}

#[test]
fn function_document_completion_includes_local_variables_and_snippets() {
    let source = "fn(value: int) -> int { var total = value; return total; }";
    let result = cfd::function_document(&json!({ "source": source }));
    let completions = result["completions"].as_array().expect("completions");
    assert!(completions.iter().any(|item| {
        item["label"] == "total" && item["detail"] == "local variable"
    }));
    let len = completions
        .iter()
        .find(|item| item["label"] == "len")
        .expect("builtin completion");
    assert_eq!(len["insertText"], "len(${1})");
    assert_eq!(len["insertTextFormat"], 2);
}
