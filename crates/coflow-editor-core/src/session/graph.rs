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

use crate::convert::record_to_field_cells_for_session;
use crate::types::{FieldCell, FieldValue, GraphData, GraphEdge, GraphNode};

use super::EditorSession;

const GRAPH_DEPTH: usize = 3;

pub(super) fn build_graph(session: &EditorSession, file_path: &str) -> GraphData {
    let mut nodes: BTreeMap<String, GraphNode> = BTreeMap::new();
    let mut edges: Vec<GraphEdge> = Vec::new();

    let starts: Vec<String> = session
        .file_to_keys
        .get(file_path)
        .cloned()
        .unwrap_or_default();

    let mut queue: VecDeque<(String, usize)> = VecDeque::new();
    let mut depths: HashMap<String, usize> = HashMap::new();

    for key in &starts {
        queue.push_back((key.clone(), 0));
        depths.insert(key.clone(), 0);
    }

    while let Some((key, depth)) = queue.pop_front() {
        let Some(record) = session
            .model
            .records()
            .find(|(_, r)| r.key == key)
            .map(|(_, r)| r)
        else {
            continue;
        };
        let host_file = session.key_to_file.get(&key).cloned().unwrap_or_default();
        let id = format!("{host_file}::{key}");
        let in_focus = host_file == file_path;
        let is_collapsed = depth >= GRAPH_DEPTH;

        let fields = if is_collapsed {
            Vec::new()
        } else {
            let mut f =
                record_to_field_cells_for_session(record, &session.model, &session.key_to_file);
            annotate_ref_files(&mut f, session);
            f
        };

        nodes.entry(id.clone()).or_insert_with(|| GraphNode {
            id: id.clone(),
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
        for (path_str, target_key) in refs {
            let Some(target_file) = session.key_to_file.get(&target_key).cloned() else {
                continue;
            };
            let target_id = format!("{target_file}::{target_key}");
            edges.push(GraphEdge {
                source: id.clone(),
                target: target_id.clone(),
                field_path: path_str,
            });
            if !depths.contains_key(&target_key) {
                depths.insert(target_key.clone(), depth + 1);
                queue.push_back((target_key, depth + 1));
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
            target_key,
            target_file,
            ..
        } => {
            *target_file = session.key_to_file.get(target_key).cloned();
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

fn collect_refs_in_record(record: &CfdRecord) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (name, value) in &record.fields {
        match value {
            DmCfdValue::Ref { key, .. } => out.push((name.clone(), key.clone())),
            DmCfdValue::Array(items) => {
                for (i, item) in items.iter().enumerate() {
                    if let DmCfdValue::Ref { key, .. } = item {
                        out.push((format!("{name}[{i}]"), key.clone()));
                    }
                }
            }
            DmCfdValue::Dict(entries) => {
                for (k, v) in entries {
                    if let DmCfdValue::Ref { key, .. } = v {
                        let key_str = match k {
                            ApiCfdDictKey::String(s) => format!("\"{s}\""),
                            ApiCfdDictKey::Int(i) => i.to_string(),
                            ApiCfdDictKey::Enum(e) => e
                                .variant
                                .clone()
                                .unwrap_or_else(|| format!("{}({})", e.enum_name, e.value)),
                        };
                        out.push((format!("{name}[{key_str}]"), key.clone()));
                    }
                }
            }
            _ => {}
        }
    }
    out
}
