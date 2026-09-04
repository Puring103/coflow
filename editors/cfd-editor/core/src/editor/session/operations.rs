//! Record queries and mutation commands for loaded editor sessions.

use super::{
    BatchWriteFieldEditOutcome, BatchWriteFieldInput, BatchWriteFieldOutcome, CfdValue,
    CollectionEdit, CreateRecordDraft, DefaultMaterialization, DeleteRecordOutcome, EditorError,
    EditorSession, FileRecords, GraphData, GraphQuery, InsertRecordOutcome, MutationFields,
    MutationOp, MutationRequest, MutationValue, PluginSchemaField, PluginSchemaType,
    ProjectSearchHit, ProjectSearchMode, ProjectSearchResults,
    RecordCoordinate, RecordRow, RefTarget, RenameRecordOutcome, ReorderRecordsOutcome,
    SessionStore, WireContext, WriteFieldOutcome, api_diagnostics_to_editor_error,
    apply_collection_edit, create_record_draft_to_wire, file_records_for_session,
    finalize_mutation, graph, record_container_index, record_type_index, record_view_to_row,
    reorder_file_path, snapshot_record_before_delete, write_field_in_session,
};
use crate::editor::{ProjectBootstrap, types::{
    FunctionDocumentState, LanguageCompletion, LanguageDiagnostic, LanguageDocumentState,
    LanguageFormattingResult, LanguagePosition, LanguageRange, LanguageTextEdit,
}};
use atomicwrites::{AllowOverwrite, AtomicFile};
use coflow_runtime::{
    DataSourceTextOverride, FlatDiagnostic, Project, ProjectRuntime, RecordSearchMode,
    RecordSearchOptions, Runtime, SchemaTextOverride,
};
use serde_json::{Value, json};
use std::io::Write;

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
        .notify(method, params)
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
            character = character.saturating_add(ch.len_utf16() as u32);
        }
    }
    source.len()
}

