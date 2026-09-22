//! 会话项目命令：check/build/diff 与项目结构变更入口。
//!
//! 结构变更（add/create/delete input）走 `coflow-project` 后触发会话重载。

use std::path::Path as StdPath;

use super::errors::project_diagnostics_to_editor_error;
use super::SessionStore;
use crate::editor::types::{EditorError, ProjectBootstrap};

impl SessionStore {
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

    pub fn build_project(&self, id: u32) -> Result<String, EditorError> {
        let yaml_path = self.project_action_context(id)?;
        let project = coflow_project::Project::open_schema_only(Some(&yaml_path))
            .map_err(|diagnostics| project_diagnostics_to_editor_error(&diagnostics))?;
        match coflow_project::commands::build_project(&project)
            .map_err(|diagnostics| project_diagnostics_to_editor_error(&diagnostics))?
        {
            coflow_project::commands::CommandOutcome::Success(report) => {
                let mut outputs = Vec::new();
                for target in report.targets {
                    outputs.push(target.code.dir.display().to_string());
                }
                Ok(format!("Build completed: {}", outputs.join(", ")))
            }
            coflow_project::commands::CommandOutcome::Diagnostics(diagnostics) => {
                Err(project_diagnostics_to_editor_error(&diagnostics))
            }
        }
    }

    pub fn build_project_status(&self, id: u32) -> Result<bool, EditorError> {
        let yaml_path = self.project_action_context(id)?;
        let project = coflow_project::Project::open_schema_only(Some(&yaml_path))
            .map_err(|diagnostics| project_diagnostics_to_editor_error(&diagnostics))?;
        match coflow_project::commands::build_project_status(&project)
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
        let session = entry.state.read();
        session
            .queries()
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
        coflow_project::add_project_input(&yaml_path, kind, path)
            .map_err(|error| EditorError::project(super::build::diagnostic_messages(&error)))?;
        self.reload_session(id)
    }

    pub fn create_project_file(
        &self,
        id: u32,
        kind: coflow_project::ProjectInputKind,
        parent_path: &StdPath,
        file_name: &str,
    ) -> Result<ProjectBootstrap, EditorError> {
        let yaml_path = self.project_action_context(id)?;
        coflow_project::create_project_file(&yaml_path, kind, parent_path, file_name)
            .map_err(|error| EditorError::project(super::build::diagnostic_messages(&error)))?;
        self.reload_session(id)
    }

    pub fn delete_project_entry(
        &self,
        id: u32,
        path: &StdPath,
    ) -> Result<ProjectBootstrap, EditorError> {
        let yaml_path = self.project_action_context(id)?;
        coflow_project::delete_project_entry(&yaml_path, path)
            .map_err(|error| EditorError::project(super::build::diagnostic_messages(&error)))?;
        self.reload_session(id)
    }
}
