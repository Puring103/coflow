//! LSP 文档同步与源码文本校验/落盘。
//!
//! 语言服务只借用会话内的嵌入式 LSP，不触碰引擎写接口。

use super::super::errors::api_diagnostics_to_editor_error;
use super::super::{EditorSession, SessionStore};
use crate::editor::types::{
    EditorError, FunctionDocumentState, LanguageCompletion, LanguageDiagnostic,
    LanguageDocumentState, LanguageFormattingResult, LanguagePosition, LanguageRange,
    LanguageTextEdit, ProjectBootstrap,
};
use coflow_project::{DataSourceTextOverride, FlatDiagnostic, ProjectRuntime, SchemaTextOverride};
use serde_json::{json, Value};

fn has_extension(path: &str, expected: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(expected))
}

fn synchronize_language_document(
    session: &mut EditorSession,
    uri: &str,
    file_path: &str,
    source: &str,
    version: i64,
) -> Result<Vec<Value>, EditorError> {
    let (method, params) = if session.language_documents.insert(uri.to_string()) {
        (
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "cfd",
                    "version": version,
                    "text": source,
                }
            }),
        )
    } else {
        (
            "textDocument/didChange",
            json!({
                "textDocument": { "uri": uri, "version": version },
                "contentChanges": [{ "text": source }],
            }),
        )
    };
    session
        .language_server
        .notify(method, &params)
        .map_err(|error| EditorError::other(format!("LSP sync failed for {file_path}: {error}")))
}

fn diagnostics_for_uri(messages: &[Value], uri: &str) -> Option<Vec<LanguageDiagnostic>> {
    messages
        .iter()
        .rev()
        .find(|message| {
            message.get("method").and_then(Value::as_str) == Some("textDocument/publishDiagnostics")
                && message.pointer("/params/uri").and_then(Value::as_str) == Some(uri)
        })
        .and_then(|message| message.pointer("/params/diagnostics"))
        .and_then(Value::as_array)
        .map(|diagnostics| diagnostics.iter().filter_map(language_diagnostic).collect())
}

fn language_diagnostic(value: &Value) -> Option<LanguageDiagnostic> {
    let code = value.get("code").and_then(|code| {
        code.as_str()
            .map(str::to_string)
            .or_else(|| code.as_i64().map(|code| code.to_string()))
    });
    Some(LanguageDiagnostic {
        range: language_range(value.get("range")?)?,
        severity: value
            .get("severity")
            .and_then(Value::as_u64)
            .and_then(|severity| u8::try_from(severity).ok())
            .unwrap_or(1),
        message: value.get("message")?.as_str()?.to_string(),
        code,
        source: value
            .get("source")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

fn language_range(value: &Value) -> Option<LanguageRange> {
    let position = |value: &Value| {
        Some(LanguagePosition {
            line: u32::try_from(value.get("line")?.as_u64()?).ok()?,
            character: u32::try_from(value.get("character")?.as_u64()?).ok()?,
        })
    };
    Some(LanguageRange {
        start: position(value.get("start")?)?,
        end: position(value.get("end")?)?,
    })
}

fn completion_items(value: &Value) -> Vec<LanguageCompletion> {
    let items = value
        .as_array()
        .or_else(|| value.get("items").and_then(Value::as_array));
    items
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let text_edit = item.get("textEdit").and_then(|edit| {
                Some(LanguageTextEdit {
                    range: language_range(edit.get("range")?)?,
                    new_text: edit.get("newText")?.as_str()?.to_string(),
                })
            });
            let documentation = item.get("documentation").and_then(|documentation| {
                documentation
                    .as_str()
                    .or_else(|| documentation.get("value").and_then(Value::as_str))
                    .map(str::to_string)
            });
            Some(LanguageCompletion {
                label: item.get("label")?.as_str()?.to_string(),
                detail: item
                    .get("detail")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                kind: item
                    .get("kind")
                    .and_then(Value::as_u64)
                    .and_then(|kind| u32::try_from(kind).ok()),
                insert_text: item
                    .get("insertText")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                insert_text_format: item
                    .get("insertTextFormat")
                    .and_then(Value::as_u64)
                    .and_then(|format| u32::try_from(format).ok()),
                documentation,
                sort_text: item
                    .get("sortText")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                filter_text: item
                    .get("filterText")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                text_edit,
            })
        })
        .collect()
}

fn formatting_edits(edits: &Value) -> Vec<LanguageTextEdit> {
    edits
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|edit| {
            Some(LanguageTextEdit {
                range: language_range(edit.get("range")?)?,
                new_text: edit.get("newText")?.as_str()?.to_string(),
            })
        })
        .collect()
}

