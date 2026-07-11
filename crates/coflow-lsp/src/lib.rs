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

use coflow_project::Project;
#[cfg(test)]
use coflow_project::normalize_path;
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
use document_symbols::document_symbols;
pub(crate) use documentation::is_builtin_name;
use formatting::format_cft;
use hover::hover_at;
#[cfg(test)]
use diagnostics::{label_uri, lsp_diagnostic, lsp_label_location};
use position::{
    byte_offset_from_position, byte_range, full_document_range, range_from_span, LspPosition,
};
use protocol::{
    did_change_document, did_open_document, did_save_document, read_message, text_document_uri,
    TextRequest,
};
use semantic_tokens::{semantic_token_data, SEMANTIC_TOKEN_MODIFIERS, SEMANTIC_TOKEN_TYPES};
use serde_json::{json, Value};
#[cfg(test)]
use std::collections::BTreeMap;
pub(crate) use state::{
    current_field_at, current_type_at, enum_name_exists, enum_variant_by_chain,
    enum_variant_exists, field_by_chain, field_by_type, quantifier_bindings_at,
    type_name_of_schema_ref, type_of_chain, LspBuild, LspDocument,
};
use std::io::{self, BufReader, Write};
#[cfg(test)]
use std::path::PathBuf;
pub(crate) use text::{
    dotted_chain_at, is_ident_continue, is_trivia_position, last_ident, line_prefix_at,
    parse_dotted_ident_chain, previous_char, word_at,
};
use uri::path_from_file_uri;
#[cfg(test)]
pub(crate) use uri::path_to_file_uri;
pub(crate) use validation::{
    CfdProjectSource, DiagnosticPublication, LspRequestDocument, LspValidationCore,
};

/// Runs the CFT language server over stdio.
///
/// # Errors
///
/// Returns an error when reading LSP messages, parsing JSON, handling a request,
/// or writing a response fails.
pub fn run(project: Project) -> Result<bool, String> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut server = LspServer::new(project, stdout.lock());
    let mut reader = BufReader::new(stdin.lock());

    while let Some(bytes) = read_message(&mut reader)? {
        let message: Value = match serde_json::from_slice(&bytes) {
            Ok(message) => message,
            Err(err) => {
                server.write_parse_error(&format!("failed to parse LSP JSON message: {err}"))?;
                continue;
            }
        };
        server.handle_message(&message)?;
        if server.should_exit {
            break;
        }
    }

    Ok(true)
}

struct LspServer<W> {
    core: LspValidationCore,
    writer: W,
    shutdown_requested: bool,
    should_exit: bool,
}

