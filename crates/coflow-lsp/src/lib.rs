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
mod position;
mod protocol;
mod semantic_tokens;
mod uri;

use coflow_cfd::parse_cfd;
use coflow_cft::ast::{
    Annotation, AnnotationArg, CheckExpr, CheckExprKind, CheckStmt, ConstLiteral, DefaultExpr,
    DefaultExprKind, Item, TypeRef, TypeRefKind,
};
use coflow_cft::lexer::{lex, TokenKind};
use coflow_cft::parser::parse_module;
use coflow_cft::{
    CftConstValue, CftSchemaEnum, CftSchemaEnumVariant, CftSchemaField, CftSchemaType,
    CftSchemaTypeRef, ModuleId, Span,
};
use coflow_project::{
    compile_schema_project_with_overrides, dedupe_cft_diagnostics, diagnostic_set_from_cft,
    normalize_path, Project, SchemaBuild, SchemaSourceOverride,
};
use definition::{
    cfd_record_definition_location, cft_schema_field_definition_location,
    cft_type_definition_location,
};
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
use documentation::{annotation_documentation, is_builtin_name, static_documentation};
use formatting::format_cft;
use position::{
    byte_offset_from_position, byte_range, full_document_range, range_from_span, LspPosition,
};
use protocol::{
    did_change_document, did_open_document, did_save_document, read_message, text_document_uri,
    TextRequest,
};
use semantic_tokens::{
    add_comment_semantic_tokens, encode_semantic_tokens, push_semantic_span,
    push_semantic_span_plain, RawSemanticToken, MOD_DECLARATION, MOD_PATH, MOD_REFERENCE,
    MOD_SCHEMA, SEM_DECORATOR, SEM_ENUM, SEM_ENUM_MEMBER, SEM_FUNCTION, SEM_KEYWORD, SEM_NUMBER,
    SEM_OPERATOR, SEM_PARAMETER, SEM_PROPERTY, SEM_STRING, SEM_TYPE, SEM_VARIABLE,
    SEMANTIC_TOKEN_MODIFIERS, SEMANTIC_TOKEN_TYPES,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::io::{self, BufReader, Write};
use std::path::{Path, PathBuf};
use uri::{path_from_file_uri, path_to_file_uri};

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

pub(crate) struct LspBuild {
    schema: SchemaBuild,
    documents: BTreeMap<String, LspDocument>,
    module_by_uri: BTreeMap<String, String>,
    module_by_path: BTreeMap<PathBuf, String>,
}

pub(crate) struct LspDocument {
    module_id: String,
    uri: String,
    pub(crate) source: String,
    pub(crate) ast: Option<coflow_cft::ast::ModuleAst>,
}

impl LspBuild {
    fn new(schema: SchemaBuild) -> Self {
        let mut documents = BTreeMap::new();
        let mut module_by_uri = BTreeMap::new();
        let mut module_by_path = BTreeMap::new();

        for (module_id, source) in &schema.sources {
            let path = schema
                .paths
                .get(module_id)
                .map_or_else(|| PathBuf::from(module_id), PathBuf::from);
            let uri = path_to_file_uri(&path);
            let ast = parse_module(&ModuleId::new(module_id.clone()), source).ok();
            module_by_uri.insert(uri.clone(), module_id.clone());
            module_by_path.insert(normalize_path(&path), module_id.clone());
            documents.insert(
                module_id.clone(),
                LspDocument {
                    module_id: module_id.clone(),
                    uri,
                    source: source.clone(),
                    ast,
                },
            );
        }

        Self {
            schema,
            documents,
            module_by_uri,
            module_by_path,
        }
    }

    pub(crate) const fn container(&self) -> Option<&coflow_cft::CftContainer> {
        self.schema.container.as_ref()
    }

    fn document_by_uri(&self, uri: &str) -> Option<&LspDocument> {
        if let Some(module_id) = self.module_by_uri.get(uri) {
            return self.documents.get(module_id);
        }
        let path = path_from_file_uri(uri)?;
        let module_id = self.module_by_path.get(&normalize_path(&path))?;
        self.documents.get(module_id)
    }

    fn document_by_module(&self, module_id: &ModuleId) -> Option<&LspDocument> {
        self.documents.get(module_id.as_str())
    }
}

struct WordAt {
    text: String,
    start: usize,
    end: usize,
}

fn hover_at(build: &LspBuild, document: &LspDocument, position: &LspPosition) -> Option<Value> {
    let offset = byte_offset_from_position(&document.source, *position);
    if is_trivia_position(&document.source, offset) {
        return None;
    }
    if let Some(annotation) = annotation_at(document, offset) {
        if let Some((_, documentation)) = annotation_documentation(annotation) {
            return Some(hover_response(
                documentation,
                &range_from_span(&document.source, annotation.span),
            ));
        }
    }

    let word = word_at(&document.source, offset)?;
    if let Some(documentation) = static_documentation(&word.text) {
        return Some(hover_response(
            documentation,
            &byte_range(&document.source, word.start, word.end),
        ));
    }

    if let Some(chain) = dotted_chain_at(&document.source, &word) {
        if chain.len() == 2 {
            if let Some((enum_def, variant)) = enum_variant_by_chain(build, &chain) {
                return Some(hover_response(
                    &format!(
                        "CFT enum variant `{}`.`{}` = `{}`.",
                        enum_def.name, variant.name, variant.value
                    ),
                    &byte_range(&document.source, word.start, word.end),
                ));
            }
        }
        if let Some((type_name, field)) = field_by_chain(build, document, offset, &chain) {
            return Some(hover_response(
                &format!(
                    "CFT field `{}`.`{}`: `{}`.",
                    type_name, field.name, field.ty
                ),
                &byte_range(&document.source, word.start, word.end),
            ));
        }
    }

    if let Some(container) = build.container() {
        if let Some(ty) = container.resolve_type(&word.text) {
            return Some(hover_response(
                &type_hover_text(ty),
                &byte_range(&document.source, word.start, word.end),
            ));
        }
        if let Some(enum_def) = container.resolve_enum(&word.text) {
            return Some(hover_response(
                &format!(
                    "CFT enum `{}` with {} variant(s).",
                    enum_def.name,
                    enum_def.variants.len()
                ),
                &byte_range(&document.source, word.start, word.end),
            ));
        }
        if let Some(constant) = container.resolve_const(&word.text) {
            return Some(hover_response(
                &format!(
                    "CFT constant `{}` = `{}`.",
                    constant.name,
                    const_value_to_string(&constant.value)
                ),
                &byte_range(&document.source, word.start, word.end),
            ));
        }
        if let Some(current_type) = current_type_at(build, document, offset) {
            if let Some(field) = current_type
                .all_fields
                .iter()
                .find(|field| field.name == word.text)
            {
                return Some(hover_response(
                    &format!(
                        "CFT field `{}`.`{}`: `{}`.",
                        current_type.name, field.name, field.ty
                    ),
                    &byte_range(&document.source, word.start, word.end),
                ));
            }
        }
    }

    None
}

fn definitions_at(build: &LspBuild, document: &LspDocument, position: &LspPosition) -> Vec<Value> {
    let offset = byte_offset_from_position(&document.source, *position);
    if is_trivia_position(&document.source, offset) {
        return Vec::new();
    }
    let Some(word) = word_at(&document.source, offset) else {
        return Vec::new();
    };
    if is_builtin_name(&word.text) {
        return Vec::new();
    }

    if let Some(chain) = dotted_chain_at(&document.source, &word) {
        if chain.len() == 2 {
            if let Some(location) = enum_variant_location_by_chain(build, &chain) {
                return vec![location];
            }
            if let Some(location) = ast_enum_variant_location_by_chain(build, &chain) {
                return vec![location];
            }
        }
        if let Some(location) = field_location_by_chain(build, document, offset, &chain) {
            return vec![location];
        }
    }

    if let Some(location) = global_location(build, &word.text) {
        return vec![location];
    }

    if let Some(location) = ast_global_location(build, &word.text) {
        return vec![location];
    }

    if let Some(current_type) = current_type_at(build, document, offset) {
        if let Some(location) = field_location(build, &current_type.name, &word.text) {
            return vec![location];
        }
    }

    Vec::new()
}

fn semantic_token_data(build: &LspBuild, document: &LspDocument) -> Vec<u32> {
    encode_semantic_tokens(semantic_raw_tokens(build, document))
}

fn semantic_raw_tokens(build: &LspBuild, document: &LspDocument) -> Vec<RawSemanticToken> {
    let mut tokens = Vec::new();
    add_comment_semantic_tokens(&document.source, &mut tokens);
    if let Ok(lexed) = lex(&ModuleId::new(document.module_id.clone()), &document.source) {
        for token in lexed {
            add_lex_semantic_token(&document.source, &token.kind, token.span, &mut tokens);
        }
    }
    if let Some(ast) = &document.ast {
        add_ast_semantic_tokens(build, document, ast, &mut tokens);
    }
    tokens
}

fn add_lex_semantic_token(
    source: &str,
    kind: &TokenKind,
    span: Span,
    tokens: &mut Vec<RawSemanticToken>,
) {
    let token_type = match kind {
        TokenKind::Const
        | TokenKind::Enum
        | TokenKind::Type
        | TokenKind::Abstract
        | TokenKind::Sealed
        | TokenKind::Check
        | TokenKind::When
        | TokenKind::All
        | TokenKind::Any
        | TokenKind::None
        | TokenKind::In
        | TokenKind::Is
        | TokenKind::True
        | TokenKind::False
        | TokenKind::Null => SEM_KEYWORD,
        TokenKind::Int(_) | TokenKind::UIntOverflow(_) | TokenKind::Float(_) => SEM_NUMBER,
        TokenKind::String(_) => SEM_STRING,
        TokenKind::Plus
        | TokenKind::Minus
        | TokenKind::Star
        | TokenKind::Slash
        | TokenKind::SlashSlash
        | TokenKind::Percent
        | TokenKind::StarStar
        | TokenKind::Less
        | TokenKind::Greater
        | TokenKind::Bang
        | TokenKind::Tilde
        | TokenKind::Amp
        | TokenKind::Pipe
        | TokenKind::Caret
        | TokenKind::AmpAmp
        | TokenKind::PipePipe
        | TokenKind::LessEq
        | TokenKind::GreaterEq
        | TokenKind::LessLess
        | TokenKind::GreaterGreater
        | TokenKind::EqEq
        | TokenKind::BangEq
        | TokenKind::Equal => SEM_OPERATOR,
        _ => return,
    };
    push_semantic_span_plain(source, span, token_type, tokens);
}

fn add_ast_semantic_tokens(
    build: &LspBuild,
    document: &LspDocument,
    ast: &coflow_cft::ast::ModuleAst,
    tokens: &mut Vec<RawSemanticToken>,
) {
    for annotation in &ast.dangling_annotations {
        add_annotation_semantic(document, annotation, tokens);
    }
    for item in &ast.items {
        match item {
            Item::Const(constant) => {
                for annotation in &constant.annotations {
                    add_annotation_semantic(document, annotation, tokens);
                }
                push_semantic_span(
                    &document.source,
                    constant.name_span,
                    SEM_VARIABLE,
                    MOD_DECLARATION | MOD_SCHEMA,
                    tokens,
                );
                if let Some(ty) = &constant.ty {
                    add_type_ref_semantic(build, document, ty, tokens);
                }
                add_const_literal_semantic(document, &constant.value, tokens);
            }
            Item::Enum(enum_def) => {
                for annotation in &enum_def.annotations {
                    add_annotation_semantic(document, annotation, tokens);
                }
                for annotation in &enum_def.dangling_annotations {
                    add_annotation_semantic(document, annotation, tokens);
                }
                push_semantic_span(
                    &document.source,
                    enum_def.name_span,
                    SEM_ENUM,
                    MOD_DECLARATION | MOD_SCHEMA,
                    tokens,
                );
                for variant in &enum_def.variants {
                    for annotation in &variant.annotations {
                        add_annotation_semantic(document, annotation, tokens);
                    }
                    push_semantic_span(
                        &document.source,
                        variant.name_span,
                        SEM_ENUM_MEMBER,
                        MOD_DECLARATION | MOD_SCHEMA,
                        tokens,
                    );
                    if let Some(value) = &variant.value {
                        push_semantic_span_plain(&document.source, value.span, SEM_NUMBER, tokens);
                    }
                }
            }
            Item::Type(ty) => {
                for annotation in &ty.annotations {
                    add_annotation_semantic(document, annotation, tokens);
                }
                for annotation in &ty.dangling_annotations {
                    add_annotation_semantic(document, annotation, tokens);
                }
                push_semantic_span(
                    &document.source,
                    ty.name_span,
                    SEM_TYPE,
                    MOD_DECLARATION | MOD_SCHEMA,
                    tokens,
                );
                if let Some(parent) = &ty.parent {
                    push_semantic_span(
                        &document.source,
                        parent.span,
                        SEM_TYPE,
                        MOD_REFERENCE | MOD_SCHEMA,
                        tokens,
                    );
                }
                for field in &ty.fields {
                    for annotation in &field.annotations {
                        add_annotation_semantic(document, annotation, tokens);
                    }
                    push_semantic_span(
                        &document.source,
                        field.name_span,
                        SEM_PROPERTY,
                        MOD_DECLARATION | MOD_SCHEMA,
                        tokens,
                    );
                    add_type_ref_semantic(build, document, &field.ty, tokens);
                    if let Some(default) = &field.default {
                        add_default_expr_semantic(document, default, tokens);
                    }
                }
                if let Some(check) = &ty.check {
                    for stmt in &check.stmts {
                        add_check_stmt_semantic(build, document, stmt, tokens);
                    }
                }
            }
        }
    }
}

fn add_annotation_semantic(
    document: &LspDocument,
    annotation: &Annotation,
    tokens: &mut Vec<RawSemanticToken>,
) {
    push_semantic_span_plain(
        &document.source,
        annotation.name_span,
        SEM_DECORATOR,
        tokens,
    );
    for arg in &annotation.args {
        match arg {
            AnnotationArg::Name(name) => {
                push_semantic_span_plain(&document.source, name.span, SEM_VARIABLE, tokens);
            }
            AnnotationArg::String(_, span) => {
                push_semantic_span_plain(&document.source, *span, SEM_STRING, tokens);
            }
            AnnotationArg::Int(_, span) | AnnotationArg::Float(_, span) => {
                push_semantic_span_plain(&document.source, *span, SEM_NUMBER, tokens);
            }
            AnnotationArg::Bool(_, span) | AnnotationArg::Null(span) => {
                push_semantic_span_plain(&document.source, *span, SEM_KEYWORD, tokens);
            }
        }
    }
}

fn add_type_ref_semantic(
    build: &LspBuild,
    document: &LspDocument,
    ty: &TypeRef,
    tokens: &mut Vec<RawSemanticToken>,
) {
    match &ty.kind {
        TypeRefKind::Int | TypeRefKind::Float | TypeRefKind::Bool | TypeRefKind::String => {
            push_semantic_span(
                &document.source,
                ty.span,
                SEM_TYPE,
                MOD_REFERENCE | MOD_SCHEMA,
                tokens,
            );
        }
        TypeRefKind::Named(name) => {
            let token_type = if enum_name_exists(build, name) {
                SEM_ENUM
            } else {
                SEM_TYPE
            };
            push_semantic_span(
                &document.source,
                ty.span,
                token_type,
                MOD_REFERENCE | MOD_SCHEMA,
                tokens,
            );
        }
        TypeRefKind::Array(inner) | TypeRefKind::Nullable(inner) => {
            add_type_ref_semantic(build, document, inner, tokens);
        }
        TypeRefKind::Ref(inner) => add_type_ref_semantic(build, document, inner, tokens),
        TypeRefKind::Dict(key, value) => {
            add_type_ref_semantic(build, document, key, tokens);
            add_type_ref_semantic(build, document, value, tokens);
        }
    }
}

fn add_const_literal_semantic(
    document: &LspDocument,
    literal: &ConstLiteral,
    tokens: &mut Vec<RawSemanticToken>,
) {
    match literal {
        ConstLiteral::Int(_, span) | ConstLiteral::Float(_, span) => {
            push_semantic_span_plain(&document.source, *span, SEM_NUMBER, tokens);
        }
        ConstLiteral::Bool(_, span) => {
            push_semantic_span_plain(&document.source, *span, SEM_KEYWORD, tokens);
        }
        ConstLiteral::String(_, span) => {
            push_semantic_span_plain(&document.source, *span, SEM_STRING, tokens);
        }
    }
}

fn add_default_expr_semantic(
    document: &LspDocument,
    expr: &DefaultExpr,
    tokens: &mut Vec<RawSemanticToken>,
) {
    match &expr.kind {
        DefaultExprKind::Int(_) | DefaultExprKind::Float(_) => {
            push_semantic_span_plain(&document.source, expr.span, SEM_NUMBER, tokens);
        }
        DefaultExprKind::Bool(_) | DefaultExprKind::Null => {
            push_semantic_span_plain(&document.source, expr.span, SEM_KEYWORD, tokens);
        }
        DefaultExprKind::String(_) => {
            push_semantic_span_plain(&document.source, expr.span, SEM_STRING, tokens);
        }
        DefaultExprKind::Name(name) => {
            push_semantic_span(
                &document.source,
                name.span,
                SEM_VARIABLE,
                MOD_REFERENCE | MOD_SCHEMA,
                tokens,
            );
        }
        DefaultExprKind::EnumVariant { enum_name, variant } => {
            push_semantic_span(
                &document.source,
                enum_name.span,
                SEM_ENUM,
                MOD_REFERENCE | MOD_SCHEMA,
                tokens,
            );
            push_semantic_span(
                &document.source,
                variant.span,
                SEM_ENUM_MEMBER,
                MOD_REFERENCE | MOD_SCHEMA,
                tokens,
            );
        }
        DefaultExprKind::Array(items) => {
            for item in items {
                add_default_expr_semantic(document, item, tokens);
            }
        }
        DefaultExprKind::Object(fields) => {
            for (name, value) in fields {
                push_semantic_span(
                    &document.source,
                    name.span,
                    SEM_PROPERTY,
                    MOD_DECLARATION | MOD_SCHEMA,
                    tokens,
                );
                add_default_expr_semantic(document, value, tokens);
            }
        }
    }
}

fn add_check_stmt_semantic(
    build: &LspBuild,
    document: &LspDocument,
    stmt: &CheckStmt,
    tokens: &mut Vec<RawSemanticToken>,
) {
    match stmt {
        CheckStmt::Expr(expr) => add_check_expr_semantic(build, document, expr, tokens),
        CheckStmt::Quantifier {
            binding,
            collection,
            body,
            ..
        } => {
            push_semantic_span(
                &document.source,
                binding.span,
                SEM_PARAMETER,
                MOD_DECLARATION,
                tokens,
            );
            add_check_expr_semantic(build, document, collection, tokens);
            for stmt in body {
                add_check_stmt_semantic(build, document, stmt, tokens);
            }
        }
        CheckStmt::When {
            condition, body, ..
        } => {
            add_check_expr_semantic(build, document, condition, tokens);
            for stmt in body {
                add_check_stmt_semantic(build, document, stmt, tokens);
            }
        }
    }
}

#[allow(clippy::too_many_lines)]
fn add_check_expr_semantic(
    build: &LspBuild,
    document: &LspDocument,
    expr: &CheckExpr,
    tokens: &mut Vec<RawSemanticToken>,
) {
    match &expr.kind {
        CheckExprKind::Int(_) | CheckExprKind::Float(_) => {
            push_semantic_span_plain(&document.source, expr.span, SEM_NUMBER, tokens);
        }
        CheckExprKind::Bool(_) | CheckExprKind::Null => {
            push_semantic_span_plain(&document.source, expr.span, SEM_KEYWORD, tokens);
        }
        CheckExprKind::String(_) => {
            push_semantic_span_plain(&document.source, expr.span, SEM_STRING, tokens);
        }
        CheckExprKind::Name(_) => {
            push_semantic_span(
                &document.source,
                expr.span,
                SEM_VARIABLE,
                MOD_REFERENCE,
                tokens,
            );
        }
        CheckExprKind::Field { expr, name } => {
            if let CheckExprKind::Name(enum_name) = &expr.kind {
                if enum_variant_exists(build, enum_name, &name.name) {
                    push_semantic_span(
                        &document.source,
                        expr.span,
                        SEM_ENUM,
                        MOD_REFERENCE | MOD_SCHEMA,
                        tokens,
                    );
                    push_semantic_span(
                        &document.source,
                        name.span,
                        SEM_ENUM_MEMBER,
                        MOD_REFERENCE | MOD_SCHEMA,
                        tokens,
                    );
                    return;
                }
            }
            add_check_expr_semantic(build, document, expr, tokens);
            push_semantic_span(
                &document.source,
                name.span,
                SEM_PROPERTY,
                MOD_REFERENCE | MOD_PATH | MOD_SCHEMA,
                tokens,
            );
        }
        CheckExprKind::Index { expr, index } => {
            add_check_expr_semantic(build, document, expr, tokens);
            add_check_expr_semantic(build, document, index, tokens);
        }
        CheckExprKind::Is { expr, predicate } => {
            add_check_expr_semantic(build, document, expr, tokens);
            match predicate {
                coflow_cft::ast::TypePredicate::Type(name) => {
                    push_semantic_span(
                        &document.source,
                        name.span,
                        SEM_TYPE,
                        MOD_REFERENCE | MOD_SCHEMA,
                        tokens,
                    );
                }
                coflow_cft::ast::TypePredicate::Null(span) => {
                    push_semantic_span_plain(&document.source, *span, SEM_KEYWORD, tokens);
                }
            }
        }
        CheckExprKind::Call { name, args } => {
            let token_type = if enum_name_exists(build, &name.name) {
                SEM_ENUM
            } else {
                SEM_FUNCTION
            };
            let modifiers = if token_type == SEM_ENUM {
                MOD_REFERENCE | MOD_SCHEMA
            } else {
                MOD_REFERENCE
            };
            push_semantic_span(&document.source, name.span, token_type, modifiers, tokens);
            for arg in args {
                add_check_expr_semantic(build, document, arg, tokens);
            }
        }
        CheckExprKind::MethodCall {
            receiver,
            name,
            args,
        } => {
            add_check_expr_semantic(build, document, receiver, tokens);
            push_semantic_span(
                &document.source,
                name.span,
                SEM_FUNCTION,
                MOD_REFERENCE,
                tokens,
            );
            for arg in args {
                add_check_expr_semantic(build, document, arg, tokens);
            }
        }
        CheckExprKind::BinOp { lhs, rhs, .. } => {
            add_check_expr_semantic(build, document, lhs, tokens);
            add_check_expr_semantic(build, document, rhs, tokens);
        }
        CheckExprKind::Unary { expr, .. } => {
            add_check_expr_semantic(build, document, expr, tokens);
        }
        CheckExprKind::CmpChain { first, rest } => {
            add_check_expr_semantic(build, document, first, tokens);
            for (_, expr) in rest {
                add_check_expr_semantic(build, document, expr, tokens);
            }
        }
    }
}

pub(crate) fn current_type_at<'a>(
    build: &'a LspBuild,
    document: &LspDocument,
    offset: usize,
) -> Option<&'a CftSchemaType> {
    build.container()?.all_types().find(|ty| {
        ty.module.as_str() == document.module_id && ty.span.start <= offset && offset <= ty.span.end
    })
}

