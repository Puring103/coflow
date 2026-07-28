//! Build the BFS-bounded reference graph for the active file.
//!
//! Starts from the active file/type, applies the same field/depth/limit
//! parameters the graph UI exposes, and only serializes the requested
//! subgraph. Cross-file refs are kept because the editor shows targets that
//! live elsewhere as off-focus nodes the user can click through.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use coflow_data_model::{CfdPath, CfdPathSegment};
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
    let mut nodes: BTreeMap<NodeKey, GraphNode> = BTreeMap::new();
    let mut edges: Vec<GraphEdge> = Vec::new();

    let starts = start_records(session, file_path);
    let available_fields = collect_available_fields(session, &starts, max_depth, node_limit);
    let mut queue: VecDeque<(RecordCoordinate, usize)> = VecDeque::new();
    let mut depths: HashMap<RecordCoordinate, usize> = HashMap::new();
    for coordinate in &starts {
        if depths.len() >= node_limit {
            break;
        }
        queue.push_back((coordinate.clone(), 0));
        depths.insert(coordinate.clone(), 0);
    }

    let queries = session.queries();
    let ctx = WireContext::new(queries, &session.diagnostics);

    while let Some((coordinate, depth)) = queue.pop_front() {
        let Some(view) = queries.record_view(&coordinate.actual_type, &coordinate.key) else {
            continue;
        };
        let host_file = view.display_path.to_string();
        let node_key = NodeKey::from_coordinate(&coordinate);
        let in_focus = host_file == file_path;
        let is_collapsed = depth >= max_depth;

        let row = record_to_row(view.record, &host_file, &ctx);
        let fields = if is_collapsed { Vec::new() } else { row.fields };

        nodes.entry(node_key.clone()).or_insert_with(|| GraphNode {
            coordinate: coordinate.clone(),
            file_path: host_file.clone(),
            in_focus_file: in_focus,
            is_collapsed,
            fields,
            field_diagnostics: row.field_diagnostics,
            diagnostic_severity: row.diagnostic_severity,
        });

        if is_collapsed {
            continue;
        }

        for edge in queries.record_references(&coordinate) {
            if !depths.contains_key(&edge.target) && depths.len() >= node_limit {
                continue;
            }
            edges.push(GraphEdge {
                source: coordinate.clone(),
                target: edge.target.clone(),
                field_path: format_field_path(&edge.path),
            });
            if depth < max_depth && !depths.contains_key(&edge.target) {
                depths.insert(edge.target.clone(), depth + 1);
                queue.push_back((edge.target, depth + 1));
            }
        }
    }

    GraphData {
        revision: session.revisions.current(),
        nodes: nodes.into_values().collect(),
        edges,
        available_fields,
    }
}

fn start_records(session: &EditorSession, file_path: &str) -> Vec<RecordCoordinate> {
    session
        .queries()
        .record_views_in_file(file_path)
        .filter_map(|view| {
            let coordinate = view.coordinate;
            (!session.queries().record_references(&coordinate).is_empty()).then_some(coordinate)
        })
        .collect()
}

fn collect_available_fields(
    session: &EditorSession,
    starts: &[RecordCoordinate],
    max_depth: usize,
    node_limit: usize,
) -> Vec<String> {
    let mut fields = BTreeSet::new();
    let mut queue: VecDeque<(RecordCoordinate, usize)> = VecDeque::new();
    let mut depths: HashMap<RecordCoordinate, usize> = HashMap::new();
    for coordinate in starts {
        if depths.len() >= node_limit {
            break;
        }
        queue.push_back((coordinate.clone(), 0));
        depths.insert(coordinate.clone(), 0);
    }
    while let Some((coordinate, depth)) = queue.pop_front() {
        if depth >= max_depth {
            continue;
        }
        for edge in session.queries().record_references(&coordinate) {
            if let Some(field) = top_level_field(&edge.path) {
                fields.insert(field.to_string());
            }
            if depth < max_depth && !depths.contains_key(&edge.target) && depths.len() < node_limit
            {
                depths.insert(edge.target.clone(), depth + 1);
                queue.push_back((edge.target, depth + 1));
            }
        }
    }
    fields.into_iter().collect()
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
            actual_type: c.actual_type.to_string(),
            key: c.key.to_string(),
        }
    }
}
