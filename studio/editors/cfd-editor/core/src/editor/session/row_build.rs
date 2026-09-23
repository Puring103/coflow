//! 会话行构建：`FileRecords` 快照与记录排序/容器辅助函数。
//!
//! 纯派生逻辑，不触碰引擎写接口，便于 `mod.rs` 只保留会话生命周期。

use std::collections::BTreeMap;

use coflow_project::RecordOrigin;

use super::build::{session_capabilities_for_file, SessionSnapshotParts};
use super::{ColumnStats, EditorSession};
use crate::editor::convert::{record_summaries, record_view_to_row, WireContext};
use crate::editor::types::{
    DeletedRecordSnapshot, EditorError, FileRecords, FileTypeOption, ProjectBootstrap, RecordColumn,
};

/// 删除前快照：落盘前捕获 `(CfdRecord, display_path)`，undo 为尽力而为。
pub(crate) fn snapshot_record_before_delete(
    session: &EditorSession,
    coordinate: &coflow_project::RecordCoordinate,
) -> Option<DeletedRecordSnapshot> {
    session
        .queries()
        .record_view(&coordinate.actual_type, &coordinate.key)
        .map(|view| DeletedRecordSnapshot {
            record: view.record.clone(),
            display_path: view.display_path.to_string(),
        })
}

/// 单文件的全量行快照：列统计在单次遍历中算好，避免前端二次聚合。
pub(crate) fn file_records_for_session(session: &EditorSession, file_path: &str) -> FileRecords {
    file_records_selection(session, file_path, None)
}

pub(crate) fn file_records_selection(
    session: &EditorSession,
    file_path: &str,
    selected: Option<&std::collections::BTreeSet<&coflow_project::RecordCoordinate>>,
) -> FileRecords {
    let queries = session.queries();
    let ctx = WireContext::new(queries, &session.diagnostics, &session.shape_cache);
    let mut records = Vec::new();
    let mut columns = Vec::<(String, ColumnStats)>::new();
    let mut column_index = BTreeMap::<String, usize>::new();
    let mut container_counts = BTreeMap::<String, usize>::new();
    let mut row_containers = Vec::new();
    for view in queries.record_views_in_file(file_path) {
        let container = record_container_key(view.origin);
        let container_index = container_counts.entry(container.clone()).or_default();
        let mut row = selected
            .is_none_or(|set| set.contains(&view.coordinate))
            .then(|| record_view_to_row(&view, &ctx));
        if let Some(row) = &mut row {
            row.container_index = *container_index;
            row_containers.push(container);
        }
        *container_index += 1;
        let mut add_summary = |name: &str, summary: &str| {
            let index = column_index.get(name).copied().unwrap_or_else(|| {
                let index = columns.len();
                columns.push((name.to_string(), ColumnStats::default()));
                column_index.insert(name.to_string(), index);
                index
            });
            let stats = &mut columns[index].1;
            stats
                .type_names
                .insert(view.coordinate.actual_type.to_string());
            stats.max_summary_len = stats.max_summary_len.max(summary.len());
        };
        // 已转换的行直接借用摘要，只有未选中的行需要单独生成列统计摘要。
        if let Some(row) = &row {
            for field in &row.fields {
                add_summary(
                    &field.name,
                    row.field_summaries
                        .get(&field.name)
                        .map(String::as_str)
                        .unwrap_or_default(),
                );
            }
        } else {
            for (name, summary) in record_summaries(view.record, &ctx) {
                add_summary(&name, &summary);
            }
        }
        if let Some(row) = row {
            records.push(row);
        }
    }
    for (row, container) in records.iter_mut().zip(row_containers) {
        row.container_size = container_counts.get(&container).copied().unwrap_or(1);
    }
    let columns = columns
        .into_iter()
        .map(|(name, stats)| RecordColumn {
            name,
            type_names: stats.type_names.into_iter().collect(),
            max_summary_len: stats.max_summary_len,
        })
        .collect();
    let type_names = session.schema_type_names.clone();
    FileRecords {
        revision: session.revisions.current(),
        file_path: file_path.to_string(),
        type_names,
        columns,
        records,
        capabilities: session_capabilities_for_file(session, file_path),
    }
}

/// 排序/移动类 mutation 需要先定位记录所在文件。
pub(crate) fn reorder_file_path(
    session: &EditorSession,
    coordinate: &coflow_project::RecordCoordinate,
) -> Result<String, EditorError> {
    session
        .queries()
        .file_for_record(&coordinate.actual_type, &coordinate.key)
        .map(str::to_string)
        .ok_or_else(|| {
            EditorError::not_found(format!(
                "record `{}.{}` not found",
                coordinate.actual_type, coordinate.key
            ))
        })
}

