use super::{
    diagnostics::CommandLocation, query::paginate, Arguments, Failure, RecordTarget, Result,
};
use crate::ReadOnlyProjectSession;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Debug, Serialize, serde::Deserialize, JsonSchema)]
pub struct ReferenceDimension {
    pub dimension: String,
    pub variant: String,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ReferencePathSegment {
    Field(String),
    Index(usize),
    DictKey(String),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ReferencePath {
    pub segments: Vec<ReferencePathSegment>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ReferenceEdge {
    pub host: RecordTarget,
    pub target: RecordTarget,
    pub path: ReferencePath,
    pub dimension: Option<ReferenceDimension>,
    pub location: Option<CommandLocation>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ReferenceQuery {
    pub items: Vec<ReferenceEdge>,
    pub total: usize,
    pub truncated: bool,
    pub next_cursor: Option<String>,
    pub depth_truncated: bool,
}

pub fn edges(session: &ReadOnlyProjectSession) -> Vec<ReferenceEdge> {
    let mut result = Vec::new();
    for (_, record) in session.model().records() {
        for edge in session.queries().record_references(&record.coordinate()) {
            // 维度引用属于覆盖值的源码，不能使用基础记录的文件位置。
            let origin = match &edge.dimension {
                None => Some(&record.origin),
                Some(dimension) => match edge.path.segments.first() {
                    Some(crate::CfdPathSegment::Field(field)) => record
                        .dimension_fields
                        .iter()
                        .find(|(name, values)| {
                            name.as_str() == field && values.dimension == dimension.dimension
                        })
                        .and_then(|(_, values)| values.variants.get(&dimension.variant))
                        .map(|value| &value.origin),
                    _ => None,
                },
            };
            let location = origin
                .and_then(|origin| origin.location_for_path(&edge.path))
                .map(|source| {
                    CommandLocation::from_source(
                        &source.into(),
                        Some(session.session.project.root_dir()),
                    )
                });
            result.push(ReferenceEdge {
                host: RecordTarget {
                    actual_type: record.actual_type().to_string(),
                    id: record.id().to_string(),
                },
                target: RecordTarget {
                    actual_type: edge.target.actual_type.to_string(),
                    id: edge.target.id.to_string(),
                },
                // 公共路径契约独立于数据模型内部类型，保留字典键的 CFD 表达式。
                path: ReferencePath {
                    segments: edge
                        .path
                        .segments
                        .into_iter()
                        .map(|segment| match segment {
                            crate::CfdPathSegment::Field(name) => ReferencePathSegment::Field(name),
                            crate::CfdPathSegment::Index(index) => {
                                ReferencePathSegment::Index(index)
                            }
                            crate::CfdPathSegment::DictKey(key) => {
                                ReferencePathSegment::DictKey(key)
                            }
                        })
                        .collect(),
                },
                dimension: edge.dimension.map(|d| ReferenceDimension {
                    dimension: d.dimension.to_string(),
                    variant: d.variant.to_string(),
                }),
                location,
            });
        }
    }
    result
}

pub fn query(session: &ReadOnlyProjectSession, args: &Arguments) -> Result<Value> {
    let ty = args.require("type")?;
    let key = args.require("id")?;
    let record = session
        .queries()
        .record_view(ty, key)
        .ok_or_else(|| Failure::missing(format!("record `{ty}::{key}` does not exist")))?;
    let direction = args.get("direction").unwrap_or("both");
    if !["incoming", "outgoing", "both"].contains(&direction) {
        return Err(Failure::request(
            "direction must be incoming, outgoing or both",
        ));
    }
    let depth = args.number("depth", 1)?;
    if depth == 0 || depth > 64 {
        return Err(Failure::request("reference depth must be in 1..=64"));
    }
    let all = edges(session);
    let mut frontier = BTreeSet::from([(
        record.coordinate.actual_type.to_string(),
        record.coordinate.id.to_string(),
    )]);
    let mut seen = frontier.clone();
    let mut included = BTreeSet::new();
    for _ in 0..depth {
        let mut next = BTreeSet::new();
        for (index, edge) in all.iter().enumerate() {
            let host = pair(&edge.host);
            let target = pair(&edge.target);
            let outgoing = direction != "incoming" && frontier.contains(&host);
            let incoming = direction != "outgoing" && frontier.contains(&target);
            if outgoing {
                included.insert(index);
                if seen.insert(target.clone()) {
                    next.insert(target);
                }
            }
            if incoming {
                included.insert(index);
                if seen.insert(host.clone()) {
                    next.insert(host);
                }
            }
        }
        frontier = next;
    }
    let depth_truncated = all.iter().enumerate().any(|(i, edge)| {
        !included.contains(&i)
            && (direction != "incoming" && frontier.contains(&pair(&edge.host))
                || direction != "outgoing" && frontier.contains(&pair(&edge.target)))
    });
    let items = all
        .into_iter()
        .enumerate()
        .filter_map(|(i, v)| included.contains(&i).then_some(v))
        .collect();
    let page = paginate(items, &args.selector()?)?;
    super::json(ReferenceQuery {
        items: page.items,
        total: page.total,
        truncated: page.truncated,
        next_cursor: page.next_cursor,
        depth_truncated,
    })
}

pub fn pair(value: &RecordTarget) -> (String, String) {
    (value.actual_type.clone(), value.id.clone())
}
