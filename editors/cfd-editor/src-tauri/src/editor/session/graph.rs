//! Build the BFS-bounded reference graph for the active file.
//!
//! Starts from every record hosted in the requested file, walks outgoing
//! refs up to `GRAPH_DEPTH` hops, and records target records as nodes
//! (collapsed past depth). Cross-file refs are kept because the editor
//! shows targets that live elsewhere as off-focus nodes the user can click
//! through.
use std::collections::{BTreeMap, HashMap, VecDeque};

use coflow_api::CfdDictKey as ApiCfdDictKey;
use coflow_data_model::{CfdRecord, CfdValue as DmCfdValue};

use crate::editor::convert::record_to_field_cells_for_session;
use crate::editor::types::{FieldCell, FieldValue, GraphData, GraphEdge, GraphNode};

use super::EditorSession;

const GRAPH_DEPTH: usize = 3;

pub(super) fn build_graph(session: &EditorSession, file_path: &str) -> GraphData {
    let mut nodes: BTreeMap<String, GraphNode> = BTreeMap::new();
    let mut edges: Vec<GraphEdge> = Vec::new();

    let starts: Vec<coflow_data_model::CfdRecordId> =
        session.engine.records.ids_in_file(file_path).to_vec();

    let mut queue: VecDeque<(coflow_data_model::CfdRecordId, usize)> = VecDeque::new();
    let mut depths: HashMap<coflow_data_model::CfdRecordId, usize> = HashMap::new();

    for id in &starts {
        queue.push_back((*id, 0));
        depths.insert(*id, 0);
    }

    while let Some((id, depth)) = queue.pop_front() {
        let Some(record) = session.engine.model.record(id) else {
            continue;
        };
        let host_file = session
            .engine
            .records
            .file_for_id(id)
            .unwrap_or_default()
            .to_string();
        let node_id = format!("{host_file}::{}", record.key);
        let in_focus = host_file == file_path;
        let is_collapsed = depth >= GRAPH_DEPTH;

        let fields = if is_collapsed {
            Vec::new()
        } else {
            let mut f = record_to_field_cells_for_session(
                record,
                &session.engine.model,
                &session.record_file_map(),
            );
            annotate_ref_files(&mut f, session);
            f
        };

        nodes.entry(node_id.clone()).or_insert_with(|| GraphNode {
            id: node_id.clone(),
            key: record.key.clone(),
            actual_type: record.actual_type.clone(),
            file_path: host_file.clone(),
            in_focus_file: in_focus,
            is_collapsed,
            fields,
        });

        if is_collapsed {
            continue;
        }

        let refs = collect_refs_in_record(record);
        for (path_str, target_type, target_key) in refs {
            let Some(target_id) = session
                .engine
                .records
                .id_for_coordinate(&target_type, &target_key)
            else {
                continue;
            };
            let Some(target_file) = session
                .engine
                .records
                .file_for_id(target_id)
                .map(str::to_string)
            else {
                continue;
            };
            let target_node_id = format!("{target_file}::{target_key}");
            edges.push(GraphEdge {
                source: node_id.clone(),
                target: target_node_id.clone(),
                field_path: path_str,
            });
            if !depths.contains_key(&target_id) {
                depths.insert(target_id, depth + 1);
                queue.push_back((target_id, depth + 1));
            }
        }
    }

    GraphData {
        nodes: nodes.into_values().collect(),
        edges,
    }
}

pub(super) fn annotate_ref_files(fields: &mut [FieldCell], session: &EditorSession) {
    for cell in fields {
        annotate_value(&mut cell.value, session);
    }
}

fn annotate_value(value: &mut FieldValue, session: &EditorSession) {
    match value {
        FieldValue::Ref {
            target_type,
            target_key,
            target_file,
            ..
        } => {
            *target_file = session
                .engine
                .records
                .file_for_coordinate(target_type, target_key)
                .map(str::to_string);
        }
        FieldValue::Object { fields, .. } => annotate_ref_files(fields, session),
        FieldValue::Array { items } => {
            for item in items {
                annotate_value(item, session);
            }
        }
        FieldValue::Dict { entries } => {
            for entry in entries {
                annotate_value(&mut entry.value, session);
            }
        }
        _ => {}
    }
}

fn collect_refs_in_record(record: &CfdRecord) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for (name, value) in &record.fields {
        match value {
            DmCfdValue::Ref {
                target_type,
                target_key,
            } => {
                out.push((name.clone(), target_type.clone(), target_key.clone()));
            }
            DmCfdValue::Array(items) => {
                for (i, item) in items.iter().enumerate() {
                    if let DmCfdValue::Ref {
                        target_type,
                        target_key,
                    } = item
                    {
                        out.push((
                            format!("{name}[{i}]"),
                            target_type.clone(),
                            target_key.clone(),
                        ));
                    }
                }
            }
            DmCfdValue::Dict(entries) => {
                for (k, v) in entries {
                    if let DmCfdValue::Ref {
                        target_type,
                        target_key,
                    } = v
                    {
                        let key_str = match k {
                            ApiCfdDictKey::String(s) => format!("\"{s}\""),
                            ApiCfdDictKey::Int(i) => i.to_string(),
                            ApiCfdDictKey::Enum(e) => e
                                .variant
                                .clone()
                                .unwrap_or_else(|| format!("{}({})", e.enum_name, e.value)),
                        };
                        out.push((
                            format!("{name}[{key_str}]"),
                            target_type.clone(),
                            target_key.clone(),
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    out
}
