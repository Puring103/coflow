//! 会话行构建：`FileRecords` 快照与记录排序/容器辅助函数。
//!
//! 纯派生逻辑，不触碰引擎写接口，便于 `mod.rs` 只保留会话生命周期。

use std::collections::BTreeMap;

use coflow_runtime::RecordOrigin;

use super::build::{session_capabilities_for_file, SessionSnapshotParts};
use super::{ColumnStats, EditorSession};
use crate::editor::convert::{record_view_to_row, WireContext};
use crate::editor::types::{
    EditorError, FileRecords, FileTypeOption, ProjectBootstrap, RecordColumn,
    DeletedRecordSnapshot,
};

/// 删除前快照：落盘前捕获 `(CfdRecord, display_path)`，undo 为尽力而为。
pub(crate) fn snapshot_record_before_delete(
    session: &EditorSession,
    coordinate: &coflow_runtime::RecordCoordinate,
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
        let mut row = record_view_to_row(&view, &ctx);
        row.container_index = *container_index;
        *container_index += 1;
        row_containers.push(container);
        for field in &row.fields {
            let index = column_index.get(&field.name).copied().unwrap_or_else(|| {
                let index = columns.len();
                columns.push((field.name.clone(), ColumnStats::default()));
                column_index.insert(field.name.clone(), index);
                index
            });
            let stats = &mut columns[index].1;
            stats
                .type_names
                .insert(row.coordinate.actual_type.to_string());
            let summary_len = row.field_summaries.get(&field.name).map_or(0, String::len);
            stats.max_summary_len = stats.max_summary_len.max(summary_len);
        }
        records.push(row);
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
    let type_names = session
        .file_type_names
        .get(file_path)
        .cloned()
        .unwrap_or_else(|| {
            queries
                .schema_type_names()
                .into_iter()
                .filter(|name| !queries.type_is_abstract(name))
                .collect()
        });
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
    coordinate: &coflow_runtime::RecordCoordinate,
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
    coordinate: &coflow_runtime::RecordCoordinate,
) -> Option<usize> {
    let file = session
        .queries()
        .file_for_record(&coordinate.actual_type, &coordinate.key)?;
    record_index_in_file(session, file, coordinate, false)
}

/// 同类型内的序号，用于跨文件转移的 `old_index`。
pub(crate) fn record_type_index(
    session: &EditorSession,
    coordinate: &coflow_runtime::RecordCoordinate,
) -> Option<usize> {
    let file = session
        .queries()
        .file_for_record(&coordinate.actual_type, &coordinate.key)?;
    record_index_in_file(session, file, coordinate, true)
}

fn record_index_in_file(
    session: &EditorSession,
    file: &str,
    coordinate: &coflow_runtime::RecordCoordinate,
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

fn first_source_file(nodes: &[coflow_runtime::FileTreeNode]) -> Option<String> {
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
        project_root: coflow_runtime::path_to_slash(&session.project_root),
        first_source_file: first_source_file(&snapshot.file_tree),
        file_tree: snapshot.file_tree,
        file_types,
        dimensions,
        diagnostics: session.diagnostics.to_wire(),
    }
}

fn snapshot_file_types(session: &EditorSession) -> BTreeMap<String, Vec<FileTypeOption>> {
    session
        .queries()
        .source_files()
        .map(|file_path| {
            // 类型计数已在 build_session 的单次遍历中算好，这里直接读取。
            let counts = session.file_type_counts.get(file_path);
            let options = session
                .file_type_names
                .get(file_path)
                .cloned()
                .unwrap_or_else(|| {
                    counts
                        .map(|by_type| by_type.keys().cloned().collect())
                        .unwrap_or_default()
                })
                .into_iter()
                .map(|name| FileTypeOption {
                    display_name: session.type_display_name(file_path, &name),
                    record_count: counts
                        .and_then(|by_type| by_type.get(&name))
                        .copied()
                        .unwrap_or_default(),
                    is_singleton: session.queries().type_is_singleton(&name),
                    name,
                })
                .collect();
            (file_path.to_string(), options)
        })
        .collect()
}
