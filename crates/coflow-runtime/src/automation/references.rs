use std::collections::BTreeSet;
use serde_json::{Value, json};
use crate::ReadOnlyProjectSession;
use super::{Arguments, Result, Failure, query::paginate};

pub fn edges(session: &ReadOnlyProjectSession) -> Vec<Value> {
    let mut result = Vec::new();
    for (_, record) in session.model().records() {
        for edge in session.queries().record_references(&record.coordinate()) {
            result.push(json!({"host": {"type": record.actual_type(), "key": record.key()},
                "target": {"type": edge.target.actual_type, "key": edge.target.key},
                "path": edge.path, "dimension": edge.dimension.map(|d| json!({"dimension": d.dimension, "variant": d.variant})),
                "location": record.origin.location_for_path(&crate::CfdPath::root())}));
        }
    }
    result
}

pub fn query(session: &ReadOnlyProjectSession, args: &Arguments) -> Result<Value> {
    let ty = args.require("type")?; let key = args.require("key")?;
    let record = session.queries().record_view(ty, key).ok_or_else(|| Failure::missing(format!("record `{ty}::{key}` does not exist")))?;
    let direction = args.get("direction").unwrap_or("both");
    if !["incoming", "outgoing", "both"].contains(&direction) { return Err(Failure::invalid("direction must be incoming, outgoing or both")); }
    let depth = args.number("depth", 1)?;
    if depth == 0 || depth > 64 { return Err(Failure::invalid("reference depth must be in 1..=64")); }
    let all = edges(session);
    let mut frontier = BTreeSet::from([(record.coordinate.actual_type.to_string(), record.coordinate.key.to_string())]);
    let mut seen = frontier.clone(); let mut included = BTreeSet::new();
    for _ in 0..depth {
        let mut next = BTreeSet::new();
        for (index, edge) in all.iter().enumerate() {
            let host = pair(&edge["host"]); let target = pair(&edge["target"]);
            if direction != "incoming" && frontier.contains(&host) { included.insert(index); if seen.insert(target.clone()) { next.insert(target); } }
            if direction != "outgoing" && frontier.contains(&pair(&edge["target"])) { included.insert(index); if seen.insert(host.clone()) { next.insert(host); } }
        }
        frontier = next;
    }
    let depth_truncated = all.iter().enumerate().any(|(i, edge)| !included.contains(&i) &&
        (direction != "incoming" && frontier.contains(&pair(&edge["host"])) || direction != "outgoing" && frontier.contains(&pair(&edge["target"]))));
    let items = all.into_iter().enumerate().filter_map(|(i,v)| included.contains(&i).then_some(v)).collect();
    let mut result = super::json(paginate(items, &args.selector()?)?)?;
    result["depth_truncated"] = json!(depth_truncated);
    Ok(result)
}

pub fn pair(value: &Value) -> (String, String) {
    (value["type"].as_str().unwrap_or_default().into(), value["key"].as_str().unwrap_or_default().into())
}