pub(crate) fn current_field_at(
    document: &LspDocument,
    offset: usize,
) -> Option<&coflow_cft::ast::FieldDef> {
    let ast = document.ast.as_ref()?;
    for item in &ast.items {
        if let Item::Type(ty) = item {
            if ty.span.start <= offset && offset <= ty.span.end {
                for field in &ty.fields {
                    if field.span.start <= offset && offset <= field.span.end {
                        return Some(field);
                    }
                }
            }
        }
    }
    None
}

pub(crate) fn type_of_chain(
    build: &LspBuild,
    document: &LspDocument,
    offset: usize,
    chain: &[String],
) -> Option<CftSchemaTypeRef> {
    let (first, rest) = chain.split_first()?;
    let mut ty_ref = type_of_name(build, document, offset, first)?;
    for part in rest {
        let type_name = type_name_of_schema_ref(&ty_ref)?;
        let (_, field) = field_by_type(build, type_name, part)?;
        ty_ref = field_receiver_type(field);
    }
    Some(ty_ref)
}

fn type_of_name(
    build: &LspBuild,
    document: &LspDocument,
    offset: usize,
    name: &str,
) -> Option<CftSchemaTypeRef> {
    let current_type = current_type_at(build, document, offset)?;
    let field = current_type
        .all_fields
        .iter()
        .find(|field| field.name == name)?;
    Some(field_receiver_type(field))
}

