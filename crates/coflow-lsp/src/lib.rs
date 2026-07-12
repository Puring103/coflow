#![cfg_attr(
    not(test),
    deny(
        clippy::dbg_macro,
        clippy::expect_used,
        clippy::panic,
        clippy::panic_in_result_fn,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable,
        clippy::unwrap_used
    )
)]

mod cfd;
mod completion;
mod definition;
mod diagnostics;
mod document_symbols;
mod documentation;
mod formatting;
mod hover;
mod position;
mod protocol;
mod semantic_tokens;
mod state;
mod text;
mod uri;
mod validation;

#[cfg(test)]
use coflow_project::normalize_path;
use coflow_project::Project;
#[cfg(test)]
use coflow_runtime::compile_schema_project_with_overrides;
use completion::completion_items;
#[cfg(test)]
pub(crate) use completion::{
    annotation_completion_items, check_expression_completion_items, completion_scope,
    dot_completion_items, top_level_completion_items, CompletionScope,
};
#[cfg(test)]
pub(crate) use definition::field_location_by_chain;
use definition::{
    cfd_record_definition_location, cft_schema_field_definition_location,
    cft_type_definition_location, definitions_at,
};
#[cfg(test)]
use diagnostics::{label_uri, lsp_diagnostic, lsp_label_location};
use document_symbols::document_symbols;
pub(crate) use documentation::is_builtin_name;
use formatting::format_cft;
use hover::hover_at;
use position::{
    byte_offset_from_position, byte_range, full_document_range, range_from_span, LspPosition,
};
use protocol::{
    did_change_document, did_change_watched_files, did_open_document, did_save_document,
    read_message, text_document_uri, TextRequest,
};
use semantic_tokens::{semantic_token_data, SEMANTIC_TOKEN_MODIFIERS, SEMANTIC_TOKEN_TYPES};
use serde_json::{json, Value};
pub(crate) use state::{
    current_field_at, current_type_at, enum_name_exists, enum_variant_by_chain,
    enum_variant_exists, field_by_chain, field_by_type, quantifier_bindings_at,
    type_name_of_schema_ref, type_of_chain, LspBuild, LspDocument,
};
#[cfg(test)]
use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::io::{self, BufReader, Write};
#[cfg(test)]
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::thread;
pub(crate) use text::{
    dotted_chain_at, is_ident_continue, is_trivia_position, last_ident, line_prefix_at,
    parse_dotted_ident_chain, previous_char, word_at,
};
use uri::path_from_file_uri;
#[cfg(test)]
pub(crate) use uri::path_to_file_uri;
pub(crate) use validation::{
    DiagnosticPublication, LspRequestDocument, LspValidationCore, ValidationSnapshot,
    ValidationWorker,
};

enum RunEvent {
    Incoming(Vec<u8>),
    ReadError(String),
    EndOfInput,
    Validation(Box<ValidationSnapshot>),
}

struct PendingRequest {
    revision: validation::ValidationRevision,
    message: Value,
}

impl PendingRequest {
    const fn new(revision: validation::ValidationRevision, message: Value) -> Self {
        Self { revision, message }
    }
}

/// Runs the CFT language server over stdio.
///
/// # Errors
///
/// Returns an error when reading LSP messages, parsing JSON, handling a request,
/// or writing a response fails.
pub fn run(project: Project) -> Result<bool, String> {
    let (events_tx, events_rx) = mpsc::channel();
    spawn_input_reader(events_tx.clone());
    let validation_worker = ValidationWorker::spawn(events_tx);
    let stdout = io::stdout();
    let mut server = LspServer::new(project, stdout.lock());
    let mut pending_requests = VecDeque::new();

    while let Ok(event) = events_rx.recv() {
        match event {
            RunEvent::Incoming(bytes) => {
                let message: Value = match serde_json::from_slice(&bytes) {
                    Ok(message) => message,
                    Err(err) => {
                        server.write_parse_error(&format!(
                            "failed to parse LSP JSON message: {err}"
                        ))?;
                        continue;
                    }
                };
                if !server.shutdown_requested {
                    if let Some(changed) =
                        apply_runtime_document_notification(&mut server.core, &message)?
                    {
                        if changed {
                            cancel_stale_pending_requests(&mut server, &mut pending_requests)?;
                            validation_worker.schedule(server.core.validation_input());
                        }
                        continue;
                    }
                }
                if request_requires_snapshot(&message) && !server.core.is_current() {
                    pending_requests
                        .push_back(PendingRequest::new(server.core.revision(), message));
                    validation_worker.schedule(server.core.validation_input());
                    continue;
                }
                server.handle_message(&message)?;
            }
            RunEvent::Validation(candidate) => {
                let publications = server.core.commit_snapshot(*candidate);
                server.publish_diagnostic_publications(publications)?;
                if server.core.is_current() {
                    cancel_stale_pending_requests(&mut server, &mut pending_requests)?;
                    while let Some(request) = pending_requests.pop_front() {
                        server.handle_message(&request.message)?;
                    }
                }
            }
            RunEvent::ReadError(error) => return Err(error),
            RunEvent::EndOfInput => break,
        }
        if server.should_exit {
            break;
        }
    }

    Ok(true)
}