fn formatting_text(source: &str, edits: &[LanguageTextEdit]) -> String {
    let mut replacements = edits
        .iter()
        .map(|edit| {
            (
                language_position_offset(source, &edit.range.start),
                language_position_offset(source, &edit.range.end),
                edit.new_text.as_str(),
            )
        })
        .collect::<Vec<_>>();
    replacements.sort_unstable_by(|left, right| right.0.cmp(&left.0).then(right.1.cmp(&left.1)));

    let mut text = source.to_string();
    for (start, end, replacement) in replacements {
        if start <= end && end <= text.len() {
            text.replace_range(start..end, replacement);
        }
    }
    text
}

fn language_position_offset(source: &str, position: &LanguagePosition) -> usize {
    let mut line = 0u32;
    let mut character = 0u32;
    for (byte_index, ch) in source.char_indices() {
        if line == position.line && character >= position.character {
            return byte_index;
        }
        if ch == '\n' {
            if line == position.line {
                return byte_index;
            }
            line = line.saturating_add(1);
            character = 0;
        } else {
            character = character.saturating_add(1 + u32::from(ch.len_utf16() == 2));
        }
    }
    source.len()
}

impl SessionStore {
    #[allow(clippy::significant_drop_tightening)]
    pub fn highlight_source_snapshot(
        &self,
        id: u32,
        file_path: &str,
        source: &str,
    ) -> Result<LanguageDocumentState, EditorError> {
        let entry = self.session(id)?;
        let mut session = entry.state.write();
        let path = session.project_root.join(file_path);
        // 只分析快照，不同步语言文档，避免 HEAD 覆盖当前草稿。
        let tokens = session
            .language_server
            .highlight_source_snapshot(&path, source);
        Ok(LanguageDocumentState {
            diagnostics: Vec::new(),
            semantic_token_data: tokens
                .get("data")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_u64)
                .filter_map(|value| u32::try_from(value).ok())
                .collect(),
            semantic_token_types: coflow_lsp::EmbeddedLsp::semantic_token_types(),
            syntax_valid: tokens
                .get("x-coflow-syntax-valid")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        })
    }

    pub fn sync_language_document(
        &self,
        id: u32,
        file_path: &str,
        source: &str,
        version: i64,
    ) -> Result<LanguageDocumentState, EditorError> {
        let path = self.source_file_path(id, file_path)?;
        let uri = coflow_lsp::EmbeddedLsp::file_uri(&path);
        let entry = self.session(id)?;
        let mut session = entry.state.write();
        let mut notifications =
            synchronize_language_document(&mut session, &uri, file_path, source, version)?;
        let (tokens, emitted) = session
            .language_server
            .request(
                "textDocument/semanticTokens/full",
                &json!({ "textDocument": { "uri": uri } }),
            )
            .map_err(EditorError::other)?;
        notifications.extend(emitted);
        if let Some(diagnostics) = diagnostics_for_uri(&notifications, &uri) {
            session
                .language_diagnostics
                .insert(uri.clone(), diagnostics);
        }
        Ok(LanguageDocumentState {
            diagnostics: session
                .language_diagnostics
                .get(&uri)
                .cloned()
                .unwrap_or_default(),
            semantic_token_data: tokens
                .get("data")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_u64)
                .filter_map(|value| u32::try_from(value).ok())
                .collect(),
            semantic_token_types: coflow_lsp::EmbeddedLsp::semantic_token_types(),
            syntax_valid: tokens
                .get("x-coflow-syntax-valid")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        })
    }

    pub fn complete_language_document(
        &self,
        id: u32,
        file_path: &str,
        source: &str,
        version: i64,
        position: &LanguagePosition,
    ) -> Result<Vec<LanguageCompletion>, EditorError> {
        let path = self.source_file_path(id, file_path)?;
        let uri = coflow_lsp::EmbeddedLsp::file_uri(&path);
        let entry = self.session(id)?;
        let mut session = entry.state.write();
        synchronize_language_document(&mut session, &uri, file_path, source, version)?;
        let (result, _) = session
            .language_server
            .request(
                "textDocument/completion",
                &json!({
                    "textDocument": { "uri": uri },
                    "position": { "line": position.line, "character": position.character },
                }),
            )
            .map_err(EditorError::other)?;
        let completions = completion_items(&result);
        drop(session);
        Ok(completions)
    }

    pub fn format_language_document(
        &self,
        id: u32,
        file_path: &str,
        source: &str,
        version: i64,
    ) -> Result<LanguageFormattingResult, EditorError> {
        let path = self.source_file_path(id, file_path)?;
        let uri = coflow_lsp::EmbeddedLsp::file_uri(&path);
        let entry = self.session(id)?;
        let mut session = entry.state.write();
        synchronize_language_document(&mut session, &uri, file_path, source, version)?;
        let (result, _) = session
            .language_server
            .request(
                "textDocument/formatting",
                &json!({
                    "textDocument": { "uri": uri },
                    "options": { "tabSize": 2, "insertSpaces": true },
                }),
            )
            .map_err(EditorError::other)?;
        let edits = formatting_edits(&result);
        let formatted = LanguageFormattingResult {
            text: formatting_text(source, &edits),
            edits,
        };
        drop(session);
        Ok(formatted)
    }

    pub fn close_language_document(&self, id: u32, file_path: &str) -> Result<(), EditorError> {
        let path = self.source_file_path(id, file_path)?;
        let uri = coflow_lsp::EmbeddedLsp::file_uri(&path);
        let entry = self.session(id)?;
        let mut session = entry.state.write();
        if session.language_documents.remove(&uri) {
            session.language_diagnostics.remove(&uri);
            session
                .language_server
                .notify(
                    "textDocument/didClose",
                    &json!({ "textDocument": { "uri": uri } }),
                )
                .map_err(EditorError::other)?;
        }
        drop(session);
        Ok(())
    }

    pub fn function_document(
        &self,
        id: u32,
        source: &str,
        body: Option<&str>,
    ) -> Result<FunctionDocumentState, EditorError> {
        let entry = self.session(id)?;
        let mut session = entry.state.write();
        let (result, _) = session
            .language_server
            .request(
                "coflow/functionDocument",
                &json!({ "source": source, "body": body }),
            )
            .map_err(EditorError::other)?;
        let diagnostics = result
            .get("diagnostics")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(language_diagnostic)
            .collect();
        let completions =
            completion_items(result.get("completions").unwrap_or_else(|| &Value::Null));
        let document = FunctionDocumentState {
            source: result
                .get("source")
                .and_then(Value::as_str)
                .unwrap_or(source)
                .to_string(),
            signature: result
                .get("signature")
                .and_then(Value::as_str)
                .unwrap_or("fn")
                .to_string(),
            body: result
                .get("body")
                .and_then(Value::as_str)
                .unwrap_or(source)
                .to_string(),
            body_range: result
                .get("bodyRange")
                .and_then(language_range)
                .unwrap_or_else(|| LanguageRange {
                    start: LanguagePosition {
                        line: 0,
                        character: 0,
                    },
                    end: LanguagePosition {
                        line: 0,
                        character: source
                            .encode_utf16()
                            .fold(0u32, |length, _| length.saturating_add(1)),
                    },
                }),
            diagnostics,
            semantic_token_data: result
                .pointer("/semanticTokens/data")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_u64)
                .filter_map(|value| u32::try_from(value).ok())
                .collect(),
            semantic_token_types: coflow_lsp::EmbeddedLsp::semantic_token_types(),
            completions,
        };
        drop(session);
        Ok(document)
    }

    pub fn validate_source_text(
        &self,
        id: u32,
        file_path: &str,
        source: &str,
    ) -> Result<Vec<FlatDiagnostic>, EditorError> {
        let path = self.source_file_path(id, file_path)?;
        let normalized_path = coflow_project::normalize_path(&path);
        if has_extension(file_path, "cft") {
            // CFT 覆盖需要重建 schema，只借用会话里的 Project，不必重新打开磁盘配置。
            let entry = self.session(id)?;
            let project = {
                let session = entry.state.read();
                session.engine.project().clone()
            };
            let mut schema_runtime = ProjectRuntime::new(project);
            let source_override = SchemaTextOverride {
                requested_module: None,
                normalized_path,
                source: source.to_string(),
            };
            return match schema_runtime.refresh_with_overrides(&[source_override]) {
                Ok(_) => Ok(Vec::new()),
                Err(diagnostics) => Ok(diagnostics.flat_diagnostics()),
            };
        }
        // CFD 覆盖复用会话源缓存：只重解析当前文件，落地与 LSP 共用的项目级诊断。
        let context = {
            let entry = self.session(id)?;
            let session = entry.state.read();
            session.engine.validation_context()
        };
        let source_override = DataSourceTextOverride {
            normalized_path,
            source: source.to_string(),
            deleted: false,
        };
        Ok(context.validate(&[source_override]))
    }

    pub fn write_source_text(
        &self,
        id: u32,
        file_path: &str,
        source: &str,
    ) -> Result<ProjectBootstrap, EditorError> {
        let path = self.source_file_path(id, file_path)?;
        let entry = self.session(id)?;
        // 候选在读锁下构建，提交只持有短写锁；版本校验阻止覆盖并发 mutation。
        let candidate = entry
            .state
            .read()
            .engine
            .prepare_source_update(&path, source)
            .map_err(api_diagnostics_to_editor_error)?;
        let mut session = entry.state.write();
        session
            .engine
            .commit_source_update(candidate)
            .map_err(api_diagnostics_to_editor_error)?;
        session.publish_commit(
            &[file_path.to_string()],
            None,
            has_extension(file_path, "cft"),
        )?;
        let snapshot = super::super::build::SessionSnapshotParts {
            file_tree: session.queries().file_tree(),
        };
        Ok(super::super::row_build::project_bootstrap(
            id, &session, snapshot,
        ))
    }
}
