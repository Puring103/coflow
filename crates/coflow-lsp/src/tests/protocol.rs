use super::common::*;
use super::*;
use crate::validation::{build_snapshot, ValidationInput};

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
    let response = messages
        .iter()
        .find(|message| message.get("id") == Some(&json!(7)))
        .expect("request response");
    assert_eq!(response["result"], Value::Null);
    assert!(messages.iter().any(|message| {
        message.get("method") == Some(&json!("textDocument/publishDiagnostics"))
            && message["params"]["diagnostics"][0]["code"] == "PROJECT-SCHEMA-PATH"
    }));
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
    assert!(messages.iter().any(|message| {
        message.get("method") == Some(&json!("textDocument/publishDiagnostics"))
            && message["params"]["diagnostics"][0]["code"] == "PROJECT-SCHEMA-PATH"
    }));
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
            .core
            .open_documents()
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
    assert!(!server.core.open_documents().contains_key(&normalized));
}

#[test]
fn validation_snapshot_rejects_stale_revision_commit() {
    let (_cleanup, project) = test_project("lsp-stale-snapshot", "type First {}\n");
    let schema_path = project.root_dir.join("schema").join("main.cft");
    let uri = path_to_file_uri(&schema_path);
    let mut core = LspValidationCore::new(project);

    assert!(core
        .apply_open_document(uri.clone(), "type First {}\n".to_string(), Some(1))
        .expect("open first revision"));
    let stale_input = core.validation_input();
    assert!(core
        .apply_change_document(uri.clone(), "type Second {}\n".to_string(), Some(2))
        .expect("change to second revision"));
    let current_input = core.validation_input();

    let stale = build_snapshot(&stale_input);
    let current = build_snapshot(&current_input);
    assert!(core.commit_snapshot(stale).is_empty());
    assert!(
        core.build().is_none(),
        "stale build must not become visible"
    );

    let publications = core.commit_snapshot(current);
    assert!(!publications.is_empty());
    let document = core
        .build()
        .and_then(|build| build.document_by_uri(&uri))
        .expect("current build document");
    assert_eq!(document.source(), "type Second {}\n");
}

#[test]
fn failed_snapshot_invalidates_build_and_clears_old_uri() {
    let (_cleanup, project) = test_project("lsp-failed-snapshot", "type Item {}\n");
    let schema_dir = project.root_dir.join("schema");
    let schema_uri = path_to_file_uri(&schema_dir.join("main.cft"));
    let mut core = LspValidationCore::new(project);

    let initial = build_snapshot(&core.validation_input());
    core.commit_snapshot(initial);
    assert!(core.build().is_some());

    std::fs::remove_dir_all(&schema_dir).expect("remove schema directory");
    core.mark_project_changed().expect("advance revision");
    let failed = build_snapshot(&core.validation_input());
    let publications = core.commit_snapshot(failed);

    assert!(
        core.build().is_none(),
        "failed revision must invalidate old build"
    );
    let cleared = publications
        .iter()
        .find(|publication| publication.uri == schema_uri)
        .expect("old schema URI clear publication");
    assert!(cleared.diagnostics.is_empty());
    assert!(
        publications
            .iter()
            .any(|publication| !publication.diagnostics.is_empty()),
        "failed snapshot should publish its replacement diagnostics"
    );
}

#[test]
fn unreadable_cfd_source_invalidates_current_snapshot() {
    let (_cleanup, project) = test_project_with_config(
        "lsp-unreadable-cfd-snapshot",
        "type Item {}\n",
        "data/items.cfd",
    );
    let source_path = project.root_dir.join("data").join("items.cfd");
    std::fs::create_dir_all(source_path.parent().expect("data parent"))
        .expect("create data directory");
    std::fs::write(&source_path, "item: Item {}\n").expect("write CFD source");
    let source_uri = path_to_file_uri(&source_path);
    let mut core = LspValidationCore::new(project);

    let initial = build_snapshot(&core.validation_input());
    core.commit_snapshot(initial);
    assert!(core.build().is_some());

    std::fs::remove_file(&source_path).expect("remove CFD source");
    core.mark_project_changed().expect("advance revision");
    let failed = build_snapshot(&core.validation_input());
    let publications = core.commit_snapshot(failed);

    assert!(core.build().is_none());
    assert!(publications.iter().any(|publication| {
        publication.uri == source_uri
            && publication
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic["code"] == "CFD-LSP")
    }));
}

