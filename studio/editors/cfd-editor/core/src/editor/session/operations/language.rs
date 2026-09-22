//! 编辑器语言入口只借用独立语言状态，项目提交不等待语言计算。
use super::super::errors::api_diagnostics_to_editor_error;
use super::super::SessionStore;
use crate::editor::types::{
    EditorError, FunctionDocumentState, LanguageCompletion, LanguageDocumentState,
    LanguageFormattingResult, LanguagePosition, ProjectBootstrap,
};
use coflow_project::{DataSourceTextOverride, FlatDiagnostic, SchemaTextOverride};
fn has_extension(path: &str, expected: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case(expected))
}
impl SessionStore {
    pub fn highlight_source_snapshot(
        &self,
        id: u32,
        file_path: &str,
        source: &str,
    ) -> Result<LanguageDocumentState, EditorError> {
        let entry = self.session(id)?;
        let (language, path) = {
            let session = entry.state.read();
            (
                session.language.clone(),
                session.project_root.join(file_path),
            )
        };
        language.run(|service| service.highlight(&path, source))
    }
    pub fn sync_language_document(
        &self,
        id: u32,
        file_path: &str,
        source: &str,
        version: i64,
    ) -> Result<LanguageDocumentState, EditorError> {
        let path = self.source_file_path(id, file_path)?;
        let language = self.session(id)?.state.read().language.clone();
        language.run(|service| {
            service.synchronize(&path, source, version)?;
            service.document(&path)
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
        let language = self.session(id)?.state.read().language.clone();
        language.run(|service| {
            service.synchronize(&path, source, version)?;
            service.completion(&path, position)
        })
    }
    pub fn format_language_document(
        &self,
        id: u32,
        file_path: &str,
        source: &str,
        version: i64,
    ) -> Result<LanguageFormattingResult, EditorError> {
        let path = self.source_file_path(id, file_path)?;
        let language = self.session(id)?.state.read().language.clone();
        language.run(|service| {
            service.synchronize(&path, source, version)?;
            service.formatting(&path)
        })
    }
    pub fn close_language_document(&self, id: u32, file_path: &str) -> Result<(), EditorError> {
        let entry = self.session(id)?;
        let (language, path) = {
            let session = entry.state.read();
            (
                session.language.clone(),
                session.project_root.join(file_path),
            )
        };
        language.run(|service| service.close_document(&path))
    }
    pub fn function_document(
        &self,
        id: u32,
        source: &str,
        body: Option<&str>,
    ) -> Result<FunctionDocumentState, EditorError> {
        let language = self.session(id)?.state.read().language.clone();
        language.run(|_| {
            Ok(coflow_lsp::service::LanguageService::function_document(
                source, body,
            ))
        })
    }
    pub fn validate_source_text(
        &self,
        id: u32,
        file_path: &str,
        source: &str,
    ) -> Result<Vec<FlatDiagnostic>, EditorError> {
        let path = self.source_file_path(id, file_path)?;
        let normalized_path = coflow_project::normalize_path(&path);
        let context = self
            .session(id)?
            .state
            .read()
            .project_session
            .validation_context();
        if has_extension(file_path, "cft") {
            return context
                .validate_schema(&[SchemaTextOverride {
                    requested_module: None,
                    normalized_path,
                    source: source.to_owned(),
                }])
                .map_err(api_diagnostics_to_editor_error);
        }
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
        let context = entry.state.read().project_session.source_update_context();
        let candidate = context
            .prepare(&path, source)
            .map_err(api_diagnostics_to_editor_error)?;
        let mut session = entry.state.write();
        session.ensure_writable()?;
        let commit = session
            .project_session
            .commit_source_update(candidate)
            .map_err(api_diagnostics_to_editor_error)?;
        session.publish_commit(&commit);
        let snapshot = super::super::build::SessionSnapshotParts {
            file_tree: session.queries().file_tree(),
        };
        Ok(super::super::row_build::project_bootstrap(
            id, &session, snapshot,
        ))
    }
}
