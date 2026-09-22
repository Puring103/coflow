use super::common::*;
use super::*;
use crate::service::{LanguagePosition, LanguageService};

#[test]
fn typed_service_matches_protocol_and_retains_diagnostics() {
    let source = "table Item { value: int; }\n";
    let (_cleanup, project) = test_project("typed-service", source);
    let path = project.root_dir().join("schema/main.cft");
    let uri = path_to_file_uri(&path);
    let mut service = LanguageService::new(project.clone());
    let mut server = LspServer::new(project, Vec::new());
    let draft = "table Item { value: Missing; }\n";
    service.synchronize(&path, draft, 7).unwrap();
    server.handle_message(&json!({"jsonrpc":"2.0", "method":"textDocument/didOpen", "params":{"textDocument":{"uri":uri,"version":7,"text":draft}}})).unwrap();
    let first = service.document(&path).unwrap();
    assert!(!first.diagnostics.is_empty());
    assert_eq!(first, service.document(&path).unwrap());
    let publications = written_messages(&server.writer);
    let diagnostics = publications
        .iter()
        .find(|m| m["method"] == "textDocument/publishDiagnostics" && m["params"]["uri"] == uri)
        .unwrap();
    assert_eq!(
        diagnostics["params"]["diagnostics"],
        wire(&first.diagnostics)
    );
    let position = LanguagePosition {
        line: 0,
        character: 20,
    };
    for (id, method, expected) in [
        (
            1,
            "textDocument/completion",
            wire(service.completion(&path, &position).unwrap()),
        ),
        (
            2,
            "textDocument/semanticTokens/full",
            json!({"data":first.semantic_token_data,"x-coflow-syntax-valid":first.syntax_valid}),
        ),
        (
            3,
            "textDocument/formatting",
            wire(service.formatting(&path).unwrap().edits),
        ),
    ] {
        server.writer.clear();
        server.handle_message(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":{"textDocument":{"uri":uri},"position":position}})).unwrap();
        let messages = written_messages(&server.writer);
        assert_eq!(
            messages.iter().find(|m| m["id"] == id).unwrap()["result"],
            expected,
            "{method}"
        );
    }
}

#[test]
fn document_versions_and_unsaved_overlays_survive_rebase() {
    let (_cleanup, project) = test_project("typed-rebase", "table Item { value: int; }\n");
    let path = project.root_dir().join("schema/main.cft");
    let mut service = LanguageService::new(project.clone());
    let draft = "table Draft { value: Missing; }\n";
    service.synchronize(&path, draft, 9).unwrap();
    let expected = service.document(&path).unwrap();
    assert!(!expected.diagnostics.is_empty());
    assert!(service.synchronize(&path, "", 8).is_err());
    assert!(service.synchronize(&path, "", 9).is_err());
    service.rebase(project).unwrap();
    assert_eq!(service.document(&path).unwrap(), expected);
    assert!(service.synchronize(&path, "", 8).is_err());
    service.close_document(&path).unwrap();
    assert!(service.document(&path).unwrap().diagnostics.is_empty());
}
