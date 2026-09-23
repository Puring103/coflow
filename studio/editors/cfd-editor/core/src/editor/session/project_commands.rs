//! 会话项目命令：check/codegen/diff 与项目结构变更入口。
//!
//! 结构变更（add/create/delete input）走 `coflow-project` 后触发会话重载。

use std::path::Path as StdPath;

use super::errors::project_diagnostics_to_editor_error;
use super::SessionStore;
use crate::editor::types::{EditorError, ProjectBootstrap};

impl SessionStore {
    /// 文件已提交后重载失败，返回明确的已提交状态，避免宿主提示用户重复写入。
    pub(super) fn reload_after_commit(
        &self,
        id: u32,
        commit: coflow_project::ProjectCommit,
    ) -> Result<ProjectBootstrap, EditorError> {
        let refresh = || {
            let entry = self.session(id)?;
            if commit.generation_changed {
                let mut state = entry.state.write();
                state.commit_internal_write(&commit.written_files);
                state.language.invalidate(
                    commit
                        .written_files
                        .iter()
                        .map(|p| state.project_root.join(p))
                        .collect(),
                );
            } else {
                let state = entry.state.read();
                return Ok(super::row_build::project_bootstrap(
                    id,
                    &state,
                    super::build::SessionSnapshotParts {
                        file_tree: state.queries().file_tree(),
                    },
                ));
            }
            self.reload_session(id)
        };
        refresh().map_err(|error: EditorError| {
            if !commit.generation_changed {
                return error;
            }
            self.mark_needs_reload(id);
            EditorError::new(
                crate::editor::types::EditorErrorKind::Committed,
                format!(
                    "文件已保存，但会话刷新失败；请重新加载项目，不要重复执行写入。{}",
                    error.message
                ),
            )
            .with_diagnostics(error.diagnostics)
        })
    }

    pub fn check_project(&self, id: u32) -> Result<String, EditorError> {
        let yaml_path = self.project_action_context(id)?;
        let project = coflow_project::Project::open_schema_only(Some(&yaml_path))
            .map_err(|diagnostics| project_diagnostics_to_editor_error(&diagnostics))?;
        match coflow_project::commands::check_project(&project)
            .map_err(|diagnostics| project_diagnostics_to_editor_error(&diagnostics))?
        {
            coflow_project::commands::CommandOutcome::Success(_) => Ok("Check passed".to_string()),
            coflow_project::commands::CommandOutcome::Diagnostics(diagnostics) => {
                Err(project_diagnostics_to_editor_error(&diagnostics))
            }
        }
    }

    pub fn generate_project_code(&self, id: u32) -> Result<String, EditorError> {
        let yaml_path = self.project_action_context(id)?;
        let project = coflow_project::Project::open_schema_only(Some(&yaml_path))
            .map_err(|diagnostics| project_diagnostics_to_editor_error(&diagnostics))?;
        match coflow_project::commands::generate_project_code(&project)
            .map_err(|diagnostics| project_diagnostics_to_editor_error(&diagnostics))?
        {
            coflow_project::commands::CommandOutcome::Success(report) => {
                let mut outputs = Vec::new();
                for target in report.targets {
                    outputs.push(target.dir.display().to_string());
                }
                Ok(format!("Codegen completed: {}", outputs.join(", ")))
            }
            coflow_project::commands::CommandOutcome::Diagnostics(diagnostics) => {
                Err(project_diagnostics_to_editor_error(&diagnostics))
            }
        }
    }

    pub fn codegen_project_status(&self, id: u32) -> Result<bool, EditorError> {
        let yaml_path = self.project_action_context(id)?;
        let project = coflow_project::Project::open_schema_only(Some(&yaml_path))
            .map_err(|diagnostics| project_diagnostics_to_editor_error(&diagnostics))?;
        match coflow_project::commands::codegen_project_status(&project)
            .map_err(|diagnostics| project_diagnostics_to_editor_error(&diagnostics))?
        {
            coflow_project::commands::CommandOutcome::Success(changed) => Ok(changed),
            coflow_project::commands::CommandOutcome::Diagnostics(diagnostics) => {
                Err(project_diagnostics_to_editor_error(&diagnostics))
            }
        }
    }

    pub fn project_diff(&self, id: u32) -> Result<coflow_project::ProjectDiff, EditorError> {
        let entry = self.session(id)?;
        let snapshot = entry.state.read().project_session.snapshot();
        snapshot
            .diff_against_head()
            .map_err(|diagnostics| project_diagnostics_to_editor_error(&diagnostics))
    }

    pub fn add_project_input(
        &self,
        id: u32,
        kind: coflow_project::ProjectInputKind,
        path: &StdPath,
    ) -> Result<ProjectBootstrap, EditorError> {
        let yaml_path = self.project_action_context(id)?;
        self.session(id)?.state.read().ensure_writable()?;
        let commit = coflow_project::add_project_input(&yaml_path, kind, path)
            .map_err(|error| EditorError::project(super::build::diagnostic_messages(&error)))?;
        self.reload_after_commit(id, commit)
    }

    pub fn create_project_file(
        &self,
        id: u32,
        kind: coflow_project::ProjectInputKind,
        parent_path: &StdPath,
        file_name: &str,
    ) -> Result<ProjectBootstrap, EditorError> {
        let yaml_path = self.project_action_context(id)?;
        self.session(id)?.state.read().ensure_writable()?;
        let commit = coflow_project::create_project_file(&yaml_path, kind, parent_path, file_name)
            .map_err(|error| EditorError::project(super::build::diagnostic_messages(&error)))?;
        self.reload_after_commit(id, commit)
    }

    pub fn delete_project_entry(
        &self,
        id: u32,
        path: &StdPath,
    ) -> Result<ProjectBootstrap, EditorError> {
        let yaml_path = self.project_action_context(id)?;
        self.session(id)?.state.read().ensure_writable()?;
        let commit = coflow_project::delete_project_entry(&yaml_path, path)
            .map_err(|error| EditorError::project(super::build::diagnostic_messages(&error)))?;
        self.reload_after_commit(id, commit)
    }
}
