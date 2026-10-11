//! 数据查询：文件行快照、搜索、插件投影、默认值/草稿、枚举/引用/图查询。
//!
//! 全部为读锁下的纯派生逻辑；引用目标带会话级缓存。

use super::super::{
    graph, mutation_apply::create_record_draft_to_wire, row_build::file_records_for_session,
    SessionStore,
};
use super::super::errors::api_diagnostics_to_editor_error;
use crate::editor::convert::{record_view_to_row, WireContext};
use crate::editor::settings::read_project_settings;
use crate::editor::types::{
    CreateRecordDraft, EditorError, FileRecords, GraphData, GraphQuery, PluginSchemaField,
    PluginSchemaType, ProjectSearchHit, ProjectSearchMode, ProjectSearchResults, RecordRow,
    RefTarget,
};
use coflow_runtime::{
    CfdValue, DefaultMaterialization, RecordCoordinate, RecordSearchMode, RecordSearchOptions,
};

impl SessionStore {
    pub fn read_source_text(&self, id: u32, file_path: &str) -> Result<String, EditorError> {
        let path = self.source_file_path(id, file_path)?;
        std::fs::read_to_string(&path).map_err(|error| {
            EditorError::project(format!("failed to read {}: {error}", path.display()))
        })
    }

    pub fn get_file_records(&self, id: u32, file_path: &str) -> Result<FileRecords, EditorError> {
        let entry = self.session(id)?;
        let session_lock = &entry.state;
        let session = session_lock.read();
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
        let session = entry.state.read();
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

    /// 返回编辑器插件可读取的 Schema 投影。
    #[allow(clippy::significant_drop_tightening)]
    pub fn get_plugin_schema(&self, id: u32) -> Result<Vec<PluginSchemaType>, EditorError> {
        let entry = self.session(id)?;
        let session = entry.state.read();
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
        let session = entry.state.read();
        let queries = session.queries();
        if !queries.schema_has_type(type_name) {
            return Err(EditorError::not_found(format!(
                "schema type `{type_name}` not found"
            )));
        }
        let ctx = WireContext::new(queries, &session.diagnostics, &session.shape_cache);
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
        let session = session_lock.read();
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
        let session = session_lock.read();
        let draft = session
            .engine
            .create_record_draft(actual_type)
            .map_err(api_diagnostics_to_editor_error)?;
        let ctx = WireContext::new(
            session.queries(),
            &session.diagnostics,
            &session.shape_cache,
        );
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
        let session = entry.state.read();
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
        let session = entry.state.read();
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
        let session = session_lock.read();
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
            let mut session = session_lock.write();
            if let Some(cached) = session.ref_target_cache.get(expected_type) {
                return Ok(cached.clone());
            }
            let settings = read_project_settings(&session.project_root)?;
            let targets: Vec<RefTarget> = session
                .queries()
                .ref_targets(expected_type)
                .into_iter()
                .map(|target| RefTarget {
                    short_name: settings
                        .short_name_fields
                        .get(target.coordinate.actual_type.as_str())
                        .and_then(|field| {
                            session
                                .queries()
                                .record_view(&target.coordinate.actual_type, &target.coordinate.key)
                                .and_then(|view| match view.record.field(field) {
                                    Some(CfdValue::String(value)) if !value.is_empty() => {
                                        Some(value.clone())
                                    }
                                    Some(CfdValue::FormattedString(value))
                                        if !value.rendered.is_empty() =>
                                    {
                                        Some(value.rendered.clone())
                                    }
                                    _ => None,
                                })
                        }),
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
        let session = session_lock.read();
        Ok(graph::build_graph(&session, query))
    }

}
