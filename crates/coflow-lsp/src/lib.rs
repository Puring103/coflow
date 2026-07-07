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
mod uri;

use coflow_cfd::parse_cfd;
use coflow_project::{
    compile_schema_project_with_overrides, dedupe_cft_diagnostics, diagnostic_set_from_cft,
    normalize_path, Project, SchemaSourceOverride,
};
use definition::{
    cfd_record_definition_location, cft_schema_field_definition_location,
    cft_type_definition_location, definitions_at,
};
#[cfg(test)]
pub(crate) use definition::field_location_by_chain;
use completion::completion_items;
#[cfg(test)]
pub(crate) use completion::{
    annotation_completion_items, check_expression_completion_items, completion_scope,
    dot_completion_items, top_level_completion_items, CompletionScope,
};
use diagnostics::{
    label_uri, lsp_diagnostic, lsp_error_diagnostic, lsp_label_location, preferred_diagnostic_uri,
};
use document_symbols::document_symbols;
pub(crate) use documentation::is_builtin_name;
use formatting::format_cft;
use hover::hover_at;
use position::{
    byte_offset_from_position, byte_range, full_document_range, range_from_span, LspPosition,
};
use protocol::{
    did_change_document, did_open_document, did_save_document, read_message, text_document_uri,
    TextRequest,
};
use semantic_tokens::{
    semantic_token_data,
    SEMANTIC_TOKEN_MODIFIERS, SEMANTIC_TOKEN_TYPES,
};
pub(crate) use state::{
    current_field_at, current_type_at, enum_name_exists, enum_variant_by_chain,
    enum_variant_exists, field_by_chain, field_by_type, quantifier_bindings_at,
    type_name_of_schema_ref, type_of_chain, LspBuild, LspDocument,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, BufReader, Write};
use std::path::{Path, PathBuf};
use uri::path_from_file_uri;
#[cfg(test)]
pub(crate) use uri::path_to_file_uri;

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
    project: Project,
    writer: W,
    open_documents: BTreeMap<PathBuf, OpenDocument>,
    published_uris: BTreeSet<String>,
    build: Option<LspBuild>,
    shutdown_requested: bool,
    should_exit: bool,
}

