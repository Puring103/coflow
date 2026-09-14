//! 会话设置命令：编辑器展示设置读写，不推进数据版本。
//!
//! 图坐标/视图/分组/workspace 全部落到 `editor-setting/editor.json`。

use std::collections::BTreeMap;

use super::SessionStore;
use crate::editor::settings::{
    read_project_settings, sanitized_column_widths, sanitized_record_groups, sanitized_views,
    sanitized_workspace, write_project_settings,
};
use crate::editor::types::{
    EditorError, EditorProjectSettings, EditorRecordGroup, EditorWorkspaceState, ViewConfig,
};

impl SessionStore {
    pub fn get_project_settings(&self, id: u32) -> Result<EditorProjectSettings, EditorError> {
        let entry = self.session(id)?;
        let session = entry.state.read();
        read_project_settings(&session.project_root)
    }

    /// 缩略名是编辑器展示设置；只接受类型的字符串字段，不改变记录身份。
    #[allow(clippy::significant_drop_tightening)]
    pub fn set_short_name_field(
        &self,
        id: u32,
        actual_type: String,
        field: Option<String>,
    ) -> Result<EditorProjectSettings, EditorError> {
        let entry = self.session(id)?;
        let mut session = entry.state.write();
        if let Some(name) = &field {
            if !session
                .queries()
                .schema_type_fields(&actual_type)
                .iter()
                .any(|(field_name, field_type)| field_name == name && field_type == "string")
            {
                return Err(EditorError::other("缩略名必须是记录类型的字符串字段"));
            }
        }
        let mut settings = read_project_settings(&session.project_root)?;
        if let Some(name) = field {
            settings.short_name_fields.insert(actual_type, name);
        } else {
            settings.short_name_fields.remove(&actual_type);
        }
        write_project_settings(&session.project_root, &settings)?;
        session.ref_target_cache.clear();
        Ok(settings)
    }

    /// 图节点坐标只属于编辑器设置，不推进配置数据版本。
    pub fn set_graph_positions(
        &self,
        id: u32,
        view_key: String,
        positions: BTreeMap<String, [f64; 2]>,
    ) -> Result<(), EditorError> {
        if positions.values().flatten().any(|value| !value.is_finite()) {
            return Err(EditorError::other("图节点坐标必须是有限数值"));
        }
        let entry = self.session(id)?;
        let session = entry.state.write();
        let mut settings = read_project_settings(&session.project_root)?;
        settings.graph_positions.insert(view_key, positions);
        write_project_settings(&session.project_root, &settings)
    }

    /// 图视图缩略/完整模式同样只是编辑器展示设置，不推进数据版本。
    pub fn set_graph_compact_mode(
        &self,
        id: u32,
        view_key: String,
        compact: bool,
    ) -> Result<(), EditorError> {
        let entry = self.session(id)?;
        let session = entry.state.write();
        let mut settings = read_project_settings(&session.project_root)?;
        settings.graph_compact_modes.insert(view_key, compact);
        write_project_settings(&session.project_root, &settings)
    }

    /// 只更新指定文件和类型的标签顺序，保留其他编辑器设置。
    pub fn set_view_order(
        &self,
        id: u32,
        file_path: String,
        actual_type: String,
        order: Vec<String>,
    ) -> Result<EditorProjectSettings, EditorError> {
        let project_root = self.project_root_for(id)?;
        let mut settings = read_project_settings(&project_root)?;
        settings
            .view_order
            .entry(file_path)
            .or_default()
            .insert(actual_type, order);
        write_project_settings(&project_root, &settings)?;
        Ok(settings)
    }

    /// Set the column widths of the implicit default table view for a
    /// (filePath, actualType).
    pub fn set_default_table_column_widths(
        &self,
        id: u32,
        file_path: String,
        actual_type: String,
        widths: BTreeMap<String, f64>,
    ) -> Result<EditorProjectSettings, EditorError> {
        let project_root = self.project_root_for(id)?;
        let mut settings = read_project_settings(&project_root)?;
        settings
            .default_table_column_widths
            .entry(file_path)
            .or_default()
            .insert(actual_type, sanitized_column_widths(widths));
        write_project_settings(&project_root, &settings)?;
        Ok(settings)
    }

    /// Overwrite the full custom-view list for a (filePath, actualType). The
    /// frontend mutates the list in memory and submits the whole thing.
    pub fn set_views(
        &self,
        id: u32,
        file_path: String,
        actual_type: String,
        views: Vec<ViewConfig>,
    ) -> Result<EditorProjectSettings, EditorError> {
        let project_root = self.project_root_for(id)?;
        let mut settings = read_project_settings(&project_root)?;
        let valid_group_ids = settings
            .record_groups
            .get(&file_path)
            .and_then(|by_type| by_type.get(&actual_type))
            .map(|groups| groups.iter().map(|group| group.id.clone()).collect())
            .unwrap_or_default();
        settings
            .views
            .entry(file_path)
            .or_default()
            .insert(actual_type, sanitized_views(views, &valid_group_ids));
        write_project_settings(&project_root, &settings)?;
        Ok(settings)
    }

    /// Update just the `column_widths` of one custom table view in place.
    #[allow(clippy::needless_pass_by_value)]
    pub fn set_view_column_widths(
        &self,
        id: u32,
        file_path: String,
        actual_type: String,
        view_id: String,
        widths: BTreeMap<String, f64>,
    ) -> Result<EditorProjectSettings, EditorError> {
        let project_root = self.project_root_for(id)?;
        let mut settings = read_project_settings(&project_root)?;
        if let Some(view) = settings
            .views
            .get_mut(&file_path)
            .and_then(|by_type| by_type.get_mut(&actual_type))
            .and_then(|views| views.iter_mut().find(|view| view.id == view_id))
        {
            view.column_widths = sanitized_column_widths(widths);
            write_project_settings(&project_root, &settings)?;
        }
        Ok(settings)
    }

    pub fn set_record_groups(
        &self,
        id: u32,
        file_path: String,
        actual_type: String,
        groups: Vec<EditorRecordGroup>,
    ) -> Result<EditorProjectSettings, EditorError> {
        let project_root = self.project_root_for(id)?;
        let mut settings = read_project_settings(&project_root)?;
        settings
            .record_groups
            .entry(file_path)
            .or_default()
            .insert(actual_type, sanitized_record_groups(groups));
        write_project_settings(&project_root, &settings)?;
        Ok(settings)
    }

    pub fn set_workspace(
        &self,
        id: u32,
        workspace: EditorWorkspaceState,
    ) -> Result<EditorProjectSettings, EditorError> {
        let project_root = self.project_root_for(id)?;
        let mut settings = read_project_settings(&project_root)?;
        settings.workspace = sanitized_workspace(workspace);
        write_project_settings(&project_root, &settings)?;
        Ok(settings)
    }
}