#[test]
fn stale_document_version_does_not_replace_newer_text() {
    let (_cleanup, project) = test_project("lsp-document-version", "type Item {}\n");
    let uri = path_to_file_uri(&project.root_dir.join("schema").join("main.cft"));
    let mut core = LspValidationCore::new(project);

    assert!(core
        .apply_open_document(uri.clone(), "type Newer {}\n".to_string(), Some(7))
        .expect("open document"));
    assert!(!core
        .apply_change_document(uri, "type Older {}\n".to_string(), Some(6))
        .expect("reject stale change"));
    let document = core
        .open_documents()
        .values()
        .next()
        .expect("open document state");
    assert_eq!(document.text, "type Newer {}\n");
    assert_eq!(document.version, Some(7));
}

#[test]
fn validation_worker_coalesces_pending_revisions_and_commits_only_latest() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{mpsc, Arc, Barrier};
    use std::time::Duration;

    let (_cleanup, project) = test_project("lsp-validation-coalescing", "type First {}\n");
    let uri = path_to_file_uri(&project.root_dir.join("schema").join("main.cft"));
    let mut core = LspValidationCore::new(project);
    core.apply_open_document(uri.clone(), "type First {}\n".to_string(), Some(1))
        .expect("open first revision");
    let first = core.validation_input();

    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let build_count = Arc::new(AtomicUsize::new(0));
    let builder_entered = Arc::clone(&entered);
    let builder_release = Arc::clone(&release);
    let builder_count = Arc::clone(&build_count);
    let (events_tx, events_rx) = mpsc::channel();
    let worker = ValidationWorker::spawn_test(
        events_tx,
        Arc::new(move |input: ValidationInput| {
            if builder_count.fetch_add(1, Ordering::SeqCst) == 0 {
                builder_entered.wait();
                builder_release.wait();
            }
            ValidationSnapshot::empty(input.revision())
        }),
    );

    assert!(worker.schedule(first));
    entered.wait();
    core.apply_change_document(uri.clone(), "type Second {}\n".to_string(), Some(2))
        .expect("second revision");
    assert!(worker.schedule(core.validation_input()));
    core.apply_change_document(uri, "type Third {}\n".to_string(), Some(3))
        .expect("third revision");
    let latest = core.validation_input();
    assert!(worker.schedule(latest.clone()));
    release.wait();

    let mut snapshots = Vec::new();
    while snapshots.len() < 2 {
        match events_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("validation worker result")
        {
            RunEvent::Validation(snapshot) => snapshots.push(snapshot),
            RunEvent::Incoming(_) | RunEvent::ReadError(_) | RunEvent::EndOfInput => {}
        }
    }
    assert_eq!(build_count.load(Ordering::SeqCst), 2);
    assert!(core.commit_snapshot(*snapshots.remove(0)).is_empty());
    assert!(!core.is_current());
    core.commit_snapshot(*snapshots.remove(0));
    assert!(core.is_current());
    assert_eq!(latest.revision(), core.validation_input().revision());
    drop(worker);
}

