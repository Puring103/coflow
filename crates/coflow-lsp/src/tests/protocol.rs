use super::common::*;
use super::*;

#[test]
fn request_errors_are_reported_without_returning_from_handler() {
    let (_cleanup, project) = test_project("lsp-request-error", "type Item { key: string; }\n");
    let schema_path = project.root_dir.join("schema");
    std::fs::remove_dir_all(schema_path).expect("remove schema dir");
    let mut server = LspServer::new(project, Vec::new());

    let result = server.handle_message(&json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": "file:///missing.cft" },
            "position": { "line": 0, "character": 0 }
        }
    }));

    assert!(result.is_ok(), "handler should isolate request errors");
    let messages = written_messages(&server.writer);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["id"], 7);
    assert!(messages[0]["error"]["message"]
        .as_str()
        .is_some_and(|message| message.contains("schema path")));
}

#[test]
fn notification_errors_are_logged_without_returning_from_handler() {
    let (_cleanup, project) =
        test_project("lsp-notification-error", "type Item { key: string; }\n");
    let schema_path = project.root_dir.join("schema");
    std::fs::remove_dir_all(schema_path).expect("remove schema dir");
    let mut server = LspServer::new(project, Vec::new());

    let result = server.handle_message(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": "file:///missing.cft",
                "text": "type Item { key: string; }\n"
            }
        }
    }));

    assert!(result.is_ok(), "handler should isolate notification errors");
    let messages = written_messages(&server.writer);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["method"], "window/logMessage");
    assert_eq!(messages[0]["params"]["type"], 1);
    assert!(messages[0]["params"]["message"]
        .as_str()
        .is_some_and(|message| message.contains("schema path")));
}

#[test]
fn requests_after_shutdown_return_invalid_request() {
    let (_cleanup, project) = test_project("lsp-shutdown", "type Item { key: string; }\n");
    let mut server = LspServer::new(project, Vec::new());

    server
        .handle_message(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "shutdown",
            "params": null
        }))
        .expect("shutdown");
    server
        .handle_message(&json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "initialize",
            "params": {}
        }))
        .expect("initialize after shutdown");
    server
        .handle_message(&json!({
            "jsonrpc": "2.0",
            "method": "exit",
            "params": null
        }))
        .expect("exit");

    let messages = written_messages(&server.writer);
    assert_eq!(messages[0]["id"], 1);
    assert_eq!(messages[0]["result"], Value::Null);
    assert_eq!(messages[1]["id"], 2);
    assert_eq!(messages[1]["error"]["code"], -32600);
    assert!(server.should_exit);
}

#[test]
fn handler_ignores_malformed_notifications_and_reports_unknown_requests() {
    let (_cleanup, project) = test_project("lsp-handler-edges", "type Item { key: string; }\n");
    let mut server = LspServer::new(project, Vec::new());

    for uri in ["file://", "file://localhost", "file://server"] {
        server
            .handle_message(&json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": { "textDocument": { "uri": uri, "text": "broken" } }
            }))
            .expect("empty file URI path is ignored");
    }
    server
        .handle_message(&json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": { "textDocument": { "uri": "not-a-file-uri", "text": "broken" } }
        }))
        .expect("invalid didOpen URI is ignored");
    server
        .handle_message(&json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didChange",
            "params": {
                "textDocument": { "uri": "not-a-file-uri" },
                "contentChanges": [{ "range": {}, "text": "ignored" }]
            }
        }))
        .expect("invalid didChange URI is ignored");
    server
        .handle_message(&json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didClose",
            "params": { "textDocument": { "uri": "not-a-file-uri" } }
        }))
        .expect("invalid didClose URI is ignored");
    server
        .handle_message(&json!({
            "jsonrpc": "2.0",
            "id": 99,
            "method": "workspace/unknown",
            "params": {}
        }))
        .expect("unknown request should be converted to an LSP error response");
    server
        .handle_message(&json!({
            "jsonrpc": "2.0",
            "method": "workspace/unknownNotification",
            "params": {}
        }))
        .expect("unknown notifications are ignored");

    let messages = written_messages(&server.writer);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["id"], 99);
    assert_eq!(messages[0]["error"]["code"], -32601);
    assert!(messages[0]["error"]["message"]
        .as_str()
        .is_some_and(|message| message.contains("workspace/unknown")));
}