impl SessionStore {
    pub fn read_source_text(&self, id: u32, file_path: &str) -> Result<String, EditorError> {
        let path = self.source_file_path(id, file_path)?;
        std::fs::read_to_string(&path).map_err(|error| {
            EditorError::project(format!("failed to read {}: {error}", path.display()))
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
        let uri = coflow::lsp::EmbeddedLsp::file_uri(&path);
        let entry = self.session(id)?;
        let mut session = entry
            .state
            .write()
            .map_err(|_| EditorError::session("session poisoned"))?;
        let mut notifications =
            synchronize_language_document(&mut session, &uri, file_path, source, version)?;
        let (tokens, emitted) = session
            .language_server
            .request(
                "textDocument/semanticTokens/full",
                json!({ "textDocument": { "uri": uri } }),
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
            semantic_token_types: coflow::lsp::EmbeddedLsp::semantic_token_types(),
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
        let uri = coflow::lsp::EmbeddedLsp::file_uri(&path);
        let entry = self.session(id)?;
        let mut session = entry
            .state
            .write()
            .map_err(|_| EditorError::session("session poisoned"))?;
        synchronize_language_document(&mut session, &uri, file_path, source, version)?;
        let (result, _) = session
            .language_server
            .request(
                "textDocument/completion",
                json!({
                    "textDocument": { "uri": uri },
                    "position": { "line": position.line, "character": position.character },
                }),
            )
            .map_err(EditorError::other)?;
        Ok(completion_items(&result))
    }

    pub fn format_language_document(
        &self,
        id: u32,
        file_path: &str,
        source: &str,
        version: i64,
    ) -> Result<LanguageFormattingResult, EditorError> {
        let path = self.source_file_path(id, file_path)?;
        let uri = coflow::lsp::EmbeddedLsp::file_uri(&path);
        let entry = self.session(id)?;
        let mut session = entry
            .state
            .write()
            .map_err(|_| EditorError::session("session poisoned"))?;
        synchronize_language_document(&mut session, &uri, file_path, source, version)?;
        let (result, _) = session
            .language_server
            .request(
                "textDocument/formatting",
                json!({
                    "textDocument": { "uri": uri },
                    "options": { "tabSize": 2, "insertSpaces": true },
                }),
            )
            .map_err(EditorError::other)?;
        let edits = formatting_edits(&result);
        Ok(LanguageFormattingResult {
            text: formatting_text(source, &edits),
            edits,
        })
    }

    pub fn close_language_document(&self, id: u32, file_path: &str) -> Result<(), EditorError> {
        let path = self.source_file_path(id, file_path)?;
        let uri = coflow::lsp::EmbeddedLsp::file_uri(&path);
        let entry = self.session(id)?;
        let mut session = entry
            .state
            .write()
            .map_err(|_| EditorError::session("session poisoned"))?;
        if session.language_documents.remove(&uri) {
            session.language_diagnostics.remove(&uri);
            session
                .language_server
                .notify(
                    "textDocument/didClose",
                    json!({ "textDocument": { "uri": uri } }),
                )
                .map_err(EditorError::other)?;
        }
        Ok(())
    }

    pub fn function_document(
        &self,
        id: u32,
        source: &str,
        body: Option<&str>,
    ) -> Result<FunctionDocumentState, EditorError> {
        let entry = self.session(id)?;
        let mut session = entry
            .state
            .write()
            .map_err(|_| EditorError::session("session poisoned"))?;
        let (result, _) = session
            .language_server
            .request(
                "coflow/functionDocument",
                json!({ "source": source, "body": body }),
            )
            .map_err(EditorError::other)?;
        let diagnostics = result
            .get("diagnostics")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(language_diagnostic)
            .collect();
        let completions = completion_items(result.get("completions").unwrap_or(&Value::Null));
        Ok(FunctionDocumentState {
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
                .unwrap_or(LanguageRange {
                    start: LanguagePosition {
                        line: 0,
                        character: 0,
                    },
                    end: LanguagePosition {
                        line: 0,
                        character: source.chars().count() as u32,
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
            semantic_token_types: coflow::lsp::EmbeddedLsp::semantic_token_types(),
            completions,
        })
    }

    pub fn validate_source_text(
        &self,
        id: u32,
        file_path: &str,
        source: &str,
    ) -> Result<Vec<FlatDiagnostic>, EditorError> {
        let path = self.source_file_path(id, file_path)?;
        let yaml_path = self.project_action_context(id)?;
        let project = Project::open_schema_only(Some(&yaml_path))
            .map_err(|diagnostics| api_diagnostics_to_editor_error(diagnostics))?;
        if file_path.ends_with(".cft") {
            let mut schema_runtime = ProjectRuntime::new(project);
            let source_override = SchemaTextOverride {
                requested_module: None,
                normalized_path: coflow_runtime::normalize_path(&path),
                source: source.to_string(),
            };
            return match schema_runtime.refresh_with_overrides(&[source_override]) {
                Ok(_) => Ok(Vec::new()),
                Err(diagnostics) => Ok(diagnostics.flat_diagnostics()),
            };
        }
        let source_override = DataSourceTextOverride {
            normalized_path: path,
            source: source.to_string(),
            deleted: false,
        };
        let runtime = Runtime::new();
        let diagnostics = match runtime
            .open_read_only_session_with_source_overrides(project, &[source_override])
        {
            Ok(session) => session.queries().diagnostics().flat_diagnostics(),
            Err(diagnostics) => diagnostics.flat_diagnostics(),
        };
        Ok(diagnostics)
    }

    pub fn write_source_text(
        &self,
        id: u32,
        file_path: &str,
        source: &str,
    ) -> Result<ProjectBootstrap, EditorError> {
        let path = self.source_file_path(id, file_path)?;
        let normalized_path = coflow_runtime::normalize_path(&path);
        let diagnostics = if file_path.ends_with(".cft") {
            let yaml_path = self.project_action_context(id)?;
            let project = Project::open_schema_only(Some(&yaml_path))
                .map_err(api_diagnostics_to_editor_error)?;
            let mut schema_runtime = ProjectRuntime::new(project.clone());
            let schema_override = SchemaTextOverride {
                requested_module: None,
                normalized_path,
                source: source.to_string(),
            };
            schema_runtime
                .refresh_with_overrides(&[schema_override])
                .map_err(api_diagnostics_to_editor_error)?;
            let schema_session = schema_runtime
                .into_latest_attempt()
                .ok_or_else(|| EditorError::project("candidate schema disappeared"))?;
            Runtime::new()
                .open_write_session_from_schema(schema_session)
                .map_err(api_diagnostics_to_editor_error)?
                .queries()
                .diagnostics()
                .flat_diagnostics()
        } else {
            let source_override = DataSourceTextOverride {
                normalized_path,
                source: source.to_string(),
                deleted: false,
            };
            let yaml_path = self.project_action_context(id)?;
            let project = Project::open_schema_only(Some(&yaml_path))
                .map_err(api_diagnostics_to_editor_error)?;
            Runtime::new()
                .open_write_session_with_source_overrides(project, &[source_override])
                .map_err(api_diagnostics_to_editor_error)?
                .queries()
                .diagnostics()
                .flat_diagnostics()
        };
        let blocking = diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic.severity == "error"
                    && diagnostic.stage != "CHECK"
                    && !matches!(diagnostic.code.as_str(), "DATA-006" | "REF-001" | "REF-002")
            })
            .cloned()
            .collect::<Vec<_>>();
        if !blocking.is_empty() {
            return Err(EditorError::write("source contains errors").with_diagnostics(blocking));
        }

        AtomicFile::new(&path, AllowOverwrite)
            .write(|file| file.write_all(source.as_bytes()))
            .map_err(|error| {
                EditorError::write(format!("failed to write {}: {error}", path.display()))
            })?;
        let entry = self.session(id)?;
        let mut session = entry
            .state
            .write()
            .map_err(|_| EditorError::session("session poisoned after source write"))?;
        session.commit_internal_write(&[file_path.to_string()]);
        drop(session);
        self.reload_session(id)
    }

    pub fn get_file_records(&self, id: u32, file_path: &str) -> Result<FileRecords, EditorError> {
        let entry = self.session(id)?;
        let session_lock = &entry.state;
        let session = session_lock
            .read()
            .map_err(|_| EditorError::session("session poisoned"))?;
        Ok(file_records_for_session(&session, file_path))
    }

    #[allow(clippy::significant_drop_tightening)]
    pub fn search_records(
        &self,
        id: u32,
        query: &str,
        mode: ProjectSearchMode,
        limit: usize,
    ) -> Result<ProjectSearchResults, EditorError> {
        let entry = self.session(id)?;
        let session = entry
            .state
            .read()
            .map_err(|_| EditorError::session("session poisoned"))?;
        let results = session.queries().search_records(&RecordSearchOptions {
            pattern: query.to_string(),
            mode: match mode {
                ProjectSearchMode::Key => RecordSearchMode::Key,
                ProjectSearchMode::FullText => RecordSearchMode::FullText,
            },
            file: None,
            actual_type: None,
            limit: Some(limit),
            offset: 0,
        });
        Ok(ProjectSearchResults {
            revision: session.revisions.current(),
            hits: results
                .hits
                .into_iter()
                .map(|hit| ProjectSearchHit {
                    file_path: hit.file_path,
                    coordinate: hit.coordinate,
                    field_path: hit.field_path,
                    preview: hit.preview,
                })
                .collect(),
            truncated: results.truncated,
        })
    }

    /// Returns the schema projection available to read-only editor extensions.
    #[allow(clippy::significant_drop_tightening)]
    pub fn get_plugin_schema(&self, id: u32) -> Result<Vec<PluginSchemaType>, EditorError> {
        let entry = self.session(id)?;
        let session = entry
            .state
            .read()
            .map_err(|_| EditorError::session("session poisoned"))?;
        let queries = session.queries();
        Ok(queries
            .schema_type_names()
            .into_iter()
            .map(|name| PluginSchemaType {
                fields: queries
                    .schema_type_fields(&name)
                    .into_iter()
                    .map(|(name, type_label)| PluginSchemaField { name, type_label })
                    .collect(),
                is_singleton: queries.type_is_singleton(&name),
                record_count: queries.record_count_for_type(&name),
                name,
            })
            .collect())
    }

    /// Returns records whose actual type exactly matches `type_name`, across all source files.
    #[allow(clippy::significant_drop_tightening)]
    pub fn get_plugin_records_by_type(
        &self,
        id: u32,
        type_name: &str,
    ) -> Result<Vec<RecordRow>, EditorError> {
        let entry = self.session(id)?;
        let session = entry
            .state
            .read()
            .map_err(|_| EditorError::session("session poisoned"))?;
        let queries = session.queries();
        if !queries.schema_has_type(type_name) {
            return Err(EditorError::not_found(format!(
                "schema type `{type_name}` not found"
            )));
        }
        let ctx = WireContext::new(queries, &session.diagnostics);
        Ok(queries
            .source_files()
            .flat_map(|file| queries.record_views_in_file(file))
            .filter(|view| view.coordinate.actual_type.as_str() == type_name)
            .map(|view| record_view_to_row(&view, &ctx))
            .collect())
    }

    pub fn make_default_object(&self, id: u32, type_name: &str) -> Result<CfdValue, EditorError> {
        let entry = self.session(id)?;
        let session_lock = &entry.state;
        let session = session_lock
            .read()
            .map_err(|_| EditorError::session("session poisoned"))?;
        session
            .engine
            .default_record_value(type_name, DefaultMaterialization::EditableShape)
            .map_err(api_diagnostics_to_editor_error)
    }

    pub fn create_record_draft(
        &self,
        id: u32,
        actual_type: &str,
    ) -> Result<CreateRecordDraft, EditorError> {
        let entry = self.session(id)?;
        let session_lock = &entry.state;
        let session = session_lock
            .read()
            .map_err(|_| EditorError::session("session poisoned"))?;
        let draft = session
            .engine
            .create_record_draft(actual_type)
            .map_err(api_diagnostics_to_editor_error)?;
        let ctx = WireContext::new(session.queries(), &session.diagnostics);
        let wire = create_record_draft_to_wire(&draft, &ctx);
        drop(session);
        Ok(wire)
    }

    pub fn render_cell_text(
        &self,
        id: u32,
        coordinate: &RecordCoordinate,
        field_path: &[coflow_runtime::CfdPathSegment],
    ) -> Result<String, EditorError> {
        let entry = self.session(id)?;
        let session = entry
            .state
            .read()
            .map_err(|_| EditorError::session("session poisoned"))?;
        session
            .engine
            .render_cell_text(coordinate, field_path)
            .map_err(api_diagnostics_to_editor_error)
    }

    pub fn parse_cell_text(
        &self,
        id: u32,
        coordinate: &RecordCoordinate,
        field_path: &[coflow_runtime::CfdPathSegment],
        text: &str,
    ) -> Result<CfdValue, EditorError> {
        let entry = self.session(id)?;
        let session = entry
            .state
            .read()
            .map_err(|_| EditorError::session("session poisoned"))?;
        session
            .engine
            .parse_cell_text(coordinate, field_path, text)
            .map_err(api_diagnostics_to_editor_error)
    }

    pub fn get_enum_variants(
        &self,
        id: u32,
        enum_name: &str,
    ) -> Result<Vec<crate::editor::types::EnumVariantOption>, EditorError> {
        let entry = self.session(id)?;
        let session_lock = &entry.state;
        let session = session_lock
            .read()
            .map_err(|_| EditorError::session("session poisoned"))?;
        Ok(session
            .queries()
            .enum_variant_options(enum_name)
            .into_iter()
            .map(
                |(name, value, label, description)| crate::editor::types::EnumVariantOption {
                    name,
                    value,
                    label,
                    description,
                },
            )
            .collect())
    }

    /// Records assignable to `expected_type`, surfaced as `RefTarget`s so
    /// the front-end can render `Type.key` and jump directly.
    pub fn get_ref_targets(
        &self,
        id: u32,
        expected_type: &str,
    ) -> Result<Vec<RefTarget>, EditorError> {
        let entry = self.session(id)?;
        let session_lock = &entry.state;
        let targets = {
            let mut session = session_lock
                .write()
                .map_err(|_| EditorError::session("session poisoned"))?;
            if let Some(cached) = session.ref_target_cache.get(expected_type) {
                return Ok(cached.clone());
            }
            let targets: Vec<RefTarget> = session
                .queries()
                .ref_targets(expected_type)
                .into_iter()
                .map(|target| RefTarget {
                    coordinate: target.coordinate,
                    file_path: target.file_path,
                })
                .collect();
            session
                .ref_target_cache
                .insert(expected_type.to_string(), targets.clone());
            targets
        };
        Ok(targets)
    }

    pub fn get_graph(&self, id: u32, query: &GraphQuery) -> Result<GraphData, EditorError> {
        let entry = self.session(id)?;
        let session_lock = &entry.state;
        let session = session_lock
            .read()
            .map_err(|_| EditorError::session("session poisoned"))?;
        Ok(graph::build_graph(&session, query))
    }

    /// Persist a single field edit addressed by its owner record coordinate.
    #[allow(clippy::too_many_lines)]
    pub fn write_field(
        &self,
        id: u32,
        coordinate: &RecordCoordinate,
        field_path: &[coflow_runtime::CfdPathSegment],
        new_value: &CfdValue,
    ) -> Result<WriteFieldOutcome, EditorError> {
        let entry = self.session(id)?;
        let mut session = entry
            .state
            .write()
            .map_err(|_| EditorError::session("session poisoned"))?;
        write_field_in_session(&mut session, coordinate, field_path, new_value)
    }

    pub fn write_fields(
        &self,
        id: u32,
        writes: &[BatchWriteFieldInput],
    ) -> Result<BatchWriteFieldOutcome, EditorError> {
        let entry = self.session(id)?;
        let mut session = entry
            .state
            .write()
            .map_err(|_| EditorError::session("session poisoned"))?;
        let mut seen = Vec::<(RecordCoordinate, Vec<coflow_runtime::CfdPathSegment>)>::new();
        let targets = writes
            .iter()
            .filter(|write| {
                if seen.iter().any(|(coordinate, path)| {
                    coordinate == &write.coordinate && path == &write.field_path
                }) {
                    false
                } else {
                    seen.push((write.coordinate.clone(), write.field_path.clone()));
                    true
                }
            })
            .filter_map(|write| {
                let old_value = session
                    .queries()
                    .effective_field_write(&write.coordinate, &write.field_path)
                    .and_then(|preview| preview.old_value);
                (old_value.as_ref() != Some(&write.new_value)).then(|| (write.clone(), old_value))
            })
            .collect::<Vec<_>>();
        if targets.is_empty() {
            return Err(EditorError::write("batch field write contains no changes"));
        }
        let report = coflow::commands::apply_project_mutation(
            &mut session.engine,
            MutationRequest {
                stop_on_write_error: true,
                ops: targets
                    .iter()
                    .map(|(write, _)| MutationOp::SetField {
                        record: write.coordinate.clone(),
                        file: None,
                        path: write.field_path.clone(),
                        value: MutationValue::Cfd(write.new_value.clone()),
                    })
                    .collect(),
            },
        )
        .map_err(api_diagnostics_to_editor_error)?;
        let report = finalize_mutation(&mut session, report, "batch field write failed")?;
        let edits = targets
            .into_iter()
            .enumerate()
            .map(|(index, (write, old_value))| {
                let final_coordinate = report
                    .applied
                    .iter()
                    .find(|applied| applied.index == index)
                    .and_then(|applied| applied.outcome.renamed.as_ref())
                    .and_then(|(old, new)| (old == &write.coordinate).then(|| new.clone()))
                    .unwrap_or_else(|| write.coordinate.clone());
                let new_value = session
                    .queries()
                    .field_value(
                        &final_coordinate.actual_type,
                        &final_coordinate.key,
                        &write.field_path,
                    )
                    .cloned();
                BatchWriteFieldEditOutcome {
                    coordinate: write.coordinate,
                    final_coordinate,
                    field_path: write.field_path,
                    old_value,
                    new_value,
                }
            })
            .collect();
        Ok(BatchWriteFieldOutcome {
            revision: session.revisions.current(),
            edits,
            diagnostics: report.diagnostics,
            affected_files: report.affected_files,
        })
    }

    pub fn edit_collection(
        &self,
        id: u32,
        coordinate: &RecordCoordinate,
        field_path: &[coflow_runtime::CfdPathSegment],
        edit: CollectionEdit,
    ) -> Result<WriteFieldOutcome, EditorError> {
        let entry = self.session(id)?;
        let mut session = entry
            .state
            .write()
            .map_err(|_| EditorError::session("session poisoned"))?;
        let current = session
            .queries()
            .field_value(&coordinate.actual_type, &coordinate.key, field_path)
            .cloned()
            .ok_or_else(|| EditorError::not_found("collection field not found"))?;
        let default_item = session
            .engine
            .default_collection_item_value_for_record(coordinate, field_path)
            .ok();
        let next = apply_collection_edit(current, edit, default_item)?;
        let outcome = write_field_in_session(&mut session, coordinate, field_path, &next);
        drop(session);
        outcome
    }

    pub fn insert_record(
        &self,
        id: u32,
        file_path: &str,
        record_key: &str,
        actual_type: &str,
        fields: CfdValue,
    ) -> Result<InsertRecordOutcome, EditorError> {
        self.insert_record_with_materialization(
            id,
            file_path,
            record_key,
            actual_type,
            fields,
            DefaultMaterialization::Minimal,
        )
    }

    pub fn insert_record_with_materialization(
        &self,
        id: u32,
        file_path: &str,
        record_key: &str,
        actual_type: &str,
        fields: CfdValue,
        materialization: DefaultMaterialization,
    ) -> Result<InsertRecordOutcome, EditorError> {
        let entry = self.session(id)?;
        let session_lock = &entry.state;
        let CfdValue::Object(boxed) = fields else {
            return Err(EditorError::write(
                "insert_record requires a CfdValue::Object for fields",
            ));
        };
        let fields_map = boxed
            .fields
            .into_iter()
            .map(|(name, value)| (name.to_string(), value))
            .collect();

        let mut session = session_lock
            .write()
            .map_err(|_| EditorError::session("session poisoned"))?;
        let report = coflow::commands::apply_project_mutation(
            &mut session.engine,
            MutationRequest {
                stop_on_write_error: true,
                ops: vec![MutationOp::InsertRecord {
                    file: file_path.to_string(),
                    actual_type: actual_type.to_string(),
                    key: record_key.to_string(),
                    fields: MutationFields::Cfd(fields_map),
                    materialization,
                }],
            },
        )
        .map_err(api_diagnostics_to_editor_error)?;
        let report = finalize_mutation(&mut session, report, "insert record failed")?;
        let file_records = file_records_for_session(&session, file_path);
        Ok(InsertRecordOutcome {
            revision: session.revisions.current(),
            file_records,
            diagnostics: report.diagnostics,
            affected_files: report.affected_files,
        })
    }

    pub fn rename_record_key(
        &self,
        id: u32,
        coordinate: &RecordCoordinate,
        new_key: &str,
    ) -> Result<RenameRecordOutcome, EditorError> {
        let entry = self.session(id)?;
        let session_lock = &entry.state;
        let mut session = session_lock
            .write()
            .map_err(|_| EditorError::session("session poisoned"))?;
        let report = coflow::commands::apply_project_mutation(
            &mut session.engine,
            MutationRequest {
                stop_on_write_error: true,
                ops: vec![MutationOp::RenameRecord {
                    record: coordinate.clone(),
                    file: None,
                    new_key: new_key.to_string(),
                }],
            },
        )
        .map_err(api_diagnostics_to_editor_error)?;
        let report = finalize_mutation(&mut session, report, "rename record failed")?;
        let outcome = report
            .applied
            .first()
            .map(|applied| applied.outcome.clone())
            .ok_or_else(|| EditorError::write("rename did not apply"))?;
        let renamed = outcome
            .renamed
            .and_then(|(old, new)| (old == *coordinate).then_some(new))
            .ok_or_else(|| EditorError::write("rename did not produce a new coordinate"))?;
        let view = session
            .queries()
            .record_view(&renamed.actual_type, &renamed.key)
            .ok_or_else(|| {
                EditorError::not_found(format!(
                    "record `{}.{}` not found after rename",
                    renamed.actual_type, renamed.key
                ))
            })?;
        let ctx = WireContext::new(session.queries(), &session.diagnostics);
        let row = record_view_to_row(&view, &ctx);
        Ok(RenameRecordOutcome {
            revision: session.revisions.current(),
            row,
            diagnostics: report.diagnostics,
            renamed,
            affected_files: report.affected_files,
        })
    }

    pub fn delete_record(
        &self,
        id: u32,
        coordinate: &RecordCoordinate,
    ) -> Result<DeleteRecordOutcome, EditorError> {
        let entry = self.session(id)?;
        let session_lock = &entry.state;
        let mut session = session_lock
            .write()
            .map_err(|_| EditorError::session("session poisoned"))?;
        let deleted_snapshot = snapshot_record_before_delete(&session, coordinate);
        let file_path = deleted_snapshot
            .as_ref()
            .map(|snapshot| snapshot.display_path.clone())
            .or_else(|| {
                session
                    .queries()
                    .file_for_record(&coordinate.actual_type, &coordinate.key)
                    .map(str::to_string)
            })
            .ok_or_else(|| {
                EditorError::not_found(format!(
                    "record `{}.{}` not found",
                    coordinate.actual_type, coordinate.key
                ))
            })?;
        let report = coflow::commands::apply_project_mutation(
            &mut session.engine,
            MutationRequest {
                stop_on_write_error: true,
                ops: vec![MutationOp::DeleteRecord {
                    record: coordinate.clone(),
                    file: None,
                }],
            },
        )
        .map_err(api_diagnostics_to_editor_error)?;
        let report = finalize_mutation(&mut session, report, "delete record failed")?;
        let file_records = file_records_for_session(&session, &file_path);
        Ok(DeleteRecordOutcome {
            revision: session.revisions.current(),
            file_records,
            diagnostics: report.diagnostics,
            affected_files: report.affected_files,
            deleted_snapshot,
        })
    }

    pub fn swap_records(
        &self,
        id: u32,
        first: &RecordCoordinate,
        second: &RecordCoordinate,
    ) -> Result<ReorderRecordsOutcome, EditorError> {
        let entry = self.session(id)?;
        let mut session = entry
            .state
            .write()
            .map_err(|_| EditorError::session("session poisoned"))?;
        let file_path = reorder_file_path(&session, first)?;
        let report = coflow::commands::apply_project_mutation(
            &mut session.engine,
            MutationRequest {
                stop_on_write_error: true,
                ops: vec![MutationOp::SwapRecords {
                    first: first.clone(),
                    second: second.clone(),
                    file: Some(file_path.clone()),
                }],
            },
        )
        .map_err(api_diagnostics_to_editor_error)?;
        let report = finalize_mutation(&mut session, report, "swap records failed")?;
        Ok(ReorderRecordsOutcome {
            revision: session.revisions.current(),
            file_records: file_records_for_session(&session, &file_path),
            diagnostics: report.diagnostics,
            affected_files: report.affected_files,
            old_index: None,
            new_index: None,
        })
    }

    pub fn move_record(
        &self,
        id: u32,
        coordinate: &RecordCoordinate,
        target_index: usize,
    ) -> Result<ReorderRecordsOutcome, EditorError> {
        let entry = self.session(id)?;
        let mut session = entry
            .state
            .write()
            .map_err(|_| EditorError::session("session poisoned"))?;
        let file_path = reorder_file_path(&session, coordinate)?;
        let old_index = record_container_index(&session, coordinate).ok_or_else(|| {
            EditorError::not_found(format!(
                "record `{}.{}` not found in source order",
                coordinate.actual_type, coordinate.key
            ))
        })?;
        let report = coflow::commands::apply_project_mutation(
            &mut session.engine,
            MutationRequest {
                stop_on_write_error: true,
                ops: vec![MutationOp::MoveRecord {
                    record: coordinate.clone(),
                    target_index,
                    file: Some(file_path.clone()),
                }],
            },
        )
        .map_err(api_diagnostics_to_editor_error)?;
        let report = finalize_mutation(&mut session, report, "move record failed")?;
        Ok(ReorderRecordsOutcome {
            revision: session.revisions.current(),
            file_records: file_records_for_session(&session, &file_path),
            diagnostics: report.diagnostics,
            affected_files: report.affected_files,
            old_index: Some(old_index),
            new_index: Some(target_index),
        })
    }

    pub fn transfer_record(
        &self,
        id: u32,
        coordinate: &RecordCoordinate,
        destination_file: &str,
        target_index: usize,
    ) -> Result<ReorderRecordsOutcome, EditorError> {
        let entry = self.session(id)?;
        let mut session = entry
            .state
            .write()
            .map_err(|_| EditorError::session("session poisoned"))?;
        let source_file = reorder_file_path(&session, coordinate)?;
        let old_index = record_type_index(&session, coordinate).ok_or_else(|| {
            EditorError::not_found(format!(
                "record `{}.{}` not found in source type order",
                coordinate.actual_type, coordinate.key
            ))
        })?;
        let report = coflow::commands::apply_project_mutation(
            &mut session.engine,
            MutationRequest {
                stop_on_write_error: true,
                ops: vec![MutationOp::TransferRecord {
                    record: coordinate.clone(),
                    destination_file: destination_file.to_string(),
                    target_index,
                    source_file: Some(source_file),
                }],
            },
        )
        .map_err(api_diagnostics_to_editor_error)?;
        let report = finalize_mutation(&mut session, report, "transfer record failed")?;
        Ok(ReorderRecordsOutcome {
            revision: session.revisions.current(),
            file_records: file_records_for_session(&session, destination_file),
            diagnostics: report.diagnostics,
            affected_files: report.affected_files,
            old_index: Some(old_index),
            new_index: Some(target_index),
        })
    }
}