fn field_by_type<'a>(
    build: &'a LspBuild,
    type_name: &str,
    field_name: &str,
) -> Option<(&'a CftSchemaType, &'a CftSchemaField)> {
    let container = build.container()?;
    let mut current = container.resolve_type(type_name);
    while let Some(ty) = current {
        if let Some(field) = ty.fields.iter().find(|field| field.name == field_name) {
            return Some((ty, field));
        }
        current = ty
            .parent
            .as_deref()
            .and_then(|parent| container.resolve_type(parent));
    }
    None
}

fn field_receiver_type(field: &CftSchemaField) -> CftSchemaTypeRef {
    field.ty_ref.clone()
}

pub(crate) fn type_name_of_schema_ref(ty: &CftSchemaTypeRef) -> Option<&str> {
    match ty {
        CftSchemaTypeRef::Named(name) => Some(name),
        CftSchemaTypeRef::Nullable(inner) => type_name_of_schema_ref(inner),
        _ => None,
    }
}

fn field_by_chain<'a>(
    build: &'a LspBuild,
    document: &LspDocument,
    offset: usize,
    chain: &[String],
) -> Option<(String, &'a CftSchemaField)> {
    let (field_name, receiver) = chain.split_last()?;
    let receiver_type = type_of_chain(build, document, offset, receiver)?;
    let type_name = type_name_of_schema_ref(&receiver_type)?;
    let (_, field) = field_by_type(build, type_name, field_name)?;
    Some((type_name.to_string(), field))
}