fn spawn_input_reader(events: Sender<RunEvent>) {
    thread::spawn(move || {
        let stdin = io::stdin();
        let mut reader = BufReader::new(stdin.lock());
        loop {
            match read_message(&mut reader) {
                Ok(Some(bytes)) => {
                    if events.send(RunEvent::Incoming(bytes)).is_err() {
                        break;
                    }
                }
                Ok(None) => {
                    let _ = events.send(RunEvent::EndOfInput);
                    break;
                }
                Err(error) => {
                    let _ = events.send(RunEvent::ReadError(error));
                    break;
                }
            }
        }
    });
}

fn apply_runtime_document_notification(
    core: &mut LspValidationCore,
    message: &Value,
) -> Result<Option<bool>, String> {
    if message.get("id").is_some() {
        return Ok(None);
    }
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        return Ok(None);
    };
    let params = message.get("params").unwrap_or(&Value::Null);
    match method {
        "textDocument/didOpen" => did_open_document(params).map_or(Ok(Some(false)), |update| {
            let (uri, text, version) = update;
            core.apply_open_document(uri, text, version).map(Some)
        }),
        "textDocument/didChange" => did_change_document(params).map_or(Ok(Some(false)), |update| {
            let (uri, text, version) = update;
            core.apply_change_document(uri, text, version).map(Some)
        }),
        "textDocument/didSave" => did_save_document(params).map_or(Ok(Some(false)), |update| {
            let (uri, text) = update;
            if let Some(text) = text {
                core.apply_change_document(uri, text, None).map(Some)
            } else {
                core.mark_project_changed().map(|()| Some(true))
            }
        }),
        "textDocument/didClose" => text_document_uri(params).map_or(Ok(Some(false)), |uri| {
            core.apply_close_document(&uri).map(Some)
        }),
        "workspace/didChangeWatchedFiles" => core
            .apply_watched_files(&did_change_watched_files(params))
            .map(Some),
        _ => Ok(None),
    }
}

fn cancel_stale_pending_requests<W: Write>(
    server: &mut LspServer<W>,
    pending_requests: &mut VecDeque<PendingRequest>,
) -> Result<(), String> {
    let revision = server.core.revision();
    let mut current = VecDeque::with_capacity(pending_requests.len());
    while let Some(request) = pending_requests.pop_front() {
        if request.revision == revision {
            current.push_back(request);
        } else if let Some(id) = request.message.get("id") {
            server.write_error(
                id,
                -32800,
                "request cancelled because the validation revision changed",
            )?;
        }
    }
    *pending_requests = current;
    Ok(())
}

#[derive(Clone, Copy)]
enum RequestMethod {
    Initialize,
    Completion,
    Hover,
    Definition,
    DocumentSymbol,
    Formatting,
    SemanticTokens,
    Shutdown,
}

impl RequestMethod {
    fn classify(method: &str) -> Option<Self> {
        match method {
            "initialize" => Some(Self::Initialize),
            "textDocument/completion" => Some(Self::Completion),
            "textDocument/hover" => Some(Self::Hover),
            "textDocument/definition" => Some(Self::Definition),
            "textDocument/documentSymbol" => Some(Self::DocumentSymbol),
            "textDocument/formatting" => Some(Self::Formatting),
            "textDocument/semanticTokens/full" => Some(Self::SemanticTokens),
            "shutdown" => Some(Self::Shutdown),
            _ => None,
        }
    }

    const fn requires_snapshot(self) -> bool {
        !matches!(self, Self::Initialize | Self::Shutdown)
    }
}

fn request_requires_snapshot(message: &Value) -> bool {
    message.get("id").is_some()
        && message
            .get("method")
            .and_then(Value::as_str)
            .and_then(RequestMethod::classify)
            .is_some_and(RequestMethod::requires_snapshot)
}

