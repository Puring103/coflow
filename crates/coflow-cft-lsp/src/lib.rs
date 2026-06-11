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

use coflow_cft::ast::{
    Annotation, AnnotationArg, CheckExpr, CheckExprKind, CheckStmt, ConstLiteral, DefaultExpr,
    DefaultExprKind, Item, TypeRef, TypeRefKind,
};
use coflow_cft::lexer::{lex, TokenKind};
use coflow_cft::parser::parse_module;
use coflow_cft::{
    CftAnnotationValue, CftConstValue, CftSchemaEnum, CftSchemaEnumVariant, CftSchemaField,
    CftSchemaType, CftSchemaTypeRef, ModuleId, Span,
};
use coflow_project::{
    compile_schema_project_with_overrides, dedupe_cft_diagnostics, normalize_path, DiagnosticJson,
    Project, SchemaBuild, SchemaSourceOverride,
};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

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
                    Ok(())
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
            return if let Some(id) = id {
                self.write_error(
                    &id,
                    -32600,
                    "server is shut down; only the exit notification is accepted",
                )
            } else {
                Ok(())
            };
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
                    "name": "coflow-cft-lsp",
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
        if let Some(path) = path_from_file_uri(uri) {
            self.open_documents.remove(&normalize_path(&path));
        }
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

        for diagnostic in diagnostics {
            let diagnostic =
                DiagnosticJson::from_cft(&diagnostic, &build.schema.sources, &build.schema.paths);
            let uri = preferred_diagnostic_uri(&preferred_uris, Path::new(&diagnostic.path));
            by_uri
                .entry(uri)
                .or_default()
                .push(lsp_diagnostic(&diagnostic));
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
    uri: String,
    text: String,
}

const COMPLETION_KIND_FUNCTION: u8 = 3;
const COMPLETION_KIND_FIELD: u8 = 5;
const COMPLETION_KIND_VARIABLE: u8 = 6;
const COMPLETION_KIND_CLASS: u8 = 7;
const COMPLETION_KIND_PROPERTY: u8 = 10;
const COMPLETION_KIND_ENUM: u8 = 13;
const COMPLETION_KIND_KEYWORD: u8 = 14;
const COMPLETION_KIND_ENUM_MEMBER: u8 = 20;
const COMPLETION_KIND_CONSTANT: u8 = 21;

const SYMBOL_KIND_CLASS: u8 = 5;
const SYMBOL_KIND_FIELD: u8 = 8;
const SYMBOL_KIND_ENUM: u8 = 10;
const SYMBOL_KIND_CONSTANT: u8 = 14;
const SYMBOL_KIND_ENUM_MEMBER: u8 = 22;

const SEMANTIC_TOKEN_TYPES: &[&str] = &[
    "namespace",
    "type",
    "enum",
    "enumMember",
    "property",
    "variable",
    "function",
    "keyword",
    "number",
    "string",
    "comment",
    "operator",
    "decorator",
    "parameter",
];
const SEMANTIC_TOKEN_MODIFIERS: &[&str] = &[];

const SEM_TYPE: u32 = 1;
const SEM_ENUM: u32 = 2;
const SEM_ENUM_MEMBER: u32 = 3;
const SEM_PROPERTY: u32 = 4;
const SEM_VARIABLE: u32 = 5;
const SEM_FUNCTION: u32 = 6;
const SEM_KEYWORD: u32 = 7;
const SEM_NUMBER: u32 = 8;
const SEM_STRING: u32 = 9;
const SEM_COMMENT: u32 = 10;
const SEM_OPERATOR: u32 = 11;
const SEM_DECORATOR: u32 = 12;
const SEM_PARAMETER: u32 = 13;

const MAX_LSP_CONTENT_LENGTH: usize = 16 * 1024 * 1024;

const KEYWORDS: &[(&str, &str)] = &[
    ("const", "Define a compile-time constant."),
    ("enum", "Define an enum."),
    ("type", "Define a schema type."),
    ("abstract", "Mark a type as non-instantiable."),
    ("sealed", "Prevent a type from being inherited."),
    ("check", "Start a validation block inside a type."),
    ("when", "Run nested checks only when the condition is true."),
    ("all", "Require every collection item to pass."),
    ("any", "Require at least one collection item to pass."),
    ("none", "Require no collection item to pass."),
    ("in", "Bind a quantifier variable to a collection."),
    ("is", "Check the runtime type or null value."),
];

const PRIMITIVE_TYPES: &[(&str, &str)] = &[
    ("int", "64-bit integer."),
    ("float", "64-bit floating point number."),
    ("bool", "Boolean value."),
    ("string", "String value."),
];

const LITERALS: &[(&str, &str)] = &[
    ("true", "Boolean true."),
    ("false", "Boolean false."),
    ("null", "Nullable value."),
];

fn is_fatal_lsp_handler_error(message: &str) -> bool {
    message.starts_with("failed to write LSP")
        || message.starts_with("failed to flush LSP")
        || message.starts_with("failed to serialize LSP")
}

const BUILTIN_FUNCTIONS: &[(&str, &str)] = &[
    (
        "len",
        "len(col): return the number of items in an array or dict.",
    ),
    (
        "contains",
        "contains(col, val): test array element or dict key presence.",
    ),
    (
        "unique",
        "unique(array): true when supported scalar elements are unique.",
    ),
    (
        "min",
        "min(array): minimum value in a non-empty int, float, or enum array.",
    ),
    (
        "max",
        "max(array): maximum value in a non-empty int, float, or enum array.",
    ),
    ("sum", "sum(array): sum an int or float array."),
    ("keys", "keys(dict): return dict keys as an array."),
    ("values", "values(dict): return dict values as an array."),
    (
        "matches",
        "matches(str, pat): regex match with a string literal pattern.",
    ),
];