fn field_location_by_chain(
    build: &LspBuild,
    document: &LspDocument,
    offset: usize,
    chain: &[String],
) -> Option<Value> {
    let (field_name, receiver) = chain.split_last()?;
    let receiver_type = type_of_chain(build, document, offset, receiver)?;
    let type_name = type_name_of_schema_ref(&receiver_type)?;
    field_location(build, type_name, field_name)
}

fn field_location(build: &LspBuild, type_name: &str, field_name: &str) -> Option<Value> {
    let (owner, field) = field_by_type(build, type_name, field_name)?;
    let document = build.document_by_module(&owner.module)?;
    let span = ast_field_name_span(document, &owner.name, field_name).unwrap_or(field.span);
    Some(location(document, span))
}

fn ast_field_name_span(document: &LspDocument, type_name: &str, field_name: &str) -> Option<Span> {
    let ast = document.ast.as_ref()?;
    for item in &ast.items {
        if let Item::Type(ty) = item {
            if ty.name == type_name {
                return ty
                    .fields
                    .iter()
                    .find(|field| field.name == field_name)
                    .map(|field| field.name_span);
            }
        }
    }
    None
}

fn enum_variant_by_chain<'a>(
    build: &'a LspBuild,
    chain: &[String],
) -> Option<(&'a CftSchemaEnum, &'a CftSchemaEnumVariant)> {
    if chain.len() != 2 {
        return None;
    }
    let enum_def = build.container()?.resolve_enum(&chain[0])?;
    let variant = enum_def
        .variants
        .iter()
        .find(|variant| variant.name == chain[1])?;
    Some((enum_def, variant))
}

