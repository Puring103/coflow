//! Build the BFS-bounded reference graph for the active file.
//!
//! Starts from every record hosted in the requested file, walks outgoing
//! refs up to `GRAPH_DEPTH` hops, and records target records as nodes
//! (collapsed past depth). Cross-file refs are kept because the editor
//! shows targets that live elsewhere as off-focus nodes the user can click
//! through.

use std::collections::{BTreeMap, HashMap, VecDeque};

use coflow_data_model::{CfdDictKey, CfdRecord, CfdRecordId, CfdValue};
use coflow_engine::RecordCoordinate;

use crate::editor::convert::{record_to_row, WireContext};
use crate::editor::types::{GraphData, GraphEdge, GraphNode};

use super::EditorSession;

const GRAPH_DEPTH: usize = 3;

pub(super) fn build_graph(session: &EditorSession, file_path: &str) -> GraphData {
    let mut nodes: BTreeMap<NodeKey, GraphNode> = BTreeMap::new();
    let mut edges: Vec<GraphEdge> = Vec::new();

    let starts: Vec<CfdRecordId> = session.engine.records.ids_in_file(file_path).to_vec();
    let mut queue: VecDeque<(CfdRecordId, usize)> = VecDeque::new();
    let mut depths: HashMap<CfdRecordId, usize> = HashMap::new();
    for id in &starts {
        queue.push_back((*id, 0));
        depths.insert(*id, 0);
    }

    let record_file_map = session.record_file_map();
    let ctx = WireContext {
        session: &session.engine,
        key_to_file: &record_file_map,
    };

    while let Some((id, depth)) = queue.pop_front() {
        let Some(record) = session.engine.model.record(id) else {
            continue;
        };
        let coordinate = RecordCoordinate::new(record.actual_type.clone(), record.key.clone());
        let host_file = session
            .engine
            .records
            .file_for_id(id)
            .unwrap_or_default()
            .to_string();
        let node_key = NodeKey::from_coordinate(&coordinate);
        let in_focus = host_file == file_path;
        let is_collapsed = depth >= GRAPH_DEPTH;

        let fields = if is_collapsed {
            Vec::new()
        } else {
            record_to_row(record, &host_file, &ctx).fields
        };

        nodes.entry(node_key.clone()).or_insert_with(|| GraphNode {
            coordinate: coordinate.clone(),
            file_path: host_file.clone(),
            in_focus_file: in_focus,
            is_collapsed,
            fields,
        });

        if is_collapsed {
            continue;
        }

        for (path_str, target_type, target_key) in collect_refs_in_record(record) {
            let Some(target_id) = session
                .engine
                .records
                .id_for_coordinate(&target_type, &target_key)
            else {
                continue;
            };
            let target_coord = RecordCoordinate::new(target_type.clone(), target_key.clone());
            edges.push(GraphEdge {
                source: coordinate.clone(),
                target: target_coord,
                field_path: path_str,
            });
            if let std::collections::hash_map::Entry::Vacant(entry) = depths.entry(target_id) {
                entry.insert(depth + 1);
                queue.push_back((target_id, depth + 1));
            }
        }
    }

    GraphData {
        nodes: nodes.into_values().collect(),
        edges,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct NodeKey {
    actual_type: String,
    key: String,
}

impl NodeKey {
    fn from_coordinate(c: &RecordCoordinate) -> Self {
        Self {
            actual_type: c.actual_type.clone(),
            key: c.key.clone(),
        }
    }
}

fn collect_refs_in_record(record: &CfdRecord) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for (name, value) in &record.fields {
        match value {
            CfdValue::Ref {
                target_type,
                target_key,
            } => {
                out.push((name.clone(), target_type.clone(), target_key.clone()));
            }
            CfdValue::Array(items) => {
                for (i, item) in items.iter().enumerate() {
                    if let CfdValue::Ref {
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
            CfdValue::Dict(entries) => {
                for (k, v) in entries {
                    if let CfdValue::Ref {
                        target_type,
                        target_key,
                    } = v
                    {
                        let key_str = match k {
                            CfdDictKey::String(s) => format!("\"{s}\""),
                            CfdDictKey::Int(i) => i.to_string(),
                            CfdDictKey::Enum(e) => e
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
