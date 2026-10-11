//! 编辑器设置清洗：workspace/列宽/分组/视图的归一化。
//!
//! 后端是清洗的唯一位置，前端直接落盘提交的完整列表。

use std::collections::{BTreeMap, BTreeSet};

use super::model::{
    MAX_FIELD_LEN, MAX_VIEW_NAME_LEN, MIN_COLUMN_WIDTH, RECORD_GROUP_COLORS,
    RESERVED_VIEW_ID_PREFIX,
};
use crate::editor::types::{
    EditorRecordGroup, EditorWorkspaceState, EditorWorkspaceTab, ViewConfig, ViewKind,
};

fn workspace_tab_id(file_path: &str, type_name: &str, target: Option<&crate::editor::types::EditorDimensionTarget>) -> String {
    match target {
        Some(target) => serde_json::to_string(&[
            &target.dimension, &target.owner_file, &target.type_name,
            target.field.as_deref().unwrap_or(""),
        ]).expect("JSON 字符串数组必可序列化"),
        None => format!("{file_path}\u{1f}{type_name}"),
    }
}

pub(crate) fn sanitized_workspace(workspace: EditorWorkspaceState) -> EditorWorkspaceState {
    const MAX_TABS: usize = 100;
    const MAX_ID_LEN: usize = 512;
    let mut ids = BTreeSet::new();
    let tabs = workspace
        .tabs
        .into_iter()
        .filter_map(|tab| {
            let file_path = tab
                .file_path
                .trim()
                .chars()
                .take(MAX_ID_LEN)
                .collect::<String>();
            let type_name = tab
                .type_name
                .trim()
                .chars()
                .take(MAX_FIELD_LEN)
                .collect::<String>();
            let view_id = tab
                .view_id
                .trim()
                .chars()
                .take(MAX_ID_LEN)
                .collect::<String>();
            if file_path.is_empty() || view_id.is_empty() {
                return None;
            }
            if let Some(target) = &tab.dimension_target {
                if file_path != format!("@dimension/{}", target.dimension)
                    || !type_name.is_empty() || target.owner_file.trim().is_empty()
                    || target.type_name.trim().is_empty()
                    || (target.singleton && target.field.is_some())
                    || (!target.singleton && target.field.as_deref().is_none_or(str::is_empty)) {
                    return None;
                }
            }
            let id = workspace_tab_id(&file_path, &type_name, tab.dimension_target.as_ref());
            if !ids.insert(id) {
                return None;
            }
            Some(EditorWorkspaceTab {
                file_path,
                type_name,
                view_id,
                view_kind: tab.view_kind,
                coordinate: tab.coordinate,
                dimension_target: tab.dimension_target,
            })
        })
        .take(MAX_TABS)
        .collect::<Vec<_>>();
    let retained_ids = tabs.iter().map(|tab| workspace_tab_id(
        &tab.file_path, &tab.type_name, tab.dimension_target.as_ref(),
    )).collect::<BTreeSet<_>>();
    let active_tab_id = workspace
        .active_tab_id
        .filter(|active| retained_ids.contains(active));
    EditorWorkspaceState {
        tabs,
        active_tab_id,
    }
}

pub(crate) fn sanitized_column_widths(widths: BTreeMap<String, f64>) -> BTreeMap<String, f64> {
    widths
        .into_iter()
        .filter_map(|(column, width)| {
            width
                .is_finite()
                .then(|| (column, width.max(MIN_COLUMN_WIDTH)))
        })
        .collect()
}

pub(crate) fn sanitized_record_groups(groups: Vec<EditorRecordGroup>) -> Vec<EditorRecordGroup> {
    let mut ids = BTreeSet::new();
    let mut assigned_records = BTreeSet::new();
    groups
        .into_iter()
        .filter_map(|group| {
            let id = group.id.trim().to_string();
            if id.is_empty() || !ids.insert(id.clone()) {
                return None;
            }
            let name = group
                .name
                .trim()
                .chars()
                .take(MAX_VIEW_NAME_LEN)
                .collect::<String>();
            let mut group_records = BTreeSet::new();
            let records = group
                .records
                .into_iter()
                .filter(|coordinate| {
                    !assigned_records.contains(coordinate)
                        && group_records.insert(coordinate.clone())
                })
                .collect::<Vec<_>>();
            if records.len() < 2 {
                return None;
            }
            assigned_records.extend(records.iter().cloned());
            Some(EditorRecordGroup {
                id,
                name: if name.is_empty() {
                    "未命名分组".to_string()
                } else {
                    name
                },
                color: group
                    .color
                    .filter(|color| RECORD_GROUP_COLORS.contains(&color.as_str())),
                records,
            })
        })
        .collect()
}

/// Trim/dedupe an ordered list of field-like strings, preserving first-seen order.
fn sanitized_field_list(fields: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    fields
        .into_iter()
        .map(|field| field.trim().chars().take(MAX_FIELD_LEN).collect::<String>())
        .filter(|field| !field.is_empty() && seen.insert(field.clone()))
        .collect()
}

/// Sanitize a (filePath, actualType)'s custom view list.
pub(crate) fn sanitized_views(
    views: Vec<ViewConfig>,
    valid_group_ids: &BTreeSet<String>,
) -> Vec<ViewConfig> {
    let mut ids = BTreeSet::new();
    views
        .into_iter()
        .filter_map(|view| {
            let id = view.id.trim().to_string();
            if id.is_empty() || id.starts_with(RESERVED_VIEW_ID_PREFIX) || !ids.insert(id.clone()) {
                return None;
            }
            let name = view
                .name
                .trim()
                .chars()
                .take(MAX_VIEW_NAME_LEN)
                .collect::<String>();
            let group_filter = view
                .group_filter
                .filter(|group_id| valid_group_ids.contains(group_id));
            let sanitized = match view.kind {
                ViewKind::Table => ViewConfig {
                    id,
                    name: if name.is_empty() {
                        "未命名视图".to_string()
                    } else {
                        name
                    },
                    kind: ViewKind::Table,
                    group_filter,
                    columns: sanitized_field_list(view.columns),
                    column_widths: sanitized_column_widths(view.column_widths),
                    relations: Vec::new(),
                    fields: Vec::new(),
                },
                ViewKind::Graph => ViewConfig {
                    id,
                    name: if name.is_empty() {
                        "未命名视图".to_string()
                    } else {
                        name
                    },
                    kind: ViewKind::Graph,
                    group_filter,
                    columns: Vec::new(),
                    column_widths: BTreeMap::new(),
                    relations: sanitized_field_list(view.relations),
                    fields: sanitized_field_list(view.fields),
                },
            };
            Some(sanitized)
        })
        .collect()
}