fn enum_variant_location_by_chain(build: &LspBuild, chain: &[String]) -> Option<Value> {
    let (enum_def, variant) = enum_variant_by_chain(build, chain)?;
    let document = build.document_by_module(&enum_def.module)?;
    let span =
        ast_enum_variant_name_span(document, &enum_def.name, &variant.name).unwrap_or(variant.span);
    Some(location(document, span))
}

fn ast_enum_variant_location_by_chain(build: &LspBuild, chain: &[String]) -> Option<Value> {
    if chain.len() != 2 {
        return None;
    }
    ast_enum_variant_location(build, &chain[0], &chain[1])
}

fn ast_enum_variant_location(
    build: &LspBuild,
    enum_name: &str,
    variant_name: &str,
) -> Option<Value> {
    for document in build.documents.values() {
        let Some(span) = ast_enum_variant_name_span(document, enum_name, variant_name) else {
            continue;
        };
        return Some(location(document, span));
    }
    None
}

fn ast_enum_variant_name_span(
    document: &LspDocument,
    enum_name: &str,
    variant_name: &str,
) -> Option<Span> {
    let ast = document.ast.as_ref()?;
    for item in &ast.items {
        if let Item::Enum(enum_def) = item {
            if enum_def.name == enum_name {
                return enum_def
                    .variants
                    .iter()
                    .find(|variant| variant.name == variant_name)
                    .map(|variant| variant.name_span);
            }
        }
    }
    None
}