/// 同容器（物理文件）内的序号，用于拖拽排序的 `old_index`。
pub(crate) fn record_container_index(
    session: &EditorSession,
    coordinate: &coflow_project::RecordCoordinate,
) -> Option<usize> {
    let file = session
        .queries()
        .file_for_record(&coordinate.actual_type, &coordinate.key)?;
    record_index_in_file(session, file, coordinate, false)
}

/// 同类型内的序号，用于跨文件转移的 `old_index`。
pub(crate) fn record_type_index(
    session: &EditorSession,
    coordinate: &coflow_project::RecordCoordinate,
) -> Option<usize> {
    let file = session
        .queries()
        .file_for_record(&coordinate.actual_type, &coordinate.key)?;
    record_index_in_file(session, file, coordinate, true)
}

fn record_index_in_file(
    session: &EditorSession,
    file: &str,
    coordinate: &coflow_project::RecordCoordinate,
    same_type: bool,
) -> Option<usize> {
    let mut index = 0;
    for view in session.queries().record_views_in_file(file) {
        // 查询已经限定文件，同容器比较无需再构造临时字符串。
        if same_type && view.coordinate.actual_type != coordinate.actual_type {
            continue;
        }
        if view.coordinate == *coordinate {
            return Some(index);
        }
        index += 1;
    }
    None
}

fn record_container_key(origin: &RecordOrigin) -> String {
    match origin {
        RecordOrigin::File { path, .. } => format!("file:{}", path.display()),
        RecordOrigin::None => "none".to_string(),
    }
}

fn first_source_file(nodes: &[coflow_project::FileTreeNode]) -> Option<String> {
    for node in nodes {
        if let Some(path) = node.first_source_descendant.clone() {
            return Some(path);
        }
    }
    None
}

/// 会话引导快照：文件树/revision/维度元数据属于同一原子快照。
pub(crate) fn project_bootstrap(
    session_id: u32,
    session: &EditorSession,
    snapshot: SessionSnapshotParts,
) -> ProjectBootstrap {
    let file_types = snapshot_file_types(session);
    // 维度元数据与文件树、revision 属于同一份原子快照，避免前端二次读取时跨版本。
    let dimensions = session.queries().dimensions();
    ProjectBootstrap {
        session_id,
        revision: session.revisions.current(),
        schema_revision: session.schema_revision,
        project_root: coflow_project::path_to_slash(&session.project_root),
        first_source_file: first_source_file(&snapshot.file_tree),
        file_tree: snapshot.file_tree,
        file_types,
        dimensions,
        diagnostics: session.diagnostics.to_wire(),
    }
}

fn snapshot_file_types(session: &EditorSession) -> BTreeMap<String, Vec<FileTypeOption>> {
    let queries = session.queries();
    // 先按实际记录与维度字段构建索引，避免每个文件/类型重复扫描继承关系。
    let mut fields_by_file = BTreeMap::<String, BTreeMap<String, BTreeMap<String, std::collections::BTreeSet<String>>>>::new();
    for dimension in queries.dimensions() {
        if let Some((_, fields)) = queries.dimension_fields(&dimension.name) {
            for field in fields {
                for target in queries.ref_targets(&field.source_type) {
                    if queries.record_view(&target.coordinate.actual_type, &target.coordinate.key)
                        .and_then(|view| view.record.field(&field.source_field).cloned()).is_none() {
                        continue;
                    }
                    fields_by_file.entry(target.file_path).or_default()
                        .entry(target.coordinate.actual_type.to_string()).or_default()
                        .entry(dimension.name.clone()).or_default()
                        .insert(field.source_field.clone());
                }
            }
        }
    }
    queries.source_files().map(|file_path| {
        let counts = session.file_type_counts.get(file_path);
        let options = session.schema_type_names.iter().cloned().map(|name| {
            let dimension_fields = fields_by_file.get(file_path).and_then(|types| types.get(&name))
                .map(|dimensions| dimensions.iter().map(|(dimension, fields)|
                    (dimension.clone(), fields.iter().cloned().collect())).collect())
                .unwrap_or_default();
            FileTypeOption {
                display_name: name.clone(),
                record_count: counts.and_then(|by_type| by_type.get(&name)).copied().unwrap_or_default(),
                is_singleton: queries.type_is_singleton(&name),
                dimension_fields,
                name,
            }
        }).collect();
        (file_path.to_string(), options)
    }).collect()
}