#[test]
fn did_save_with_text_updates_document_and_without_text_revalidates_project() {
    let source = "type Item { key: string; }\n";
    let (_cleanup, project) = test_project("lsp-save-edges", source);
    let schema_path = project.root_dir.join("schema").join("main.cft");
    let uri = path_to_file_uri(&schema_path);
    let mut server = LspServer::new(project, Vec::new());

    server
        .handle_message(&json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": { "textDocument": { "uri": uri, "text": source } }
        }))
        .expect("open document");
    server.writer.clear();

    let changed = "type Item { key: int; }\n";
    server
        .handle_message(&json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didSave",
            "params": {
                "textDocument": { "uri": uri },
                "text": changed
            }
        }))
        .expect("didSave with text");
    let normalized = normalize_path(&schema_path);
    assert_eq!(
        server
            .open_documents
            .get(&normalized)
            .map(|document| document.text.as_str()),
        Some(changed)
    );

    server
        .handle_message(&json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didSave",
            "params": { "textDocument": { "uri": uri } }
        }))
        .expect("didSave without text revalidates");
    server
        .handle_message(&json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didClose",
            "params": { "textDocument": { "uri": uri } }
        }))
        .expect("didClose removes document");
    assert!(!server.open_documents.contains_key(&normalized));
}

#[test]
fn feature_requests_return_empty_results_for_missing_params_and_unknown_documents() {
    let source = "type Item { key: string; }\n";
    let (_cleanup, project) = test_project("lsp-request-param-edges", source);
    let mut server = LspServer::new(project, Vec::new());
    server.validate_project().expect("initial validation");
    server.writer.clear();

    let unknown_doc = json!({
        "textDocument": { "uri": "file:///not-in-project.cft" },
        "position": { "line": 0, "character": 0 }
    });
    let unknown_uri = json!({
        "textDocument": { "uri": "file:///not-in-project.cft" }
    });
    let requests = [
        ("textDocument/completion", json!({}), Value::Null),
        ("textDocument/hover", json!({}), Value::Null),
        ("textDocument/definition", json!({}), Value::Null),
        ("textDocument/documentSymbol", json!({}), json!([])),
        ("textDocument/formatting", json!({}), Value::Null),
        (
            "textDocument/semanticTokens/full",
            json!({}),
            json!({"data": []}),
        ),
        ("textDocument/completion", unknown_doc.clone(), json!([])),
        ("textDocument/hover", unknown_doc.clone(), Value::Null),
        ("textDocument/definition", unknown_doc, Value::Null),
        (
            "textDocument/documentSymbol",
            unknown_uri.clone(),
            json!([]),
        ),
        ("textDocument/formatting", unknown_uri.clone(), json!([])),
        (
            "textDocument/semanticTokens/full",
            unknown_uri,
            json!({"data": []}),
        ),
    ];

    for (index, (method, params, _)) in requests.iter().enumerate() {
        server
            .handle_message(&json!({
                "jsonrpc": "2.0",
                "id": index,
                "method": method,
                "params": params
            }))
            .expect("request should be handled");
    }

    let messages = written_messages(&server.writer);
    assert_eq!(messages.len(), requests.len());
    for (message, (_, _, expected)) in messages.iter().zip(requests) {
        assert_eq!(message["result"], expected);
    }
}