impl<W: Write> LspServer<W> {
    const fn new(project: Project, writer: W) -> Self {
        Self {
            project,
            writer,
            open_documents: BTreeMap::new(),
            published_uris: BTreeSet::new(),
            build: None,
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
        if let Some(path) = path_from_file_uri(&uri) {
            self.open_documents
                .insert(normalize_path(&path), OpenDocument { uri, text });
            self.validate_project()?;
        }
        Ok(())
    }

    fn change_document(&mut self, uri: String, text: String) -> Result<(), String> {
        if let Some(path) = path_from_file_uri(&uri) {
            let normalized = normalize_path(&path);
            self.open_documents
                .entry(normalized)
                .and_modify(|document| document.text.clone_from(&text))
                .or_insert(OpenDocument { uri, text });
            self.validate_project()?;
        }
        Ok(())
    }

    fn close_document(&mut self, uri: &str) -> Result<(), String> {
        let Some(path) = path_from_file_uri(uri) else {
            return Ok(());
        };
        self.open_documents.remove(&normalize_path(&path));
        self.publish_diagnostics(uri, &[])?;
        self.validate_project()
    }

    fn validate_project(&mut self) -> Result<(), String> {
        let schema_files = self.project.schema_files()?;
        let mut schema_by_path = BTreeMap::new();

        for file in &schema_files {
            schema_by_path.insert(
                normalize_path(&file.canonical_path),
                (file.module_id.clone(), file.canonical_path.clone()),
            );
        }

        let mut overrides = Vec::new();
        let mut non_schema_diagnostics = Vec::new();

        for (normalized_path, document) in &self.open_documents {
            if let Some((module_id, _)) = schema_by_path.get(normalized_path) {
                overrides.push(SchemaSourceOverride {
                    requested_module: Some(module_id.clone()),
                    normalized_path: normalized_path.clone(),
                    source: document.text.clone(),
                });
            } else if is_cfd_path(normalized_path) {
                let (_, errors) = parse_cfd(&document.text);
                let diags = cfd::syntax_diagnostics(&document.text, &errors);
                non_schema_diagnostics.push((document.uri.clone(), diags));
            } else {
                non_schema_diagnostics.push((
                    document.uri.clone(),
                    vec![lsp_error_diagnostic(
                        "CFT-LSP",
                        "file is not part of the configured CFT schema",
                    )],
                ));
            }
        }

        let preferred_uris = self.preferred_diagnostic_uris(&schema_by_path);
        let raw_build = compile_schema_project_with_overrides(&self.project, &overrides)?;
        let build = LspBuild::new(raw_build);
        let diagnostics = dedupe_cft_diagnostics(build.schema.diagnostics.clone());
        let mut by_uri: BTreeMap<String, Vec<Value>> = BTreeMap::new();

        let diagnostic_set =
            diagnostic_set_from_cft(diagnostics, &build.schema.sources, &build.schema.paths);
        for diagnostic in &diagnostic_set {
            let uri = diagnostic
                .primary
                .as_ref()
                .map(|label| lsp_label_location(&label.location))
                .map_or_else(
                    || preferred_diagnostic_uri(&preferred_uris, Path::new("")),
                    |location| label_uri(&location, &preferred_uris),
                );
            by_uri
                .entry(uri)
                .or_default()
                .push(lsp_diagnostic(diagnostic));
        }

        let mut touched_uris = self.published_uris.clone();
        for path in build.schema.paths.values() {
            touched_uris.insert(preferred_diagnostic_uri(&preferred_uris, Path::new(path)));
        }
        for document in self.open_documents.values() {
            touched_uris.insert(document.uri.clone());
        }
        for (uri, diagnostics) in non_schema_diagnostics {
            by_uri.insert(uri.clone(), diagnostics);
            touched_uris.insert(uri);
        }

        for uri in touched_uris {
            let diagnostics = by_uri.remove(&uri).unwrap_or_default();
            self.publish_diagnostics(&uri, &diagnostics)?;
        }

        self.build = Some(build);
        Ok(())
    }

    fn preferred_diagnostic_uris(
        &self,
        schema_by_path: &BTreeMap<PathBuf, (String, PathBuf)>,
    ) -> BTreeMap<PathBuf, String> {
        let mut preferred = BTreeMap::new();
        for (normalized_path, document) in &self.open_documents {
            if schema_by_path.contains_key(normalized_path) {
                preferred.insert(normalized_path.clone(), document.uri.clone());
            }
        }
        preferred
    }

    fn completion(&mut self, id: &Value, params: &Value) -> Result<(), String> {
        let Some(request) = TextRequest::from_params(params) else {
            return self.write_response(id, &Value::Null);
        };
        if let Some(source) = self.cfd_source_by_uri(&request.uri) {
            let (ast, _) = parse_cfd(&source);
            let offset = byte_offset_from_position(&source, request.position);
            let schema = self.schema();
            let result = cfd::completion(&source, &ast, schema, offset);
            return self.write_response(id, &result);
        }
        let Some(build) = self.ensure_build()? else {
            return self.write_response(id, &json!([]));
        };
        let Some(document) = build.document_by_uri(&request.uri) else {
            return self.write_response(id, &json!([]));
        };
        let items = completion_items(build, document, &request.position);
        self.write_response(id, &json!(items))
    }

    fn hover(&mut self, id: &Value, params: &Value) -> Result<(), String> {
        let Some(request) = TextRequest::from_params(params) else {
            return self.write_response(id, &Value::Null);
        };
        if let Some(source) = self.cfd_source_by_uri(&request.uri) {
            let (ast, _) = parse_cfd(&source);
            let offset = byte_offset_from_position(&source, request.position);
            let schema = self.schema();
            let result = cfd::hover(&source, &ast, schema, offset);
            return self.write_response(id, &result);
        }
        let Some(build) = self.ensure_build()? else {
            return self.write_response(id, &Value::Null);
        };
        let Some(document) = build.document_by_uri(&request.uri) else {
            return self.write_response(id, &Value::Null);
        };
        let hover = hover_at(build, document, &request.position);
        self.write_response(id, &hover.unwrap_or(Value::Null))
    }

    fn definition(&mut self, id: &Value, params: &Value) -> Result<(), String> {
        let Some(request) = TextRequest::from_params(params) else {
            return self.write_response(id, &Value::Null);
        };
        if let Some(source) = self.cfd_source_by_uri(&request.uri) {
            let (ast, _) = parse_cfd(&source);
            let offset = byte_offset_from_position(&source, request.position);
            if let Some(type_name) = cfd::definition_type_name(&ast, offset) {
                let type_name = type_name.to_string();
                self.ensure_build()?;
                if let Some(build) = &self.build {
                    if let Some(location) = cft_type_definition_location(build, &type_name) {
                        return self.write_response(id, &json!(location));
                    }
                }
            }
            if let Some((type_name, field_name)) =
                cfd::definition_field_name(&ast, self.schema(), offset)
            {
                self.ensure_build()?;
                if let Some(build) = &self.build {
                    if let Some(location) =
                        cft_schema_field_definition_location(build, &type_name, field_name)
                    {
                        return self.write_response(id, &json!(location));
                    }
                }
            }
            if let Some(ref_key) = cfd::definition_ref_key(&ast, offset) {
                let ref_key = ref_key.to_string();
                if let Some(location) =
                    cfd_record_definition_location(&self.project, &self.open_documents, &ref_key)
                {
                    return self.write_response(id, &json!(location));
                }
            }
            return self.write_response(id, &Value::Null);
        }
        let Some(build) = self.ensure_build()? else {
            return self.write_response(id, &Value::Null);
        };
        let Some(document) = build.document_by_uri(&request.uri) else {
            return self.write_response(id, &Value::Null);
        };
        let definitions = definitions_at(build, document, &request.position);
        self.write_response(id, &json!(definitions))
    }

    fn document_symbol(&mut self, id: &Value, params: &Value) -> Result<(), String> {
        let Some(uri) = text_document_uri(params) else {
            return self.write_response(id, &json!([]));
        };
        if let Some(source) = self.cfd_source_by_uri(&uri) {
            let (ast, _) = parse_cfd(&source);
            return self.write_response(id, &cfd::document_symbols(&source, &ast));
        }
        let result = {
            let Some(build) = self.ensure_build()? else {
                return self.write_response(id, &json!([]));
            };
            let Some(document) = build.document_by_uri(&uri) else {
                return self.write_response(id, &json!([]));
            };
            json!(document_symbols(document))
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
        if let Some(source) = self.cfd_source_by_uri(&uri) {
            let (ast, _) = parse_cfd(&source);
            return self.write_response(id, &cfd::semantic_tokens(&source, &ast));
        }
        let result = {
            let Some(build) = self.ensure_build()? else {
                return self.write_response(id, &json!({"data": []}));
            };
            let Some(document) = build.document_by_uri(&uri) else {
                return self.write_response(id, &json!({"data": []}));
            };
            json!({
                "data": semantic_token_data(build, document)
            })
        };
        self.write_response(id, &result)
    }

    fn ensure_build(&mut self) -> Result<Option<&LspBuild>, String> {
        if self.build.is_none() {
            self.validate_project()?;
        }
        Ok(self.build.as_ref())
    }

    /// Return the source text of an open `.cfd` document by URI, or `None`
    /// when the URI does not correspond to an open CFD file.
    fn cfd_source_by_uri(&self, uri: &str) -> Option<String> {
        let path = path_from_file_uri(uri)?;
        if !is_cfd_path(&path) {
            return None;
        }
        let normalized = normalize_path(&path);
        self.open_documents
            .get(&normalized)
            .map(|doc| doc.text.clone())
    }

    /// Return the compiled schema from the current build, if available.
    fn schema(&self) -> Option<&coflow_cft::CftContainer> {
        self.build
            .as_ref()
            .and_then(|b| b.schema.container.as_ref())
    }

    fn publish_diagnostics(&mut self, uri: &str, diagnostics: &[Value]) -> Result<(), String> {
        self.published_uris.insert(uri.to_string());
        self.write_notification(
            "textDocument/publishDiagnostics",
            &json!({
                "uri": uri,
                "diagnostics": diagnostics
            }),
        )
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

#[derive(Debug)]
struct OpenDocument {
    pub(crate) uri: String,
    pub(crate) text: String,
}

const MAX_LSP_CONTENT_LENGTH: usize = 16 * 1024 * 1024;

fn is_fatal_lsp_handler_error(message: &str) -> bool {
    message.starts_with("failed to write LSP")
        || message.starts_with("failed to flush LSP")
        || message.starts_with("failed to serialize LSP")
}

pub(crate) struct WordAt {
    text: String,
    start: usize,
    end: usize,
}

pub(crate) fn is_trivia_position(source: &str, offset: usize) -> bool {
    let line_prefix = line_prefix_at(source, offset);
    if is_after_line_comment(line_prefix) {
        return true;
    }
    is_inside_string(source, offset)
}

fn is_inside_string(source: &str, offset: usize) -> bool {
    let line_start = source[..offset].rfind('\n').map_or(0, |index| index + 1);
    let mut in_string = false;
    let mut escaped = false;
    for ch in source[line_start..offset].chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if in_string && ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
        }
    }
    in_string
}

pub(crate) fn parse_dotted_ident_chain(text: &str) -> Option<Vec<String>> {
    let mut parts = Vec::new();
    for part in text.split('.') {
        let trimmed = part.trim();
        if trimmed.is_empty() || !trimmed.chars().all(is_ident_continue) {
            return None;
        }
        parts.push(trimmed.to_string());
    }
    (!parts.is_empty()).then_some(parts)
}

fn is_after_line_comment(line_prefix: &str) -> bool {
    let mut in_string = false;
    let mut escaped = false;
    for ch in line_prefix.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if in_string && ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if !in_string && ch == '#' {
            return true;
        }
    }
    false
}

pub(crate) fn dotted_chain_at(source: &str, word: &WordAt) -> Option<Vec<String>> {
    let line_start = source[..word.start]
        .rfind('\n')
        .map_or(0, |index| index + 1);
    let line_end = source[word.end..]
        .find('\n')
        .map_or(source.len(), |index| word.end + index);
    let mut start = word.start;
    while start > line_start {
        let previous = previous_char(source, start)?;
        if previous.1 == '.' || previous.1.is_whitespace() || is_ident_continue(previous.1) {
            start = previous.0;
        } else {
            break;
        }
    }
    let mut end = word.end;
    while end < line_end {
        let Some(ch) = source[end..].chars().next() else {
            break;
        };
        if ch == '.' || ch.is_whitespace() || is_ident_continue(ch) {
            end += ch.len_utf8();
        } else {
            break;
        }
    }
    parse_dotted_ident_chain(&source[start..end])
}

pub(crate) fn word_at(source: &str, offset: usize) -> Option<WordAt> {
    let mut start = offset.min(source.len());
    if start == source.len()
        || !source[start..]
            .chars()
            .next()
            .is_some_and(is_ident_continue)
    {
        if let Some((previous, ch)) = previous_char(source, start) {
            if is_ident_continue(ch) {
                start = previous;
            }
        }
    }
    while let Some((previous, ch)) = previous_char(source, start) {
        if is_ident_continue(ch) {
            start = previous;
        } else {
            break;
        }
    }
    let mut end = start;
    while end < source.len() {
        let Some(ch) = source[end..].chars().next() else {
            break;
        };
        if is_ident_continue(ch) {
            end += ch.len_utf8();
        } else {
            break;
        }
    }
    (end > start).then(|| WordAt {
        text: source[start..end].to_string(),
        start,
        end,
    })
}

pub(crate) fn previous_char(source: &str, offset: usize) -> Option<(usize, char)> {
    source[..offset].char_indices().next_back()
}

pub(crate) fn is_ident_continue(ch: char) -> bool {
    ch == '_' || ch.is_alphanumeric()
}

pub(crate) fn last_ident(text: &str) -> Option<&str> {
    let end = text.trim_end().len();
    let mut start = end;
    while let Some((previous, ch)) = previous_char(text, start) {
        if is_ident_continue(ch) {
            start = previous;
        } else {
            break;
        }
    }
    (start < end).then_some(&text[start..end])
}

pub(crate) fn line_prefix_at(source: &str, offset: usize) -> &str {
    let start = source[..offset].rfind('\n').map_or(0, |index| index + 1);
    &source[start..offset]
}

fn is_cfd_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e == "cfd")
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
