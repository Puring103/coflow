//! Build the BFS-bounded reference graph for the active file.
//!
//! Starts from the active file/type, applies the same field/depth/limit
//! parameters the graph UI exposes, and only serializes the requested
//! subgraph. Cross-file refs are kept because the editor shows targets that
//! live elsewhere as off-focus nodes the user can click through.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use coflow_data_model::{CfdPath, CfdPathSegment, CfdRecordId, RefEdge};
use coflow_runtime::{format_field_path, RecordCoordinate};

use crate::editor::convert::{record_to_row, WireContext};
use crate::editor::types::{GraphData, GraphEdge, GraphNode, GraphQuery};

use super::EditorSession;

const GRAPH_DEPTH: usize = 3;
const GRAPH_NODE_LIMIT: usize = 1_000;

pub(super) fn build_graph(session: &EditorSession, query: &GraphQuery) -> GraphData {
    let file_path = query.file_path.as_str();
    let max_depth = query.depth.unwrap_or(GRAPH_DEPTH);
    let node_limit = query.limit.unwrap_or(GRAPH_NODE_LIMIT).max(1);
    let enabled_fields = query
        .enabled_fields
        .as_ref()
        .map(|fields| fields.iter().map(String::as_str).collect::<BTreeSet<_>>());
    let mut nodes: BTreeMap<NodeKey, GraphNode> = BTreeMap::new();
    let mut edges: Vec<GraphEdge> = Vec::new();

    let starts = start_records(
        session,
        file_path,
        query.active_type.as_deref(),
        enabled_fields.as_ref(),
    );
    let available_starts = start_records(session, file_path, query.active_type.as_deref(), None);
    let available_fields =
        collect_available_fields(session, &available_starts, max_depth, node_limit);
    let mut queue: VecDeque<(CfdRecordId, usize)> = VecDeque::new();
    let mut depths: HashMap<CfdRecordId, usize> = HashMap::new();
    for id in &starts {
        if depths.len() >= node_limit {
            break;
        }
        queue.push_back((*id, 0));
        depths.insert(*id, 0);
    }

    let ctx = WireContext::new(&session.engine);

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
        let is_collapsed = depth >= max_depth;

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

        for edge in session.engine.model.direct_ref_edges_from_host(id) {
            if !edge_enabled(edge, enabled_fields.as_ref()) {
                continue;
            }
            let Some(target_record) = session.engine.model.record(edge.target) else {
                continue;
            };
            if !depths.contains_key(&edge.target) && depths.len() >= node_limit {
                continue;
            }
            let target_coord =
                RecordCoordinate::new(target_record.actual_type(), target_record.key.clone());
            edges.push(GraphEdge {
                source: coordinate.clone(),
                target: target_coord,
                field_path: format_field_path(&edge.path),
            });
            if depth < max_depth && !depths.contains_key(&edge.target) {
                depths.insert(edge.target, depth + 1);
                queue.push_back((edge.target, depth + 1));
            }
        }
    }

    GraphData {
        nodes: nodes.into_values().collect(),
        edges,
        available_fields,
    }
}

fn start_records(
    session: &EditorSession,
    file_path: &str,
    active_type: Option<&str>,
    enabled_fields: Option<&BTreeSet<&str>>,
) -> Vec<CfdRecordId> {
    session
        .engine
        .records
        .ids_in_file(file_path)
        .iter()
        .copied()
        .filter(|id| {
            let Some(record) = session.engine.model.record(*id) else {
                return false;
            };
            active_type.is_none_or(|expected| record.actual_type() == expected)
                && session
                    .engine
                    .model
                    .direct_ref_edges_from_host(*id)
                    .any(|edge| edge_enabled(edge, enabled_fields))
        })
        .collect()
}

fn collect_available_fields(
    session: &EditorSession,
    starts: &[CfdRecordId],
    max_depth: usize,
    node_limit: usize,
) -> Vec<String> {
    let mut fields = BTreeSet::new();
    let mut queue: VecDeque<(CfdRecordId, usize)> = VecDeque::new();
    let mut depths: HashMap<CfdRecordId, usize> = HashMap::new();
    for id in starts {
        if depths.len() >= node_limit {
            break;
        }
        queue.push_back((*id, 0));
        depths.insert(*id, 0);
    }
    while let Some((id, depth)) = queue.pop_front() {
        if depth >= max_depth {
            continue;
        }
        for edge in session.engine.model.direct_ref_edges_from_host(id) {
            if let Some(field) = top_level_field(&edge.path) {
                fields.insert(field.to_string());
            }
            if depth < max_depth && !depths.contains_key(&edge.target) && depths.len() < node_limit
            {
                depths.insert(edge.target, depth + 1);
                queue.push_back((edge.target, depth + 1));
            }
        }
    }
    fields.into_iter().collect()
}

fn edge_enabled(edge: &RefEdge, enabled_fields: Option<&BTreeSet<&str>>) -> bool {
    let Some(enabled_fields) = enabled_fields else {
        return true;
    };
    top_level_field(&edge.path).is_some_and(|field| enabled_fields.contains(field))
}

fn top_level_field(path: &CfdPath) -> Option<&str> {
    path.segments.iter().find_map(|segment| match segment {
        CfdPathSegment::Field(name) => Some(name.as_str()),
        CfdPathSegment::Index(_) | CfdPathSegment::DictKey(_) => None,
    })
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