fn enum_name_exists(build: &LspBuild, enum_name: &str) -> bool {
    build
        .container()
        .is_some_and(|container| container.resolve_enum(enum_name).is_some())
        || ast_enum_name_exists(build, enum_name)
}

fn enum_variant_exists(build: &LspBuild, enum_name: &str, variant_name: &str) -> bool {
    enum_variant_by_chain(build, &[enum_name.to_string(), variant_name.to_string()]).is_some()
        || ast_enum_variant_location(build, enum_name, variant_name).is_some()
}

fn ast_enum_name_exists(build: &LspBuild, enum_name: &str) -> bool {
    build.documents.values().any(|document| {
        document.ast.as_ref().is_some_and(|ast| {
            ast.items
                .iter()
                .any(|item| matches!(item, Item::Enum(enum_def) if enum_def.name == enum_name))
        })
    })
}

fn global_location(build: &LspBuild, name: &str) -> Option<Value> {
    let container = build.container()?;
    if let Some(ty) = container.resolve_type(name) {
        let document = build.document_by_module(&ty.module)?;
        return Some(location(
            document,
            ast_top_level_name_span(document, name).unwrap_or(ty.span),
        ));
    }
    if let Some(enum_def) = container.resolve_enum(name) {
        let document = build.document_by_module(&enum_def.module)?;
        return Some(location(
            document,
            ast_top_level_name_span(document, name).unwrap_or(enum_def.span),
        ));
    }
    if let Some(constant) = container.resolve_const(name) {
        let document = build.document_by_module(&constant.module)?;
        return Some(location(
            document,
            ast_top_level_name_span(document, name).unwrap_or(constant.span),
        ));
    }
    None
}