#[test]
fn queued_feature_request_is_cancelled_when_its_validation_revision_expires() {
    let (_cleanup, project) = test_project("lsp-stale-queued-request", "type First {}\n");
    let uri = path_to_file_uri(&project.root_dir.join("schema").join("main.cft"));
    let mut server = LspServer::new(project, Vec::new());
    server
        .core
        .apply_open_document(uri.clone(), "type First {}\n".to_string(), Some(1))
        .expect("open first revision");
    let mut pending = VecDeque::from([PendingRequest::new(
        server.core.revision(),
        json!({
            "jsonrpc": "2.0",
            "id": 41,
            "method": "textDocument/hover",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": 0, "character": 5 }
            }
        }),
    )]);

    server
        .core
        .apply_change_document(uri, "type Second {}\n".to_string(), Some(2))
        .expect("advance past queued request");
    let current = build_snapshot(&server.core.validation_input());
    server.core.commit_snapshot(current);
    cancel_stale_pending_requests(&mut server, &mut pending).expect("cancel stale request");

    assert!(pending.is_empty());
    let messages = written_messages(&server.writer);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["id"], 41);
    assert_eq!(messages[0]["error"]["code"], -32800);
}

#[test]
fn watched_closed_schema_cfd_and_config_files_refresh_the_snapshot() {
    let (_cleanup, project) =
        test_project_with_config("lsp-watched-files", "type Item {}\n", "data/items.cfd");
    let root = project.root_dir.clone();
    let schema_path = root.join("schema").join("main.cft");
    let cfd_path = root.join("data").join("items.cfd");
    std::fs::create_dir_all(cfd_path.parent().expect("data parent")).expect("create data dir");
    std::fs::write(&cfd_path, "old: Item {}\n").expect("write initial CFD");
    let mut server = LspServer::new(project, Vec::new());
    server.validate_project().expect("initial validation");

    std::fs::write(&schema_path, "type Item { value: int = 1; }\n").expect("change schema");
    std::fs::write(&cfd_path, "new: Item {}\n").expect("change CFD");
    server
        .handle_message(&json!({
            "jsonrpc": "2.0",
            "method": "workspace/didChangeWatchedFiles",
            "params": {
                "changes": [
                    { "uri": path_to_file_uri(&schema_path), "type": 2 },
                    { "uri": path_to_file_uri(&cfd_path), "type": 2 }
                ]
            }
        }))
        .expect("refresh closed files");

    let schema_document = server
        .core
        .build()
        .and_then(|build| build.document_by_uri(&path_to_file_uri(&schema_path)))
        .expect("refreshed schema document");
    assert!(schema_document.source.contains("value: int"));
    let LspRequestDocument::Cfd(cfd_document) =
        server.core.request_document(&path_to_file_uri(&cfd_path))
    else {
        panic!("closed CFD should be available from the validation snapshot");
    };
    assert!(cfd_document.source.contains("new: Item"));

    let alternate_path = root.join("alternate.cft");
    std::fs::write(&alternate_path, "type Alternate {}\n").expect("write alternate schema");
    let config_path = root.join("coflow.yaml");
    std::fs::write(&config_path, "schema: alternate.cft\n").expect("change config");
    server
        .handle_message(&json!({
            "jsonrpc": "2.0",
            "method": "workspace/didChangeWatchedFiles",
            "params": {
                "changes": [{ "uri": path_to_file_uri(&config_path), "type": 2 }]
            }
        }))
        .expect("reload project config");
    assert!(server.core.build().is_some_and(|build| build
        .document_by_uri(&path_to_file_uri(&alternate_path))
        .is_some()));
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
fn unified_diagnostic_severity_is_preserved_in_lsp_rendering() {
    for (severity, expected) in [
        (coflow_api::Severity::Error, 1),
        (coflow_api::Severity::Warning, 2),
        (coflow_api::Severity::Info, 3),
    ] {
        let diagnostic = coflow_api::Diagnostic {
            code: "TEST".to_string(),
            stage: "TEST".to_string(),
            severity,
            message: "message".to_string(),
            primary: None,
            related: Vec::new(),
        };

        assert_eq!(lsp_diagnostic(&diagnostic)["severity"], json!(expected));
    }
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