const ANNOTATIONS: &[AnnotationCompletion] = &[
    AnnotationCompletion {
        label: "@struct",
        insert_text: "@struct",
        detail: "type annotation",
        documentation: "Generate a value type. The target must be a sealed type.",
    },
    AnnotationCompletion {
        label: "@flag",
        insert_text: "@flag",
        detail: "enum annotation",
        documentation: "Mark an enum as bit flags. Non-zero values must be powers of two.",
    },
    AnnotationCompletion {
        label: "@id",
        insert_text: "@id",
        detail: "field annotation",
        documentation: "Mark a string or int field as the primary key.",
    },
    AnnotationCompletion {
        label: "@ref",
        insert_text: "@ref(${1:TypeName})",
        detail: "field annotation",
        documentation: "Mark a string or int field as a reference to a type.",
    },
    AnnotationCompletion {
        label: "@index",
        insert_text: "@index",
        detail: "field annotation",
        documentation: "Generate an index for a non-nullable string, int, or enum field.",
    },
    AnnotationCompletion {
        label: "@display",
        insert_text: "@display(\"${1:text}\")",
        detail: "type, enum, or field annotation",
        documentation: "Attach a human-readable display name.",
    },
    AnnotationCompletion {
        label: "@deprecated",
        insert_text: "@deprecated",
        detail: "type, enum, or field annotation",
        documentation: "Mark the target as deprecated for generated code.",
    },
];

struct AnnotationCompletion {
    label: &'static str,
    insert_text: &'static str,
    detail: &'static str,
    documentation: &'static str,
}

struct LspBuild {
    schema: SchemaBuild,
    documents: BTreeMap<String, LspDocument>,
    module_by_uri: BTreeMap<String, String>,
    module_by_path: BTreeMap<PathBuf, String>,
}

struct LspDocument {
    module_id: String,
    uri: String,
    source: String,
    ast: Option<coflow_cft::ast::ModuleAst>,
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

    const fn container(&self) -> Option<&coflow_cft::CftContainer> {
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

struct TextRequest {
    uri: String,
    position: LspPosition,
}

impl TextRequest {
    fn from_params(params: &Value) -> Option<Self> {
        Some(Self {
            uri: text_document_uri(params)?,
            position: LspPosition::from_value(params.get("position")?)?,
        })
    }
}

#[derive(Clone, Copy)]
struct LspPosition {
    line: usize,
    character: usize,
}

impl LspPosition {
    fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            line: value.get("line")?.as_u64()?.try_into().ok()?,
            character: value.get("character")?.as_u64()?.try_into().ok()?,
        })
    }
}