struct LspServer<W> {
    core: LspValidationCore,
    writer: W,
    shutdown_requested: bool,
    should_exit: bool,
}

fn cfd_definition(document: &validation::CfdRequestDocument<'_>, offset: usize) -> Value {
    let schema_location = cfd::definition_type_name(document.ast, offset)
        .and_then(|type_name| {
            document
                .build
                .and_then(|build| cft_type_definition_location(build, type_name))
        })
        .or_else(|| {
            cfd::definition_field_name(document.ast, document.schema, offset).and_then(
                |(type_name, field_name)| {
                    document.build.and_then(|build| {
                        cft_schema_field_definition_location(build, &type_name, field_name)
                    })
                },
            )
        });
    schema_location.map_or_else(
        || {
            cfd::definition_ref_target(document.ast, document.schema, offset)
                .and_then(|(target_type, ref_key)| {
                    document.build.and_then(|build| {
                        cfd_record_definition_location(build, &target_type, &ref_key)
                    })
                })
                .map_or(Value::Null, |location| json!(location))
        },
        |location| json!(location),
    )
}

impl<W: Write> LspServer<W> {
    const fn new(project: Project, writer: W) -> Self {
        Self {
            core: LspValidationCore::new(project),
            writer,
            shutdown_requested: false,
            should_exit: false,
        }
    }

    fn handle_message(&mut self, message: &Value) -> Result<(), String> {
        let id = message.get("id").cloned();
        match self.handle_message_inner(message) {
            Ok(()) => Ok(()),
            Err(err) if is_fatal_lsp_handler_error(&err) => Err(err),
            Err(err) => {
                if let Some(id) = id {
                    self.write_error(&id, -32603, &err)
                } else {
                    self.write_log_message(1, &err)
                }
            }
        }
    }

    fn handle_message_inner(&mut self, message: &Value) -> Result<(), String> {
        let id = message.get("id").cloned();
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            return Ok(());
        };
        let params = message.get("params").unwrap_or(&Value::Null);
        let request_method = RequestMethod::classify(method);

        if self.shutdown_requested && method != "exit" {
            return id.map_or(Ok(()), |id| {
                self.write_error(
                    &id,
                    -32600,
                    "server is shut down; only the exit notification is accepted",
                )
            });
        }