fn ast_global_location(build: &LspBuild, name: &str) -> Option<Value> {
    for document in build.documents.values() {
        let Some(ast) = &document.ast else {
            continue;
        };
        for item in &ast.items {
            match item {
                Item::Const(constant) if constant.name == name => {
                    return Some(location(document, constant.name_span));
                }
                Item::Enum(enum_def) if enum_def.name == name => {
                    return Some(location(document, enum_def.name_span));
                }
                Item::Type(ty) if ty.name == name => {
                    return Some(location(document, ty.name_span));
                }
                Item::Const(_) | Item::Enum(_) | Item::Type(_) => {}
            }
        }
    }
    None
}

fn ast_top_level_name_span(document: &LspDocument, name: &str) -> Option<Span> {
    let ast = document.ast.as_ref()?;
    ast.items.iter().find_map(|item| match item {
        Item::Const(constant) if constant.name == name => Some(constant.name_span),
        Item::Enum(enum_def) if enum_def.name == name => Some(enum_def.name_span),
        Item::Type(ty) if ty.name == name => Some(ty.name_span),
        _ => None,
    })
}

fn location(document: &LspDocument, span: Span) -> Value {
    json!({
        "uri": document.uri,
        "range": range_from_span(&document.source, span)
    })
}

fn type_hover_text(ty: &CftSchemaType) -> String {
    let mut flags = Vec::new();
    if ty.is_abstract {
        flags.push("abstract");
    }
    if ty.is_sealed {
        flags.push("sealed");
    }
    let mut text = if flags.is_empty() {
        format!("CFT type `{}`", ty.name)
    } else {
        format!("CFT {} type `{}`", flags.join(" "), ty.name)
    };
    if let Some(parent) = &ty.parent {
        let _ = write!(text, " extends `{parent}`");
    }
    let _ = write!(text, " with {} field(s).", ty.all_fields.len());
    text
}

fn const_value_to_string(value: &CftConstValue) -> String {
    match value {
        CftConstValue::Int(value) => value.to_string(),
        CftConstValue::Float(value) => value.to_string(),
        CftConstValue::Bool(value) => value.to_string(),
        CftConstValue::String(value) => format!("{value:?}"),
    }
}

fn hover_response(contents: &str, range: &Value) -> Value {
    json!({
        "contents": {
            "kind": "markdown",
            "value": contents
        },
        "range": range
    })
}

fn annotation_at(document: &LspDocument, offset: usize) -> Option<&Annotation> {
    fn find_in(annotations: &[Annotation], offset: usize) -> Option<&Annotation> {
        annotations.iter().find(|annotation| {
            annotation.name_span.start <= offset && offset <= annotation.name_span.end
        })
    }

    let ast = document.ast.as_ref()?;
    if let Some(annotation) = find_in(&ast.dangling_annotations, offset) {
        return Some(annotation);
    }
    for item in &ast.items {
        match item {
            Item::Const(constant) => {
                if let Some(annotation) = find_in(&constant.annotations, offset) {
                    return Some(annotation);
                }
            }
            Item::Enum(enum_def) => {
                if let Some(annotation) = find_in(&enum_def.annotations, offset)
                    .or_else(|| find_in(&enum_def.dangling_annotations, offset))
                {
                    return Some(annotation);
                }
                for variant in &enum_def.variants {
                    if let Some(annotation) = find_in(&variant.annotations, offset) {
                        return Some(annotation);
                    }
                }
            }
            Item::Type(ty) => {
                if let Some(annotation) = find_in(&ty.annotations, offset)
                    .or_else(|| find_in(&ty.dangling_annotations, offset))
                {
                    return Some(annotation);
                }
                for field in &ty.fields {
                    if let Some(annotation) = find_in(&field.annotations, offset) {
                        return Some(annotation);
                    }
                }
            }
        }
    }
    None
}

