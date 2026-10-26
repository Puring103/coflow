//! 会话行构建：`FileRecords` 快照与记录排序/容器辅助函数。
//!
//! 纯派生逻辑，不触碰引擎写接口，便于 `mod.rs` 只保留会话生命周期。

use std::collections::{BTreeMap, BTreeSet};

use coflow_runtime::RecordOrigin;

use super::build::{session_capabilities_for_file, SessionSnapshotParts};
use super::{ColumnStats, EditorSession};
use crate::editor::convert::{record_view_to_row, WireContext};
use crate::editor::types::{
    DeletedRecordSnapshot, EditorError, FileRecords, FileTypeOption, ProjectBootstrap, RecordColumn,
};

/// 删除前快照：落盘前捕获 `(CfdRecord, display_path)`，undo 为尽力而为。
pub(crate) fn snapshot_record_before_delete(
    session: &EditorSession,
    coordinate: &coflow_runtime::RecordCoordinate,
) -> Option<DeletedRecordSnapshot> {
    session
        .queries()
        .record_view(&coordinate.actual_type, &coordinate.id)
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
            let metrics = stats
                .content_metrics
                .entry(row.coordinate.actual_type.to_string())
                .or_default();
            if let Some(annotation) = &field.annotation {
                if metrics.declared_type.is_none() {
                    metrics.declared_type.clone_from(&annotation.declared_type);
                }
                if metrics.label.is_none() && metrics.description.is_none() {
                    metrics.label.clone_from(&annotation.label);
                    metrics.description.clone_from(&annotation.description);
                }
                if metrics.dropdown_kind.is_none() {
                    metrics.dropdown_kind = if annotation.ref_target_type.is_some() {
                        Some("ref".into())
                    } else if annotation.enum_type.is_some() {
                        Some("enum".into())
                    } else {
                        None
                    };
                }
            }
            if metrics.declared_type.is_none() {
                metrics.declared_type = Some(inferred_display_type(&field.value));
            }
            if metrics.dropdown_kind.is_none() {
                metrics.dropdown_kind = match &field.value {
                    coflow_runtime::CfdValue::Ref(_) => Some("ref".into()),
                    coflow_runtime::CfdValue::Enum(_) => Some("enum".into()),
                    coflow_runtime::CfdValue::Bool(_) => Some("bool".into()),
                    _ => None,
                };
            }
            // 与前端原估宽规则一致，内容达到上限后停止扫描长字符串。
            if metrics.max_width < 600.0 {
                let summary = row
                    .field_summaries
                    .get(&field.name)
                    .map_or("", String::as_str);
                metrics.max_width = metrics.max_width.max(summary_width(summary));
            }
            metrics.has_complex_value |= matches!(
                field.value,
                coflow_runtime::CfdValue::Object(_)
                    | coflow_runtime::CfdValue::Array(_)
                    | coflow_runtime::CfdValue::Dict(_)
            );
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
            content_metrics: stats.content_metrics,
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
                .filter(|name| !queries.type_is_abstract(name) && !queries.type_is_data(name))
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

// 无 schema 的额外字段仍提供与值一致的显示类型；Option 只剥去实际包装。
fn inferred_display_type(value: &coflow_runtime::CfdValue) -> String {
    use coflow_runtime::CfdValue;
    match value {
        CfdValue::OptionSome(inner) => inferred_display_type(inner),
        CfdValue::OptionNone => "None".into(),
        CfdValue::Bool(_) => "bool".into(),
        CfdValue::Int(_) => "int".into(),
        CfdValue::Float(_) => "float".into(),
        CfdValue::String(_) => "string".into(),
        CfdValue::FormattedString(_) => "formatted_string".into(),
        CfdValue::Enum(value) => value.enum_name.to_string(),
        CfdValue::Object(value) => value.actual_type().to_string(),
        CfdValue::Ref(_) => "ref".into(),
        CfdValue::Array(_) => "array".into(),
        CfdValue::Dict(_) => "dict".into(),
    }
}

/// 排序/移动类 mutation 需要先定位记录所在文件。
pub(crate) fn reorder_file_path(
    session: &EditorSession,
    coordinate: &coflow_runtime::RecordCoordinate,
) -> Result<String, EditorError> {
    session
        .queries()
        .file_for_record(&coordinate.actual_type, &coordinate.id)
        .map(str::to_string)
        .ok_or_else(|| {
            EditorError::not_found(format!(
                "record `{}.{}` not found",
                coordinate.actual_type, coordinate.id
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
        .file_for_record(&coordinate.actual_type, &coordinate.id)?;
    record_index_in_file(session, file, coordinate, false)
}

/// 同类型内的序号，用于跨文件转移的 `old_index`。
pub(crate) fn record_type_index(
    session: &EditorSession,
    coordinate: &coflow_runtime::RecordCoordinate,
) -> Option<usize> {
    let file = session
        .queries()
        .file_for_record(&coordinate.actual_type, &coordinate.id)?;
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
    // 初始工作区入口必须出现在类型或数据树中，纯覆盖通过维度树导航。
    nodes.iter().find_map(|node| {
        if node.is_dir {
            first_source_file(&node.children)
        } else if node.in_sources && !node.dimension_only {
            Some(node.path.clone())
        } else {
            None
        }
    })
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
    let queries = session.queries();
    // 维度导航使用基础记录的业务来源，不让维度辅助记录进入可编辑类型列表。
    let mut fields_by_file =
        BTreeMap::<String, BTreeMap<String, BTreeMap<String, BTreeSet<String>>>>::new();
    for dimension in queries.dimensions() {
        {
            for field in &dimension.fields {
                for target in queries.ref_targets(&field.source_type) {
                    fields_by_file
                        .entry(target.file_path)
                        .or_default()
                        .entry(target.coordinate.actual_type.to_string())
                        .or_default()
                        .entry(dimension.name.clone())
                        .or_default()
                        .insert(field.source_field.clone());
                }
            }
        }
    }
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
                    dimension_fields: fields_by_file
                        .get(file_path)
                        .and_then(|types| types.get(&name))
                        .map(|dimensions| {
                            dimensions
                                .iter()
                                .map(|(dimension, fields)| {
                                    (dimension.clone(), fields.iter().cloned().collect())
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                    name,
                })
                .collect();
            (file_path.to_string(), options)
        })
        .collect()
}

/// 14px 字体下的确定性估宽；精确溢出判断仍由浏览器负责。
fn summary_width(text: &str) -> f64 {
    let mut width: f64 = 0.0;
    for ch in text.chars() {
        let wide = matches!(ch as u32, 0x1100..=0x115F | 0x2E80..=0x9FFF | 0xA960..=0xA97F | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6 | 0x20000..=0x3FFFD);
        width += if wide { 15.4 } else { 7.7 };
        if width >= 600.0 {
            return 600.0;
        }
    }
    width
}

#[cfg(test)]
mod width_tests {
    use super::summary_width;
    #[test]
    fn width_preserves_unicode_and_caps_long_text() {
        assert!((summary_width("A中😀") - 30.8).abs() < 0.0001);
        assert_eq!(summary_width(&"中".repeat(100_000)), 600.0);
        assert_eq!(summary_width(""), 0.0);
    }
}