        match (id, request_method, method) {
            (Some(id), Some(RequestMethod::Initialize), _) => self.initialize(&id),
            (Some(id), Some(RequestMethod::Completion), _) => self.completion(&id, params),
            (Some(id), Some(RequestMethod::Hover), _) => self.hover(&id, params),
            (Some(id), Some(RequestMethod::Definition), _) => self.definition(&id, params),
            (Some(id), Some(RequestMethod::DocumentSymbol), _) => self.document_symbol(&id, params),
            (Some(id), Some(RequestMethod::Formatting), _) => self.formatting(&id, params),
            (Some(id), Some(RequestMethod::SemanticTokens), _) => self.semantic_tokens(&id, params),
            (Some(id), Some(RequestMethod::Shutdown), _) => {
                self.shutdown_requested = true;
                self.write_response(&id, &Value::Null)
            }
            (None, _, "exit") => {
                self.should_exit = true;
                Ok(())
            }
            (None, _, "textDocument/didOpen") => {
                if let Some((uri, text, version)) = did_open_document(params) {
                    self.open_document(uri, text, version)?;
                }
                Ok(())
            }
            (None, _, "textDocument/didChange") => {
                if let Some((uri, text, version)) = did_change_document(params) {
                    self.change_document(uri, text, version)?;
                }
                Ok(())
            }
            (None, _, "textDocument/didSave") => {
                if let Some((uri, text)) = did_save_document(params) {
                    if let Some(text) = text {
                        self.change_document(uri, text, None)?;
                    } else {
                        self.refresh_project()?;
                    }
                }
                Ok(())
            }
            (None, _, "textDocument/didClose") => {
                if let Some(uri) = text_document_uri(params) {
                    self.close_document(&uri)?;
                }
                Ok(())
            }
            (None, _, "workspace/didChangeWatchedFiles") => self.watched_files_changed(params),
            (Some(id), None, _) => {
                self.write_error(&id, -32601, &format!("method `{method}` not found"))
            }
            (None, _, _) => Ok(()),
        }
    }

    fn initialize(&mut self, id: &Value) -> Result<(), String> {
        self.write_response(
            id,
            &json!({
                "capabilities": {
                    "textDocumentSync": {
                        "openClose": true,
                        "change": 1,
                        "save": {
                            "includeText": false
                        }
                    },
                    "completionProvider": {
                        "triggerCharacters": [".", "@", ":", " ", "("],
                        "resolveProvider": false
                    },
                    "hoverProvider": true,
                    "definitionProvider": true,
                    "documentSymbolProvider": true,
                    "documentFormattingProvider": true,
                    "semanticTokensProvider": {
                        "legend": {
                            "tokenTypes": SEMANTIC_TOKEN_TYPES,
                            "tokenModifiers": SEMANTIC_TOKEN_MODIFIERS
                        },
                        "full": true
                    }
                },
                "serverInfo": {
                    "name": "coflow-lsp",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }),
        )
    }

    fn open_document(
        &mut self,
        uri: String,
        text: String,
        version: Option<i64>,
    ) -> Result<(), String> {
        let publications = self.core.open_document(uri, text, version)?;
        self.publish_diagnostic_publications(publications)
    }

    fn change_document(
        &mut self,
        uri: String,
        text: String,
        version: Option<i64>,
    ) -> Result<(), String> {
        let publications = self.core.change_document(uri, text, version)?;
        self.publish_diagnostic_publications(publications)
    }

    fn close_document(&mut self, uri: &str) -> Result<(), String> {
        let publications = self.core.close_document(uri)?;
        self.publish_diagnostic_publications(publications)
    }

    #[cfg(test)]
    fn validate_project(&mut self) -> Result<(), String> {
        let publications = self.core.validate_project();
        self.publish_diagnostic_publications(publications)
    }

    fn refresh_project(&mut self) -> Result<(), String> {
        let publications = self.core.refresh_project()?;
        self.publish_diagnostic_publications(publications)
    }

    fn watched_files_changed(&mut self, params: &Value) -> Result<(), String> {
        if !self
            .core
            .apply_watched_files(&did_change_watched_files(params))?
        {
            return Ok(());
        }
        let publications = self.core.validate_project();
        self.publish_diagnostic_publications(publications)
    }

    fn completion(&mut self, id: &Value, params: &Value) -> Result<(), String> {
        let Some(request) = TextRequest::from_params(params) else {
            return self.write_response(id, &Value::Null);
        };
        let result = match self.request_document(&request.uri)? {
            LspRequestDocument::Cfd(document) => {
                let offset = byte_offset_from_position(document.source, request.position);
                cfd::completion(document.source, document.ast, document.schema, offset)
            }
            LspRequestDocument::Cft { build, document } => {
                json!(completion_items(build, document, &request.position))
            }
            LspRequestDocument::Missing => json!([]),
        };
        self.write_response(id, &result)
    }

    fn hover(&mut self, id: &Value, params: &Value) -> Result<(), String> {
        let Some(request) = TextRequest::from_params(params) else {
            return self.write_response(id, &Value::Null);
        };
        let result = match self.request_document(&request.uri)? {
            LspRequestDocument::Cfd(document) => {
                let offset = byte_offset_from_position(document.source, request.position);
                cfd::hover(document.source, document.ast, document.schema, offset)
            }
            LspRequestDocument::Cft { build, document } => {
                hover_at(build, document, &request.position).unwrap_or(Value::Null)
            }
            LspRequestDocument::Missing => Value::Null,
        };
        self.write_response(id, &result)
    }

    fn definition(&mut self, id: &Value, params: &Value) -> Result<(), String> {
        let Some(request) = TextRequest::from_params(params) else {
            return self.write_response(id, &Value::Null);
        };
        let result = match self.request_document(&request.uri)? {
            LspRequestDocument::Cfd(document) => {
                let offset = byte_offset_from_position(document.source, request.position);
                cfd_definition(&document, offset)
            }
            LspRequestDocument::Cft { build, document } => {
                json!(definitions_at(build, document, &request.position))
            }
            LspRequestDocument::Missing => Value::Null,
        };
        self.write_response(id, &result)
    }

    fn document_symbol(&mut self, id: &Value, params: &Value) -> Result<(), String> {
        let Some(uri) = text_document_uri(params) else {
            return self.write_response(id, &json!([]));
        };
        let result = match self.request_document(&uri)? {
            LspRequestDocument::Cfd(document) => {
                cfd::document_symbols(document.source, document.ast)
            }
            LspRequestDocument::Cft { document, .. } => json!(document_symbols(document)),
            LspRequestDocument::Missing => json!([]),
        };
        self.write_response(id, &result)
    }

    fn formatting(&mut self, id: &Value, params: &Value) -> Result<(), String> {
        let Some(uri) = text_document_uri(params) else {
            return self.write_response(id, &Value::Null);
        };
        let result = {
            let Some(build) = self.ensure_build()? else {
                return self.write_response(id, &json!([]));
            };
            let Some(document) = build.document_by_uri(&uri) else {
                return self.write_response(id, &json!([]));
            };
            let formatted = format_cft(&document.source);
            if formatted == document.source {
                json!([])
            } else {
                json!([{
                    "range": full_document_range(&document.source),
                    "newText": formatted
                }])
            }
        };
        self.write_response(id, &result)
    }

    fn semantic_tokens(&mut self, id: &Value, params: &Value) -> Result<(), String> {
        let Some(uri) = text_document_uri(params) else {
            return self.write_response(id, &json!({"data": []}));
        };
        let result = match self.request_document(&uri)? {
            LspRequestDocument::Cfd(document) => {
                cfd::semantic_tokens(document.source, document.ast)
            }
            LspRequestDocument::Cft { build, document } => {
                json!({
                    "data": semantic_token_data(build, document)
                })
            }
            LspRequestDocument::Missing => json!({"data": []}),
        };
        self.write_response(id, &result)
    }

    fn request_document(&mut self, uri: &str) -> Result<LspRequestDocument<'_>, String> {
        let publications = self.core.prepare_request_document(uri);
        self.publish_diagnostic_publications(publications)?;
        Ok(self.core.request_document(uri))
    }

    fn ensure_build(&mut self) -> Result<Option<&LspBuild>, String> {
        let publications = self.core.ensure_build_publications();
        self.publish_diagnostic_publications(publications)?;
        Ok(self.core.build())
    }

    fn publish_diagnostics(
        &mut self,
        uri: &str,
        diagnostics: &[Value],
        version: Option<i64>,
    ) -> Result<(), String> {
        let params = version.map_or_else(
            || json!({ "uri": uri, "diagnostics": diagnostics }),
            |version| json!({ "uri": uri, "version": version, "diagnostics": diagnostics }),
        );
        self.write_notification("textDocument/publishDiagnostics", &params)
    }

    fn publish_diagnostic_publications(
        &mut self,
        publications: Vec<DiagnosticPublication>,
    ) -> Result<(), String> {
        for publication in publications {
            self.publish_diagnostics(
                &publication.uri,
                &publication.diagnostics,
                publication.version,
            )?;
        }
        Ok(())
    }

    fn write_response(&mut self, id: &Value, result: &Value) -> Result<(), String> {
        self.write_json(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result
        }))
    }

    fn write_error(&mut self, id: &Value, code: i64, message: &str) -> Result<(), String> {
        self.write_json(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": code,
                "message": message
            }
        }))
    }

    fn write_parse_error(&mut self, message: &str) -> Result<(), String> {
        self.write_json(&json!({
            "jsonrpc": "2.0",
            "id": Value::Null,
            "error": {
                "code": -32700,
                "message": message
            }
        }))
    }

    fn write_notification(&mut self, method: &str, params: &Value) -> Result<(), String> {
        self.write_json(&json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        }))
    }

    fn write_log_message(&mut self, message_type: u8, message: &str) -> Result<(), String> {
        self.write_notification(
            "window/logMessage",
            &json!({
                "type": message_type,
                "message": message
            }),
        )
    }

    fn write_json(&mut self, value: &Value) -> Result<(), String> {
        let body = serde_json::to_vec(value)
            .map_err(|err| format!("failed to serialize LSP message: {err}"))?;
        write!(self.writer, "Content-Length: {}\r\n\r\n", body.len())
            .map_err(|err| format!("failed to write LSP header: {err}"))?;
        self.writer
            .write_all(&body)
            .map_err(|err| format!("failed to write LSP body: {err}"))?;
        self.writer
            .flush()
            .map_err(|err| format!("failed to flush LSP message: {err}"))
    }
}

const MAX_LSP_CONTENT_LENGTH: usize = 16 * 1024 * 1024;

fn is_fatal_lsp_handler_error(message: &str) -> bool {
    message.starts_with("failed to write LSP")
        || message.starts_with("failed to flush LSP")
        || message.starts_with("failed to serialize LSP")
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic, clippy::too_many_lines)]
mod tests {
    use super::*;
    use crate::position::position_from_byte;

    mod cfd_tests;
    mod cft;
    mod common;
    mod protocol;
    mod semantic;
}