impl<W: Write> LspServer<W> {
    fn new(project: Project, writer: W) -> Self {
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

        if self.shutdown_requested && method != "exit" {
            return id.map_or(Ok(()), |id| {
                self.write_error(
                    &id,
                    -32600,
                    "server is shut down; only the exit notification is accepted",
                )
            });
        }

        match (id, method) {
            (Some(id), "initialize") => self.initialize(&id),
            (Some(id), "textDocument/completion") => self.completion(&id, params),
            (Some(id), "textDocument/hover") => self.hover(&id, params),
            (Some(id), "textDocument/definition") => self.definition(&id, params),
            (Some(id), "textDocument/documentSymbol") => self.document_symbol(&id, params),
            (Some(id), "textDocument/formatting") => self.formatting(&id, params),
            (Some(id), "textDocument/semanticTokens/full") => self.semantic_tokens(&id, params),
            (Some(id), "shutdown") => {
                self.shutdown_requested = true;
                self.write_response(&id, &Value::Null)
            }
            (None, "exit") => {
                self.should_exit = true;
                Ok(())
            }
            (None, "textDocument/didOpen") => {
                if let Some((uri, text)) = did_open_document(params) {
                    self.open_document(uri, text)?;
                }
                Ok(())
            }
            (None, "textDocument/didChange") => {
                if let Some((uri, text)) = did_change_document(params) {
                    self.change_document(uri, text)?;
                }
                Ok(())
            }
            (None, "textDocument/didSave") => {
                if let Some((uri, text)) = did_save_document(params) {
                    if let Some(text) = text {
                        self.change_document(uri, text)?;
                    } else {
                        self.validate_project()?;
                    }
                }
                Ok(())
            }
            (None, "textDocument/didClose") => {
                if let Some(uri) = text_document_uri(params) {
                    self.close_document(&uri)?;
                }
                Ok(())
            }
            (Some(id), _) => self.write_error(&id, -32601, &format!("method `{method}` not found")),
            (None, _) => Ok(()),
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

    fn open_document(&mut self, uri: String, text: String) -> Result<(), String> {
        let publications = self.core.open_document(uri, text)?;
        self.publish_diagnostic_publications(publications)
    }

    fn change_document(&mut self, uri: String, text: String) -> Result<(), String> {
        let publications = self.core.change_document(uri, text)?;
        self.publish_diagnostic_publications(publications)
    }

    fn close_document(&mut self, uri: &str) -> Result<(), String> {
        let publications = self.core.close_document(uri)?;
        self.publish_diagnostic_publications(publications)
    }

    fn validate_project(&mut self) -> Result<(), String> {
        let publications = self.core.validate_project()?;
        self.publish_diagnostic_publications(publications)
    }

    fn completion(&mut self, id: &Value, params: &Value) -> Result<(), String> {
        let Some(request) = TextRequest::from_params(params) else {
            return self.write_response(id, &Value::Null);
        };
        let result = match self.request_document(&request.uri)? {
            LspRequestDocument::Cfd(document) => {
                let offset = byte_offset_from_position(&document.source, request.position);
                cfd::completion(&document.source, &document.ast, document.schema, offset)
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
                let offset = byte_offset_from_position(&document.source, request.position);
                cfd::hover(&document.source, &document.ast, document.schema, offset)
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
                let offset = byte_offset_from_position(&document.source, request.position);
                if let Some(location) = cfd::definition_type_name(&document.ast, offset).and_then(
                    |type_name| {
                        document
                            .build
                            .and_then(|build| cft_type_definition_location(build, type_name))
                    },
                ) {
                    json!(location)
                } else if let Some(location) =
                    cfd::definition_field_name(&document.ast, document.schema, offset).and_then(
                        |(type_name, field_name)| {
                            document.build.and_then(|build| {
                                cft_schema_field_definition_location(
                                    build, &type_name, field_name,
                                )
                            })
                        },
                    )
                {
                    json!(location)
                } else if let Some(ref_key) = cfd::definition_ref_key(&document.ast, offset) {
                    let sources = self.core.cfd_project_sources();
                    cfd_record_definition_location(&sources, ref_key).map_or(Value::Null, |location| {
                        json!(location)
                    })
                } else {
                    Value::Null
                }
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
                cfd::document_symbols(&document.source, &document.ast)
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
                cfd::semantic_tokens(&document.source, &document.ast)
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
        let publications = self.core.prepare_request_document(uri)?;
        self.publish_diagnostic_publications(publications)?;
        Ok(self.core.request_document(uri))
    }

    fn ensure_build(&mut self) -> Result<Option<&LspBuild>, String> {
        let publications = self.core.ensure_build_publications()?;
        self.publish_diagnostic_publications(publications)?;
        Ok(self.core.build())
    }

    fn publish_diagnostics(&mut self, uri: &str, diagnostics: &[Value]) -> Result<(), String> {
        self.write_notification(
            "textDocument/publishDiagnostics",
            &json!({
                "uri": uri,
                "diagnostics": diagnostics
            }),
        )
    }

    fn publish_diagnostic_publications(
        &mut self,
        publications: Vec<DiagnosticPublication>,
    ) -> Result<(), String> {
        for publication in publications {
            self.publish_diagnostics(&publication.uri, &publication.diagnostics)?;
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