pub(crate) fn quantifier_bindings_at(document: &LspDocument, offset: usize) -> Vec<String> {
    let mut bindings = Vec::new();
    let Some(ast) = &document.ast else {
        return bindings;
    };
    for item in &ast.items {
        if let Item::Type(ty) = item {
            if let Some(check) = &ty.check {
                collect_quantifier_bindings(&check.stmts, offset, &mut bindings);
            }
        }
    }
    bindings
}

fn collect_quantifier_bindings(stmts: &[CheckStmt], offset: usize, bindings: &mut Vec<String>) {
    for stmt in stmts {
        match stmt {
            CheckStmt::Quantifier {
                binding,
                body,
                span,
                ..
            } => {
                if span.start <= offset && offset <= span.end {
                    bindings.push(binding.name.clone());
                    collect_quantifier_bindings(body, offset, bindings);
                }
            }
            CheckStmt::When { body, span, .. } => {
                if span.start <= offset && offset <= span.end {
                    collect_quantifier_bindings(body, offset, bindings);
                }
            }
            CheckStmt::Expr(_) => {}
        }
    }
}

pub(crate) fn is_annotation_completion_context(line_prefix: &str) -> bool {
    let Some(index) = line_prefix.rfind('@') else {
        return false;
    };
    line_prefix[index + 1..].chars().all(is_ident_continue)
}

pub(crate) fn is_type_predicate_context(line_prefix: &str) -> bool {
    let trimmed = line_prefix.trim_end();
    let Some(last_word) = last_ident(trimmed) else {
        return false;
    };
    if last_word == "is" {
        return true;
    }
    trimmed[..trimmed.len() - last_word.len()]
        .trim_end()
        .ends_with("is")
}

pub(crate) fn is_type_header_parent_context(line_prefix: &str) -> bool {
    let Some(colon) = line_prefix.rfind(':') else {
        return false;
    };
    let before_colon = &line_prefix[..colon];
    before_colon.contains("type")
}

pub(crate) fn is_type_reference_context(line_prefix: &str) -> bool {
    let trimmed = line_prefix.trim_end();
    let Some(colon) = trimmed.rfind(':') else {
        return false;
    };
    let after_colon = &trimmed[colon + 1..];
    !after_colon.contains(';') && !after_colon.contains('=')
}

pub(crate) fn is_const_value_context(line_prefix: &str) -> bool {
    let trimmed = line_prefix.trim_end();
    trimmed.contains("const ") && trimmed.contains('=') && !trimmed.contains(';')
}

pub(crate) fn is_field_default_context(line_prefix: &str) -> bool {
    let trimmed = line_prefix.trim_end();
    let Some(equal) = trimmed.rfind('=') else {
        return false;
    };
    let Some(colon) = trimmed.rfind(':') else {
        return false;
    };
    colon < equal && !trimmed[equal + 1..].contains(';')
}

pub(crate) fn top_level_needs_type_keyword(line_prefix: &str) -> bool {
    matches!(last_ident(line_prefix), Some("abstract" | "sealed"))
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

pub(crate) fn receiver_chain_before_dot(line_prefix: &str) -> Option<Vec<String>> {
    let dot = line_prefix.rfind('.')?;
    let typed = line_prefix[dot + 1..].trim_start();
    if !typed.chars().all(is_ident_continue) {
        return None;
    }
    let receiver = trailing_dotted_ident_chain(&line_prefix[..dot])?;
    parse_dotted_ident_chain(receiver)
}

fn parse_dotted_ident_chain(text: &str) -> Option<Vec<String>> {
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

fn trailing_dotted_ident_chain(text: &str) -> Option<&str> {
    let trimmed_end = text.trim_end().len();
    let bytes = text.as_bytes();
    let mut start = trimmed_end;
    let mut saw_ident = false;
    let mut allow_dot = false;

    while start > 0 {
        let (previous, ch) = previous_char(text, start)?;
        if is_ident_continue(ch) {
            saw_ident = true;
            allow_dot = true;
            start = previous;
            continue;
        }
        if ch == '.' && allow_dot {
            saw_ident = false;
            allow_dot = false;
            start = previous;
            continue;
        }
        if ch.is_whitespace() && !saw_ident && previous + ch.len_utf8() == start {
            start = previous;
            continue;
        }
        break;
    }

    while start < trimmed_end && bytes[start].is_ascii_whitespace() {
        start += 1;
    }
    (saw_ident && start < trimmed_end).then_some(&text[start..trimmed_end])
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

fn dotted_chain_at(source: &str, word: &WordAt) -> Option<Vec<String>> {
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

fn word_at(source: &str, offset: usize) -> Option<WordAt> {
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

fn previous_char(source: &str, offset: usize) -> Option<(usize, char)> {
    source[..offset].char_indices().next_back()
}

fn is_ident_continue(ch: char) -> bool {
    ch == '_' || ch.is_alphanumeric()
}

fn last_ident(text: &str) -> Option<&str> {
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
