//! Build the BFS-bounded reference graph for the active file.
//!
//! Starts from every record hosted in the requested file, walks outgoing
//! refs up to `GRAPH_DEPTH` hops, and records target records as nodes
//! (collapsed past depth). Cross-file refs are kept because the editor
//! shows targets that live elsewhere as off-focus nodes the user can click
//! through.

use std::collections::{BTreeMap, HashMap, VecDeque};

use coflow_data_model::{CfdPath, CfdPathSegment, CfdRecordId};
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

    let ctx = WireContext {
        session: &session.engine,
    };

    while let Some((id, depth)) = queue.pop_front() {
        let Some(record) = session.engine.model.record(id) else {
            continue;
        };
        let coordinate = RecordCoordinate::new(record.actual_type(), record.key.clone());
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

        for edge in session.engine.model.ref_edges_from_host(id) {
            let Some(target_record) = session.engine.model.record(edge.target) else {
                continue;
            };
            let target_coord =
                RecordCoordinate::new(target_record.actual_type(), target_record.key.clone());
            edges.push(GraphEdge {
                source: coordinate.clone(),
                target: target_coord,
                field_path: format_path(&edge.path),
            });
            if let std::collections::hash_map::Entry::Vacant(entry) = depths.entry(edge.target) {
                entry.insert(depth + 1);
                queue.push_back((edge.target, depth + 1));
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

fn format_path(path: &CfdPath) -> String {
    let mut out = String::new();
    for segment in &path.segments {
        match segment {
            CfdPathSegment::Field(name) => {
                if !out.is_empty() {
                    out.push('.');
                }
                out.push_str(name);
            }
            CfdPathSegment::Index(index) => {
                out.push('[');
                out.push_str(&index.to_string());
                out.push(']');
            }
            CfdPathSegment::DictKey(key) => {
                out.push('[');
                out.push_str(key);
                out.push(']');
            }
        }
    }
    out
}
