use crate::{
    compile_schema_project_with_overrides, dedupe_cft_diagnostics, normalize_path, DiagnosticJson,
    Project, SchemaSourceOverride,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

pub(crate) fn run(project: Project) -> Result<bool, String> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut server = LspServer::new(project, stdout.lock());
    let mut reader = BufReader::new(stdin.lock());

    while let Some(bytes) = read_message(&mut reader)? {
        let message: Value = serde_json::from_slice(&bytes)
            .map_err(|err| format!("failed to parse LSP JSON message: {err}"))?;
        server.handle_message(message)?;
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
    shutdown_requested: bool,
    should_exit: bool,
}

impl<W: Write> LspServer<W> {
    fn new(project: Project, writer: W) -> Self {
        Self {
            project,
            writer,
            open_documents: BTreeMap::new(),
            published_uris: BTreeSet::new(),
            shutdown_requested: false,
            should_exit: false,
        }
    }

    fn handle_message(&mut self, message: Value) -> Result<(), String> {
        let id = message.get("id").cloned();
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            return Ok(());
        };
        let params = message.get("params").unwrap_or(&Value::Null);

        match (id, method) {
            (Some(id), "initialize") => self.initialize(id),
            (None, "initialized") => Ok(()),
            (Some(id), "shutdown") => {
                self.shutdown_requested = true;
                self.write_response(id, Value::Null)
            }
            (None, "exit") => {
                self.should_exit = true;
                Ok(())
            }
            (Some(id), _) => self.write_error(id, -32601, format!("method `{method}` not found")),
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
                    self.close_document(uri)?;
                }
                Ok(())
            }
            (None, "$/cancelRequest" | "$/setTrace" | "workspace/didChangeConfiguration") => Ok(()),
            (None, _) => Ok(()),
        }
    }

    fn initialize(&mut self, id: Value) -> Result<(), String> {
        self.write_response(
            id,
            json!({
                "capabilities": {
                    "textDocumentSync": {
                        "openClose": true,
                        "change": 1,
                        "save": {
                            "includeText": false
                        }
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
                .and_modify(|document| document.text = text.clone())
                .or_insert(OpenDocument { uri, text });
            self.validate_project()?;
        }
        Ok(())
    }

    fn close_document(&mut self, uri: String) -> Result<(), String> {
        if let Some(path) = path_from_file_uri(&uri) {
            self.open_documents.remove(&normalize_path(&path));
        }
        self.publish_diagnostics(uri, Vec::new())?;
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

        let build = compile_schema_project_with_overrides(&self.project, &overrides)?;
        let diagnostics = dedupe_cft_diagnostics(build.diagnostics);
        let mut by_uri: BTreeMap<String, Vec<Value>> = BTreeMap::new();

        for diagnostic in diagnostics {
            let diagnostic = DiagnosticJson::from_cft(&diagnostic, &build.sources, &build.paths);
            let uri = path_to_file_uri(Path::new(&diagnostic.path));
            by_uri
                .entry(uri)
                .or_default()
                .push(lsp_diagnostic(&diagnostic));
        }

        let mut touched_uris = self.published_uris.clone();
        for path in build.paths.values() {
            touched_uris.insert(path_to_file_uri(Path::new(path)));
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
            self.publish_diagnostics(uri, diagnostics)?;
        }

        Ok(())
    }

    fn publish_diagnostics(&mut self, uri: String, diagnostics: Vec<Value>) -> Result<(), String> {
        self.published_uris.insert(uri.clone());
        self.write_notification(
            "textDocument/publishDiagnostics",
            json!({
                "uri": uri,
                "diagnostics": diagnostics
            }),
        )
    }

    fn write_response(&mut self, id: Value, result: Value) -> Result<(), String> {
        self.write_json(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result
        }))
    }

    fn write_error(&mut self, id: Value, code: i64, message: String) -> Result<(), String> {
        self.write_json(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": code,
                "message": message
            }
        }))
    }

    fn write_notification(&mut self, method: &str, params: Value) -> Result<(), String> {
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

    let mut out = json!({
        "range": lsp_range(
            diagnostic.start_line,
            diagnostic.start_character,
            diagnostic.end_line,
            diagnostic.end_character,
        ),
        "severity": 1,
        "code": &diagnostic.code,
        "source": format!("cft {}", diagnostic.stage),
        "message": &diagnostic.message
    });

    if !related.is_empty() {
        out.as_object_mut()
            .expect("diagnostic is an object")
            .insert("relatedInformation".to_string(), Value::Array(related));
    }

    out
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
    let decoded = percent_decode(rest)?;
    let path = if cfg!(windows) {
        let without_leading_slash =
            if decoded.len() >= 3 && decoded.as_bytes()[0] == b'/' && decoded.as_bytes()[2] == b':'
            {
                &decoded[1..]
            } else {
                decoded.as_str()
            };
        without_leading_slash.replace('/', "\\")
    } else {
        decoded
    };
    Some(PathBuf::from(path))
}

fn path_to_file_uri(path: &Path) -> String {
    let mut path = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        if let Some(stripped) = path.strip_prefix("//?/") {
            path = stripped.to_string().into();
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

fn hex_value(byte: u8) -> Option<u8> {
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
                out.push(byte as char)
            }
            _ => {
                out.push('%');
                out.push_str(&format!("{byte:02X}"));
            }
        }
    }

    out
}