struct WordAt {
    text: String,
    start: usize,
    end: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CompletionScope {
    TopLevel,
    TypeBody,
    CheckBlock,
    EnumBody,
}

fn completion_items(
    build: &LspBuild,
    document: &LspDocument,
    position: &LspPosition,
) -> Vec<Value> {
    let offset = byte_offset_from_position(&document.source, *position);
    let line_prefix = line_prefix_at(&document.source, offset);
    let scope = completion_scope(document, offset);

    if is_trivia_position(&document.source, offset) {
        return Vec::new();
    }

    if is_type_predicate_context(line_prefix) {
        let mut items = named_type_completion_items(build);
        items.push(completion_item(
            "null",
            COMPLETION_KIND_KEYWORD,
            "Null predicate",
            Some("Nullable value."),
        ));
        return items;
    }

    if is_annotation_completion_context(line_prefix) {
        return annotation_completion_items(scope);
    }

    if let Some(chain) = receiver_chain_before_dot(line_prefix) {
        return dot_completion_items(build, document, offset, &chain);
    }

    if is_ref_annotation_context(line_prefix) {
        return named_type_completion_items(build);
    }

    if top_level_needs_type_keyword(line_prefix) {
        return top_level_completion_items(line_prefix);
    }

    if is_type_header_parent_context(line_prefix) {
        return named_type_completion_items(build);
    }

    match scope {
        CompletionScope::TopLevel => {
            if is_const_value_context(line_prefix) {
                return const_value_completion_items();
            }
            top_level_completion_items(line_prefix)
        }
        CompletionScope::TypeBody => {
            if is_field_default_context(line_prefix) {
                return field_default_completion_items(build, current_field_at(document, offset));
            }
            if is_type_reference_context(line_prefix) {
                return type_completion_items(build);
            }
            type_member_completion_items()
        }
        CompletionScope::CheckBlock => check_expression_completion_items(build, document, offset),
        CompletionScope::EnumBody => Vec::new(),
    }
}

fn top_level_completion_items(line_prefix: &str) -> Vec<Value> {
    let labels: &[&str] = if top_level_needs_type_keyword(line_prefix) {
        &["type"]
    } else {
        &["const", "enum", "type", "abstract", "sealed"]
    };
    keyword_completion_items(labels)
}

fn type_member_completion_items() -> Vec<Value> {
    keyword_completion_items(&["check"])
}

fn check_expression_completion_items(
    build: &LspBuild,
    document: &LspDocument,
    offset: usize,
) -> Vec<Value> {
    let mut items = Vec::new();
    items.extend(keyword_completion_items(&["when", "all", "any", "none"]));
    items.extend(literal_completion_items(true));
    items.extend(function_completion_items());
    items.extend(const_completion_items(build));

    if let Some(current_type) = current_type_at(build, document, offset) {
        for field in &current_type.all_fields {
            items.push(completion_item(
                &field.name,
                COMPLETION_KIND_FIELD,
                &format!("{} field", current_type.name),
                None,
            ));
        }
    }

    for binding in quantifier_bindings_at(document, offset) {
        items.push(completion_item(
            &binding,
            COMPLETION_KIND_VARIABLE,
            "CFT quantifier binding",
            None,
        ));
    }

    items
}

fn keyword_completion_items(labels: &[&str]) -> Vec<Value> {
    labels
        .iter()
        .filter_map(|requested| {
            KEYWORDS
                .iter()
                .find(|(label, _)| label == requested)
                .map(|(label, documentation)| {
                    completion_item(
                        label,
                        COMPLETION_KIND_KEYWORD,
                        "CFT keyword",
                        Some(documentation),
                    )
                })
        })
        .collect()
}

fn literal_completion_items(include_null: bool) -> Vec<Value> {
    LITERALS
        .iter()
        .filter(|(label, _)| include_null || *label != "null")
        .map(|(label, documentation)| {
            completion_item(
                label,
                COMPLETION_KIND_KEYWORD,
                "CFT literal",
                Some(documentation),
            )
        })
        .collect()
}

fn function_completion_items() -> Vec<Value> {
    BUILTIN_FUNCTIONS
        .iter()
        .map(|(label, documentation)| {
            let mut item = completion_item(
                label,
                COMPLETION_KIND_FUNCTION,
                "CFT built-in function",
                Some(documentation),
            );
            insert_object_field(&mut item, "insertText", json!(format!("{label}($1)")));
            insert_object_field(&mut item, "insertTextFormat", json!(2));
            item
        })
        .collect()
}

fn const_value_completion_items() -> Vec<Value> {
    literal_completion_items(false)
}

fn field_default_completion_items(
    build: &LspBuild,
    field: Option<&coflow_cft::ast::FieldDef>,
) -> Vec<Value> {
    let mut items = Vec::new();
    let Some(field) = field else {
        items.extend(literal_completion_items(true));
        items.extend(const_completion_items(build));
        return items;
    };

    collect_default_items_for_type(build, &field.ty, &mut items);
    items.extend(const_completion_items_for_type(build, &field.ty));
    items
}

fn collect_default_items_for_type(build: &LspBuild, ty: &TypeRef, items: &mut Vec<Value>) {
    match &ty.kind {
        TypeRefKind::Bool => items.extend(literal_completion_items(false)),
        TypeRefKind::Int | TypeRefKind::Float | TypeRefKind::String => {}
        TypeRefKind::Named(name) => {
            if let Some(enum_def) = build
                .container()
                .and_then(|container| container.resolve_enum(name))
            {
                items.extend(enum_def.variants.iter().map(|variant| {
                    let label = format!("{}.{}", enum_def.name, variant.name);
                    completion_item(
                        &label,
                        COMPLETION_KIND_ENUM_MEMBER,
                        "CFT enum variant",
                        None,
                    )
                }));
            }
        }
        TypeRefKind::Array(_) => {
            items.push(completion_item(
                "[]",
                COMPLETION_KIND_CONSTANT,
                "Empty array default",
                None,
            ));
        }
        TypeRefKind::Dict(_, _) => {
            items.push(completion_item(
                "{}",
                COMPLETION_KIND_CONSTANT,
                "Empty object default",
                None,
            ));
        }
        TypeRefKind::Nullable(inner) => {
            items.push(completion_item(
                "null",
                COMPLETION_KIND_KEYWORD,
                "CFT literal",
                Some("Nullable value."),
            ));
            collect_default_items_for_type(build, inner, items);
        }
    }
}

fn dot_completion_items(
    build: &LspBuild,
    document: &LspDocument,
    offset: usize,
    chain: &[String],
) -> Vec<Value> {
    if chain.len() == 1 {
        if let Some(enum_def) = build.container().and_then(|container| {
            container
                .resolve_enum(&chain[0])
                .or_else(|| container.resolve_enum(chain[0].as_str()))
        }) {
            return enum_def
                .variants
                .iter()
                .map(|variant| {
                    completion_item(
                        &variant.name,
                        COMPLETION_KIND_ENUM_MEMBER,
                        &format!("{} variant", enum_def.name),
                        None,
                    )
                })
                .collect();
        }
    }

    let Some(receiver_type) = type_of_chain(build, document, offset, chain) else {
        return Vec::new();
    };
    let Some(type_name) = type_name_of_schema_ref(&receiver_type) else {
        return Vec::new();
    };
    let Some(ty) = build
        .container()
        .and_then(|container| container.resolve_type(type_name))
    else {
        return Vec::new();
    };

    ty.all_fields
        .iter()
        .map(|field| {
            completion_item(
                &field.name,
                COMPLETION_KIND_FIELD,
                &format!("{type_name} field"),
                None,
            )
        })
        .collect()
}

fn type_completion_items(build: &LspBuild) -> Vec<Value> {
    let mut items = Vec::new();
    for (label, documentation) in PRIMITIVE_TYPES {
        items.push(completion_item(
            label,
            COMPLETION_KIND_KEYWORD,
            "Primitive type",
            Some(documentation),
        ));
    }
    if let Some(container) = build.container() {
        for ty in container.all_types() {
            items.push(completion_item(
                &ty.name,
                COMPLETION_KIND_CLASS,
                "CFT type",
                None,
            ));
        }
        for enum_def in container.all_enums() {
            items.push(completion_item(
                &enum_def.name,
                COMPLETION_KIND_ENUM,
                "CFT enum",
                None,
            ));
        }
    } else {
        for document in build.documents.values() {
            if let Some(ast) = &document.ast {
                for item in &ast.items {
                    match item {
                        Item::Type(ty) => items.push(completion_item(
                            &ty.name,
                            COMPLETION_KIND_CLASS,
                            "CFT type",
                            None,
                        )),
                        Item::Enum(enum_def) => items.push(completion_item(
                            &enum_def.name,
                            COMPLETION_KIND_ENUM,
                            "CFT enum",
                            None,
                        )),
                        Item::Const(_) => {}
                    }
                }
            }
        }
    }
    items
}

fn named_type_completion_items(build: &LspBuild) -> Vec<Value> {
    let mut items = Vec::new();
    if let Some(container) = build.container() {
        for ty in container.all_types() {
            items.push(completion_item(
                &ty.name,
                COMPLETION_KIND_CLASS,
                "CFT type",
                None,
            ));
        }
    }
    items
}

fn const_completion_items(build: &LspBuild) -> Vec<Value> {
    let mut items = Vec::new();
    if let Some(container) = build.container() {
        for constant in container
            .module_ids()
            .filter_map(|module_id| container.schema(module_id))
            .flat_map(|module| &module.consts)
        {
            items.push(completion_item(
                &constant.name,
                COMPLETION_KIND_CONSTANT,
                "CFT constant",
                None,
            ));
        }
    }
    items
}

fn const_completion_items_for_type(build: &LspBuild, ty: &TypeRef) -> Vec<Value> {
    let mut items = Vec::new();
    let Some(container) = build.container() else {
        return items;
    };
    for constant in container
        .module_ids()
        .filter_map(|module_id| container.schema(module_id))
        .flat_map(|module| &module.consts)
        .filter(|constant| const_value_assignable_to_type(&constant.value, ty))
    {
        items.push(completion_item(
            &constant.name,
            COMPLETION_KIND_CONSTANT,
            "CFT constant",
            None,
        ));
    }
    items
}

fn const_value_assignable_to_type(value: &CftConstValue, ty: &TypeRef) -> bool {
    match (&ty.kind, value) {
        (TypeRefKind::Int, CftConstValue::Int(_))
        | (TypeRefKind::Float, CftConstValue::Float(_))
        | (TypeRefKind::Bool, CftConstValue::Bool(_))
        | (TypeRefKind::String, CftConstValue::String(_)) => true,
        (TypeRefKind::Nullable(inner), value) => const_value_assignable_to_type(value, inner),
        _ => false,
    }
}

fn completion_item(label: &str, kind: u8, detail: &str, documentation: Option<&str>) -> Value {
    let mut item = Map::new();
    item.insert("label".to_string(), json!(label));
    item.insert("kind".to_string(), json!(kind));
    item.insert("detail".to_string(), json!(detail));
    if let Some(documentation) = documentation {
        item.insert("documentation".to_string(), json!(documentation));
    }
    Value::Object(item)
}

fn annotation_completion_item(annotation: &AnnotationCompletion) -> Value {
    let mut item = completion_item(
        annotation.label,
        COMPLETION_KIND_PROPERTY,
        annotation.detail,
        Some(annotation.documentation),
    );
    insert_object_field(&mut item, "insertText", json!(annotation.insert_text));
    insert_object_field(
        &mut item,
        "sortText",
        json!(format!("0_{}", annotation.label)),
    );
    if annotation.insert_text.contains('$') {
        insert_object_field(&mut item, "insertTextFormat", json!(2));
    }
    item
}

fn insert_object_field(object: &mut Value, key: &str, value: Value) {
    if let Value::Object(fields) = object {
        fields.insert(key.to_string(), value);
    }
}

fn annotation_completion_items(scope: CompletionScope) -> Vec<Value> {
    ANNOTATIONS
        .iter()
        .filter(|annotation| annotation_applies_to_scope(annotation.label, scope))
        .map(annotation_completion_item)
        .collect()
}

fn annotation_applies_to_scope(label: &str, scope: CompletionScope) -> bool {
    match label {
        "@struct" | "@flag" => scope == CompletionScope::TopLevel,
        "@id" | "@ref" | "@index" => scope == CompletionScope::TypeBody,
        "@display" | "@deprecated" => {
            matches!(scope, CompletionScope::TopLevel | CompletionScope::TypeBody)
        }
        _ => true,
    }
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

fn document_symbols(document: &LspDocument) -> Vec<Value> {
    let Some(ast) = &document.ast else {
        return Vec::new();
    };
    let mut symbols = Vec::new();
    for item in &ast.items {
        match item {
            Item::Const(constant) => symbols.push(document_symbol(
                &document.source,
                &constant.name,
                SYMBOL_KIND_CONSTANT,
                constant.span,
                constant.name_span,
                &[],
            )),
            Item::Enum(enum_def) => {
                let children = enum_def
                    .variants
                    .iter()
                    .map(|variant| {
                        document_symbol(
                            &document.source,
                            &variant.name,
                            SYMBOL_KIND_ENUM_MEMBER,
                            variant.span,
                            variant.name_span,
                            &[],
                        )
                    })
                    .collect::<Vec<_>>();
                symbols.push(document_symbol(
                    &document.source,
                    &enum_def.name,
                    SYMBOL_KIND_ENUM,
                    enum_def.span,
                    enum_def.name_span,
                    &children,
                ));
            }
            Item::Type(ty) => {
                let children = ty
                    .fields
                    .iter()
                    .map(|field| {
                        document_symbol(
                            &document.source,
                            &field.name,
                            SYMBOL_KIND_FIELD,
                            field.span,
                            field.name_span,
                            &[],
                        )
                    })
                    .collect::<Vec<_>>();
                symbols.push(document_symbol(
                    &document.source,
                    &ty.name,
                    SYMBOL_KIND_CLASS,
                    ty.span,
                    ty.name_span,
                    &children,
                ));
            }
        }
    }
    symbols
}

fn document_symbol(
    source: &str,
    name: &str,
    kind: u8,
    span: Span,
    name_span: Span,
    children: &[Value],
) -> Value {
    json!({
        "name": name,
        "kind": kind,
        "range": range_from_span(source, span),
        "selectionRange": range_from_span(source, name_span),
        "children": children
    })
}

fn semantic_token_data(build: &LspBuild, document: &LspDocument) -> Vec<u32> {
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
    encode_semantic_tokens(tokens)
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
    push_semantic_span(source, span, token_type, tokens);
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
                push_semantic_span(&document.source, constant.name_span, SEM_VARIABLE, tokens);
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
                push_semantic_span(&document.source, enum_def.name_span, SEM_ENUM, tokens);
                for variant in &enum_def.variants {
                    for annotation in &variant.annotations {
                        add_annotation_semantic(document, annotation, tokens);
                    }
                    push_semantic_span(
                        &document.source,
                        variant.name_span,
                        SEM_ENUM_MEMBER,
                        tokens,
                    );
                    if let Some(value) = &variant.value {
                        push_semantic_span(&document.source, value.span, SEM_NUMBER, tokens);
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
                push_semantic_span(&document.source, ty.name_span, SEM_TYPE, tokens);
                if let Some(parent) = &ty.parent {
                    push_semantic_span(&document.source, parent.span, SEM_TYPE, tokens);
                }
                for field in &ty.fields {
                    for annotation in &field.annotations {
                        add_annotation_semantic(document, annotation, tokens);
                    }
                    push_semantic_span(&document.source, field.name_span, SEM_PROPERTY, tokens);
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
    push_semantic_span(
        &document.source,
        annotation.name_span,
        SEM_DECORATOR,
        tokens,
    );
    for arg in &annotation.args {
        match arg {
            AnnotationArg::Name(name) => {
                let token_type = if matches!(annotation.name.as_str(), "ref") {
                    SEM_TYPE
                } else {
                    SEM_VARIABLE
                };
                push_semantic_span(&document.source, name.span, token_type, tokens);
            }
            AnnotationArg::String(_, span) => {
                push_semantic_span(&document.source, *span, SEM_STRING, tokens);
            }
            AnnotationArg::Int(_, span) | AnnotationArg::Float(_, span) => {
                push_semantic_span(&document.source, *span, SEM_NUMBER, tokens);
            }
            AnnotationArg::Bool(_, span) | AnnotationArg::Null(span) => {
                push_semantic_span(&document.source, *span, SEM_KEYWORD, tokens);
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
            push_semantic_span(&document.source, ty.span, SEM_TYPE, tokens);
        }
        TypeRefKind::Named(name) => {
            let token_type = if enum_name_exists(build, name) {
                SEM_ENUM
            } else {
                SEM_TYPE
            };
            push_semantic_span(&document.source, ty.span, token_type, tokens);
        }
        TypeRefKind::Array(inner) | TypeRefKind::Nullable(inner) => {
            add_type_ref_semantic(build, document, inner, tokens);
        }
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
            push_semantic_span(&document.source, *span, SEM_NUMBER, tokens);
        }
        ConstLiteral::Bool(_, span) => {
            push_semantic_span(&document.source, *span, SEM_KEYWORD, tokens);
        }
        ConstLiteral::String(_, span) => {
            push_semantic_span(&document.source, *span, SEM_STRING, tokens);
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
            push_semantic_span(&document.source, expr.span, SEM_NUMBER, tokens);
        }
        DefaultExprKind::Bool(_) | DefaultExprKind::Null => {
            push_semantic_span(&document.source, expr.span, SEM_KEYWORD, tokens);
        }
        DefaultExprKind::String(_) => {
            push_semantic_span(&document.source, expr.span, SEM_STRING, tokens);
        }
        DefaultExprKind::Name(name) => {
            push_semantic_span(&document.source, name.span, SEM_VARIABLE, tokens);
        }
        DefaultExprKind::EnumVariant { enum_name, variant } => {
            push_semantic_span(&document.source, enum_name.span, SEM_ENUM, tokens);
            push_semantic_span(&document.source, variant.span, SEM_ENUM_MEMBER, tokens);
        }
        DefaultExprKind::Array(items) => {
            for item in items {
                add_default_expr_semantic(document, item, tokens);
            }
        }
        DefaultExprKind::Object(fields) => {
            for (name, value) in fields {
                push_semantic_span(&document.source, name.span, SEM_PROPERTY, tokens);
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
            push_semantic_span(&document.source, binding.span, SEM_PARAMETER, tokens);
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

fn add_check_expr_semantic(
    build: &LspBuild,
    document: &LspDocument,
    expr: &CheckExpr,
    tokens: &mut Vec<RawSemanticToken>,
) {
    match &expr.kind {
        CheckExprKind::Int(_) | CheckExprKind::Float(_) => {
            push_semantic_span(&document.source, expr.span, SEM_NUMBER, tokens);
        }
        CheckExprKind::Bool(_) | CheckExprKind::Null => {
            push_semantic_span(&document.source, expr.span, SEM_KEYWORD, tokens);
        }
        CheckExprKind::String(_) => {
            push_semantic_span(&document.source, expr.span, SEM_STRING, tokens);
        }
        CheckExprKind::Name(_) => {
            push_semantic_span(&document.source, expr.span, SEM_VARIABLE, tokens);
        }
        CheckExprKind::Field { expr, name } => {
            if let CheckExprKind::Name(enum_name) = &expr.kind {
                if enum_variant_exists(build, enum_name, &name.name) {
                    push_semantic_span(&document.source, expr.span, SEM_ENUM, tokens);
                    push_semantic_span(&document.source, name.span, SEM_ENUM_MEMBER, tokens);
                    return;
                }
            }
            add_check_expr_semantic(build, document, expr, tokens);
            push_semantic_span(&document.source, name.span, SEM_PROPERTY, tokens);
        }
        CheckExprKind::Index { expr, index } => {
            add_check_expr_semantic(build, document, expr, tokens);
            add_check_expr_semantic(build, document, index, tokens);
        }
        CheckExprKind::Is { expr, predicate } => {
            add_check_expr_semantic(build, document, expr, tokens);
            match predicate {
                coflow_cft::ast::TypePredicate::Type(name) => {
                    push_semantic_span(&document.source, name.span, SEM_TYPE, tokens);
                }
                coflow_cft::ast::TypePredicate::Null(span) => {
                    push_semantic_span(&document.source, *span, SEM_KEYWORD, tokens);
                }
            }
        }
        CheckExprKind::Call { name, args } => {
            let token_type = if enum_name_exists(build, &name.name) {
                SEM_ENUM
            } else {
                SEM_FUNCTION
            };
            push_semantic_span(&document.source, name.span, token_type, tokens);
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

fn add_comment_semantic_tokens(source: &str, tokens: &mut Vec<RawSemanticToken>) {
    let mut line_start = 0;
    for line in source.split_inclusive('\n') {
        if let Some(comment_start) = comment_start_in_line(line) {
            let start = line_start + comment_start;
            let end = line_start + line.trim_end_matches(['\r', '\n']).len();
            push_semantic_span(source, Span::new(start, end), SEM_COMMENT, tokens);
        }
        line_start += line.len();
    }
}

fn comment_start_in_line(line: &str) -> Option<usize> {
    let mut in_string = false;
    let mut escaped = false;
    for (index, ch) in line.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
        } else if ch == '"' {
            in_string = true;
        } else if ch == '#' {
            return Some(index);
        }
    }
    None
}

#[derive(Clone)]
struct RawSemanticToken {
    line: usize,
    character: usize,
    length: usize,
    token_type: u32,
}

fn push_semantic_span(
    source: &str,
    span: Span,
    token_type: u32,
    tokens: &mut Vec<RawSemanticToken>,
) {
    if span.end <= span.start {
        return;
    }
    let start = position_from_byte(source, span.start);
    let end = position_from_byte(source, span.end);
    if start.line != end.line || end.character <= start.character {
        return;
    }
    tokens.push(RawSemanticToken {
        line: start.line,
        character: start.character,
        length: end.character - start.character,
        token_type,
    });
}

fn encode_semantic_tokens(mut tokens: Vec<RawSemanticToken>) -> Vec<u32> {
    tokens.sort_by_key(|token| (token.line, token.character, token.length));
    let mut deduped = Vec::new();
    let mut last_end = (0, 0);
    let mut has_last = false;
    for token in tokens {
        if has_last && (token.line, token.character) < last_end {
            continue;
        }
        last_end = (token.line, token.character + token.length);
        has_last = true;
        deduped.push(token);
    }

    let mut data = Vec::with_capacity(deduped.len() * 5);
    let mut previous_line = 0;
    let mut previous_character = 0;
    for token in deduped {
        let delta_line = token.line - previous_line;
        let delta_start = if delta_line == 0 {
            token.character - previous_character
        } else {
            token.character
        };
        data.push(usize_to_u32_saturating(delta_line));
        data.push(usize_to_u32_saturating(delta_start));
        data.push(usize_to_u32_saturating(token.length));
        data.push(token.token_type);
        data.push(0);
        previous_line = token.line;
        previous_character = token.character;
    }
    data
}

fn usize_to_u32_saturating(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn current_type_at<'a>(
    build: &'a LspBuild,
    document: &LspDocument,
    offset: usize,
) -> Option<&'a CftSchemaType> {
    build.container()?.all_types().find(|ty| {
        ty.module.as_str() == document.module_id && ty.span.start <= offset && offset <= ty.span.end
    })
}

fn completion_scope(document: &LspDocument, offset: usize) -> CompletionScope {
    let Some(ast) = &document.ast else {
        return CompletionScope::TopLevel;
    };

    for item in &ast.items {
        match item {
            Item::Enum(enum_def)
                if enum_def.span.start <= offset && offset <= enum_def.span.end =>
            {
                return CompletionScope::EnumBody;
            }
            Item::Type(ty) if ty.span.start <= offset && offset <= ty.span.end => {
                if check_block_contains(ty.check.as_ref(), offset) {
                    return CompletionScope::CheckBlock;
                }
                return CompletionScope::TypeBody;
            }
            Item::Const(_) | Item::Enum(_) | Item::Type(_) => {}
        }
    }

    CompletionScope::TopLevel
}

fn check_block_contains(check: Option<&coflow_cft::ast::CheckBlock>, offset: usize) -> bool {
    check.is_some_and(|check| check.span.start <= offset && offset <= check.span.end)
}

fn current_field_at(document: &LspDocument, offset: usize) -> Option<&coflow_cft::ast::FieldDef> {
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

fn type_of_chain(
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
    ref_target(field).map_or_else(|| field.ty_ref.clone(), CftSchemaTypeRef::Named)
}

fn ref_target(field: &CftSchemaField) -> Option<String> {
    field
        .annotations
        .iter()
        .find(|annotation| annotation.name == "ref")
        .and_then(|annotation| annotation.args.first())
        .and_then(|arg| match arg {
            CftAnnotationValue::Name(name) | CftAnnotationValue::String(name) => Some(name.clone()),
            _ => None,
        })
}

fn type_name_of_schema_ref(ty: &CftSchemaTypeRef) -> Option<&str> {
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

fn annotation_documentation(annotation: &Annotation) -> Option<(&'static str, &'static str)> {
    let label = format!("@{}", annotation.name);
    ANNOTATIONS
        .iter()
        .find(|item| item.label == label)
        .map(|item| (item.label, item.documentation))
}

fn static_documentation(text: &str) -> Option<&'static str> {
    KEYWORDS
        .iter()
        .chain(PRIMITIVE_TYPES)
        .chain(LITERALS)
        .chain(BUILTIN_FUNCTIONS)
        .find_map(|(label, documentation)| (*label == text).then_some(*documentation))
        .or_else(|| {
            ANNOTATIONS
                .iter()
                .find(|annotation| annotation.label == text)
                .map(|annotation| annotation.documentation)
        })
}

fn is_builtin_name(name: &str) -> bool {
    KEYWORDS
        .iter()
        .chain(PRIMITIVE_TYPES)
        .chain(LITERALS)
        .chain(BUILTIN_FUNCTIONS)
        .any(|(label, _)| *label == name)
}

fn quantifier_bindings_at(document: &LspDocument, offset: usize) -> Vec<String> {
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

fn is_annotation_completion_context(line_prefix: &str) -> bool {
    let Some(index) = line_prefix.rfind('@') else {
        return false;
    };
    line_prefix[index + 1..].chars().all(is_ident_continue)
}

fn is_type_predicate_context(line_prefix: &str) -> bool {
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

fn is_ref_annotation_context(line_prefix: &str) -> bool {
    let trimmed = line_prefix.trim_end();
    let Some(open) = trimmed.rfind('(') else {
        return false;
    };
    let before_open = trimmed[..open].trim_end();
    before_open.ends_with("@ref")
}

fn is_type_header_parent_context(line_prefix: &str) -> bool {
    let Some(colon) = line_prefix.rfind(':') else {
        return false;
    };
    let before_colon = &line_prefix[..colon];
    before_colon.contains("type")
}

fn is_type_reference_context(line_prefix: &str) -> bool {
    let trimmed = line_prefix.trim_end();
    let Some(colon) = trimmed.rfind(':') else {
        return false;
    };
    let after_colon = &trimmed[colon + 1..];
    !after_colon.contains(';') && !after_colon.contains('=')
}

fn is_const_value_context(line_prefix: &str) -> bool {
    let trimmed = line_prefix.trim_end();
    trimmed.contains("const ") && trimmed.contains('=') && !trimmed.contains(';')
}

fn is_field_default_context(line_prefix: &str) -> bool {
    let trimmed = line_prefix.trim_end();
    let Some(equal) = trimmed.rfind('=') else {
        return false;
    };
    let Some(colon) = trimmed.rfind(':') else {
        return false;
    };
    colon < equal && !trimmed[equal + 1..].contains(';')
}

fn top_level_needs_type_keyword(line_prefix: &str) -> bool {
    matches!(last_ident(line_prefix), Some("abstract" | "sealed"))
}

fn is_trivia_position(source: &str, offset: usize) -> bool {
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

fn receiver_chain_before_dot(line_prefix: &str) -> Option<Vec<String>> {
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

fn line_prefix_at(source: &str, offset: usize) -> &str {
    let start = source[..offset].rfind('\n').map_or(0, |index| index + 1);
    &source[start..offset]
}

fn format_cft(source: &str) -> String {
    let mut output = String::new();
    let mut indent = 0usize;
    let ended_with_newline = source.ends_with('\n');

    for raw_line in source.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            output.push('\n');
            continue;
        }
        if starts_with_closing_delimiter(trimmed) {
            indent = indent.saturating_sub(1);
        }
        output.push_str(&"  ".repeat(indent));
        output.push_str(trimmed);
        output.push('\n');
        indent = adjusted_indent(indent, trimmed);
    }

    if !ended_with_newline && output.ends_with('\n') {
        output.pop();
    }
    output
}

fn starts_with_closing_delimiter(line: &str) -> bool {
    line.starts_with('}') || line.starts_with(']')
}

fn adjusted_indent(mut indent: usize, line: &str) -> usize {
    let mut in_string = false;
    let mut escaped = false;
    for ch in line.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '#' => break,
            '{' | '[' => indent += 1,
            '}' | ']' => indent = indent.saturating_sub(1),
            _ => {}
        }
    }
    indent
}

fn full_document_range(source: &str) -> Value {
    let end = position_from_byte(source, source.len());
    lsp_range(0, 0, end.line, end.character)
}

fn byte_range(source: &str, start: usize, end: usize) -> Value {
    let start = position_from_byte(source, start);
    let end = position_from_byte(source, end);
    lsp_range(start.line, start.character, end.line, end.character)
}

fn range_from_span(source: &str, span: Span) -> Value {
    byte_range(source, span.start, span.end.max(span.start + 1))
}

fn byte_offset_from_position(source: &str, position: LspPosition) -> usize {
    let mut line = 0;
    let mut character = 0;
    for (byte_index, ch) in source.char_indices() {
        if line == position.line && character >= position.character {
            return byte_index;
        }
        if ch == '\n' {
            if line == position.line {
                return byte_index;
            }
            line += 1;
            character = 0;
        } else {
            character += ch.len_utf16();
        }
    }
    source.len()
}

fn position_from_byte(source: &str, byte_offset: usize) -> LspPosition {
    let target = byte_offset.min(source.len());
    let mut line = 0;
    let mut character = 0;
    for (byte_index, ch) in source.char_indices() {
        if byte_index >= target {
            break;
        }
        if ch == '\n' {
            line += 1;
            character = 0;
        } else {
            character += ch.len_utf16();
        }
    }
    LspPosition { line, character }
}

fn read_message<R: BufRead>(reader: &mut R) -> Result<Option<Vec<u8>>, String> {
    let mut content_length = None;
    let mut saw_header = false;

    loop {
        let mut line = String::new();
        let bytes = reader
            .read_line(&mut line)
            .map_err(|err| format!("failed to read LSP header: {err}"))?;

        if bytes == 0 {
            return if saw_header {
                Err("unexpected EOF while reading LSP headers".to_string())
            } else {
                Ok(None)
            };
        }

        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }

        saw_header = true;
        if let Some((name, value)) = trimmed.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = Some(
                    value
                        .trim()
                        .parse::<usize>()
                        .map_err(|err| format!("invalid LSP Content-Length: {err}"))?,
                );
            }
        }
    }

    let content_length =
        content_length.ok_or_else(|| "missing LSP Content-Length header".to_string())?;
    if content_length > MAX_LSP_CONTENT_LENGTH {
        return Err(format!(
            "LSP Content-Length {content_length} exceeds maximum {MAX_LSP_CONTENT_LENGTH}"
        ));
    }
    let mut body = vec![0; content_length];
    reader
        .read_exact(&mut body)
        .map_err(|err| format!("failed to read LSP body: {err}"))?;
    Ok(Some(body))
}

fn did_open_document(params: &Value) -> Option<(String, String)> {
    let document = params.get("textDocument")?;
    Some((
        document.get("uri")?.as_str()?.to_string(),
        document.get("text")?.as_str()?.to_string(),
    ))
}

fn did_change_document(params: &Value) -> Option<(String, String)> {
    let uri = text_document_uri(params)?;
    let text = params
        .get("contentChanges")?
        .as_array()?
        .iter()
        .rev()
        .find_map(|change| change.get("text").and_then(Value::as_str))?
        .to_string();
    Some((uri, text))
}

fn did_save_document(params: &Value) -> Option<(String, Option<String>)> {
    Some((
        text_document_uri(params)?,
        params
            .get("text")
            .and_then(Value::as_str)
            .map(str::to_string),
    ))
}

fn text_document_uri(params: &Value) -> Option<String> {
    params
        .get("textDocument")?
        .get("uri")?
        .as_str()
        .map(str::to_string)
}

fn lsp_diagnostic(diagnostic: &DiagnosticJson) -> Value {
    let related: Vec<_> = diagnostic
        .related
        .iter()
        .map(|related| {
            json!({
                "location": {
                    "uri": path_to_file_uri(Path::new(&related.path)),
                    "range": lsp_range(
                        related.start_line,
                        related.start_character,
                        related.end_line,
                        related.end_character,
                    )
                },
                "message": related.label.as_deref().unwrap_or("")
            })
        })
        .collect();

    let mut out = Map::new();
    out.insert(
        "range".to_string(),
        lsp_range(
            diagnostic.start_line,
            diagnostic.start_character,
            diagnostic.end_line,
            diagnostic.end_character,
        ),
    );
    out.insert("severity".to_string(), json!(1));
    out.insert("code".to_string(), json!(&diagnostic.code));
    out.insert(
        "source".to_string(),
        json!(format!("cft {}", diagnostic.stage)),
    );
    out.insert("message".to_string(), json!(&diagnostic.message));

    if !related.is_empty() {
        out.insert("relatedInformation".to_string(), Value::Array(related));
    }

    Value::Object(out)
}

fn lsp_error_diagnostic(code: &str, message: &str) -> Value {
    json!({
        "range": lsp_range(0, 0, 0, 1),
        "severity": 2,
        "code": code,
        "source": "cft LSP",
        "message": message
    })
}

fn preferred_diagnostic_uri(preferred_uris: &BTreeMap<PathBuf, String>, path: &Path) -> String {
    preferred_uris
        .get(&normalize_path(path))
        .cloned()
        .unwrap_or_else(|| path_to_file_uri(path))
}

fn lsp_range(
    start_line: usize,
    start_character: usize,
    end_line: usize,
    end_character: usize,
) -> Value {
    json!({
        "start": {
            "line": start_line,
            "character": start_character
        },
        "end": {
            "line": end_line,
            "character": end_character
        }
    })
}

fn path_from_file_uri(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let (authority, path) = if let Some(stripped) = rest.strip_prefix('/') {
        ("", format!("/{stripped}"))
    } else if let Some((authority, path)) = rest.split_once('/') {
        (authority, format!("/{path}"))
    } else {
        (rest, String::new())
    };
    let authority = percent_decode(authority)?;
    let decoded = percent_decode(&path)?;
    let path = if cfg!(windows) {
        if authority.is_empty() || authority.eq_ignore_ascii_case("localhost") {
            let without_leading_slash = if decoded.len() >= 3
                && decoded.as_bytes()[0] == b'/'
                && decoded.as_bytes()[2] == b':'
            {
                &decoded[1..]
            } else {
                decoded.as_str()
            };
            without_leading_slash.replace('/', "\\")
        } else {
            format!(r"\\{}{}", authority, decoded.replace('/', r"\"))
        }
    } else if authority.is_empty() || authority == "localhost" {
        decoded
    } else {
        format!("//{authority}{decoded}")
    };
    Some(PathBuf::from(path))
}

fn path_to_file_uri(path: &Path) -> String {
    let mut path = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        if let Some(stripped) = path.strip_prefix("//?/") {
            path = stripped.to_string();
        }
    }
    if cfg!(windows) && path.len() >= 2 && path.as_bytes()[1] == b':' {
        path.insert(0, '/');
    }
    format!("file://{}", percent_encode_uri_path(&path))
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = hex_value(*bytes.get(index + 1)?)?;
            let low = hex_value(*bytes.get(index + 2)?)?;
            out.push((high << 4) | low);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }

    String::from_utf8(out).ok()
}

const fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn percent_encode_uri_path(value: &str) -> String {
    let mut out = String::new();

    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' | b':' => {
                out.push(byte as char);
            }
            _ => {
                out.push('%');
                let _ = write!(out, "{byte:02X}");
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn request_errors_are_reported_without_returning_from_handler() {
        let (_cleanup, project) = test_project("lsp-request-error", "type Item { id: string; }\n");
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
    fn requests_after_shutdown_return_invalid_request() {
        let (_cleanup, project) = test_project("lsp-shutdown", "type Item { id: string; }\n");
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
    fn oversized_content_length_is_rejected_before_body_allocation() {
        let mut reader = io::Cursor::new(format!(
            "Content-Length: {}\r\n\r\n",
            16 * 1024 * 1024 + 1
        ));

        let err = read_message(&mut reader).expect_err("expected content length cap error");

        assert!(
            err.contains("exceeds"),
            "expected cap error, got `{err}`"
        );
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

    #[test]
    fn hover_and_definition_ignore_comment_and_string_words() {
        let source = "type Monster { id: string; }\n\
type Item {\n\
  note: string = \"Monster\";\n\
  # Monster\n\
  target: Monster;\n\
}\n";
        let (_cleanup, project) = test_project("lsp-trivia", source);
        let build = LspBuild::new(compile_schema_project_with_overrides(&project, &[])
            .expect("compile schema"));
        let document = build
            .documents
            .values()
            .next()
            .expect("document should exist");

        let string_position = position_from_byte(source, position_inside(source, "\"Monster\"", "Monster", 1));
        let comment_position = position_from_byte(source, position_inside(source, "# Monster", "Monster", 1));

        assert_eq!(hover_at(&build, document, &string_position), None);
        assert_eq!(hover_at(&build, document, &comment_position), None);
        assert!(definitions_at(&build, document, &string_position).is_empty());
        assert!(definitions_at(&build, document, &comment_position).is_empty());
    }

    struct TempProject(PathBuf);

    impl Drop for TempProject {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn test_project(name: &str, source: &str) -> (TempProject, Project) {
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

    fn written_messages(bytes: &[u8]) -> Vec<Value> {
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

    fn position_inside(
        source: &str,
        context: &str,
        needle: &str,
        character_offset: usize,
    ) -> usize {
        let context_start = source.find(context).expect("context");
        let needle_start = context.find(needle).expect("needle");
        context_start + needle_start + character_offset
    }
}