#[test]
fn initialize_advertises_semantic_token_modifiers() {
    let (_cleanup, project) = test_project(
        "lsp-semantic-modifier-legend",
        "type Item { key: string; }\n",
    );
    let mut server = LspServer::new(project, Vec::new());

    server
        .handle_message(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        }))
        .expect("initialize");

    let messages = written_messages(&server.writer);
    let modifiers = messages[0]["result"]["capabilities"]["semanticTokensProvider"]["legend"]
        ["tokenModifiers"]
        .as_array()
        .expect("token modifiers");
    let modifier_names = modifiers
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert_eq!(
        modifier_names,
        vec!["declaration", "reference", "path", "record", "schema"]
    );
}

#[test]
fn formatting_requests_handle_idempotent_unknown_and_dirty_documents() {
    let source = "type Item {\n  key: string;\n}\n";
    let (_cleanup, project) = test_project("lsp-formatting-edges", source);
    let schema_path = project.root_dir.join("schema").join("main.cft");
    let schema_uri = path_to_file_uri(&schema_path);
    let extra_uri = path_to_file_uri(&project.root_dir.join("outside.cft"));
    let mut server = LspServer::new(project, Vec::new());

    server
        .handle_message(&json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": extra_uri,
                    "text": "type Extra { key: string; }\n"
                }
            }
        }))
        .expect("non-schema open should be diagnosed, not fatal");
    assert!(written_messages(&server.writer).iter().any(|message| {
        message["method"] == "textDocument/publishDiagnostics"
            && message["params"]["diagnostics"]
                .as_array()
                .is_some_and(|diagnostics| {
                    diagnostics
                        .iter()
                        .any(|diagnostic| diagnostic["code"] == "CFT-LSP")
                })
    }));
    server.writer.clear();

    server
        .handle_message(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "textDocument/formatting",
            "params": { "textDocument": { "uri": schema_uri } }
        }))
        .expect("formatting request");
    assert_eq!(written_messages(&server.writer)[0]["result"], json!([]));
    server.writer.clear();

    let dirty = "type Item {\nkey: string;\n}";
    server
        .handle_message(&json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didChange",
            "params": {
                "textDocument": { "uri": schema_uri },
                "contentChanges": [{ "text": dirty }]
            }
        }))
        .expect("dirty change");
    server.writer.clear();
    server
        .handle_message(&json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "textDocument/formatting",
            "params": { "textDocument": { "uri": schema_uri } }
        }))
        .expect("formatting dirty document");

    let messages = written_messages(&server.writer);
    let edits = messages[0]["result"].as_array().expect("edits");
    assert_eq!(edits.len(), 1);
    assert!(edits[0]["newText"]
        .as_str()
        .is_some_and(|text| text.contains("  key: string;")));
}

#[test]
fn oversized_content_length_is_rejected_before_body_allocation() {
    let mut reader = io::Cursor::new(format!("Content-Length: {}\r\n\r\n", 16 * 1024 * 1024 + 1));

    let err = read_message(&mut reader).expect_err("expected content length cap error");

    assert!(err.contains("exceeds"), "expected cap error, got `{err}`");
}

#[test]
fn file_uri_parser_rejects_file_uris_without_paths() {
    assert!(path_from_file_uri("file://").is_none());
    assert!(path_from_file_uri("file://localhost").is_none());
    assert!(path_from_file_uri("file://server").is_none());
}

#[test]
fn file_uri_parser_handles_windows_localhost_and_unc_forms() {
    if cfg!(windows) {
        assert_eq!(
            path_from_file_uri("file:///C:/Game/schema/main.cft"),
            Some(PathBuf::from(r"C:\Game\schema\main.cft"))
        );
        assert_eq!(
            path_from_file_uri("file://localhost/C:/Game/schema/main.cft"),
            Some(PathBuf::from(r"C:\Game\schema\main.cft"))
        );
        assert_eq!(
            path_from_file_uri("file://server/share/schema/main.cft"),
            Some(PathBuf::from(r"\\server\share\schema\main.cft"))
        );
    } else {
        assert_eq!(
            path_from_file_uri("file://localhost/tmp/schema/main.cft"),
            Some(PathBuf::from("/tmp/schema/main.cft"))
        );
    }
}
