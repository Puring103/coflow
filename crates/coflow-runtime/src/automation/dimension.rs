use serde_json::{Value, json};
use crate::{ReadOnlyProjectSession, CfdRecord};
use super::{Arguments, InputSnapshot, Result, Failure, query::{selected_records, paginate}};

pub fn field_value(record: &CfdRecord, field: &str, dimension: &str, variant: &str) -> Value {
    let base = record.field(field);
    let explicit = record.dimension_field(field).filter(|f| f.dimension.as_str() == dimension)
        .and_then(|f| f.variants.get(variant));
    let effective = explicit.map(|v| &v.value).or(base);
    json!({"type": record.actual_type(), "key": record.key(), "field": field,
        "dimension": dimension, "variant": variant, "base": base, "explicit": explicit.is_some(),
        "override": explicit.map(|v| &v.value), "effective": effective,
        "origin": if explicit.is_some() { "dimension_override" } else { "base" },
        "location": explicit.map(|v| &v.origin)})
}

pub fn query(command: &str, session: &ReadOnlyProjectSession, snapshot: &InputSnapshot, args: &Arguments) -> Result<Value> {
    let name = args.require("dimension")?;
    let dimension = session.queries().dimensions().into_iter().find(|d| d.name == name)
        .ok_or_else(|| Failure::missing(format!("unknown dimension `{name}`")))?;
    if command == "dimension.inspect" { return super::json(dimension); }
    if command == "dimension.variants" {
        let items: Vec<_> = dimension.variants.iter().map(|variant| {
            let mut count = 0;
            let mut locations = Vec::new();
            for (_, record) in session.model().records() {
                for values in record.dimension_fields.values().filter(|v| v.dimension.as_str() == name) {
                    if let Some(value) = values.variants.get(variant.as_str()) { count += 1; locations.push(&value.origin); }
                }
            }
            json!({"name": variant, "overrides": count, "locations": locations})
        }).collect();
        return super::json(paginate(items, &args.selector()?, &snapshot.id)?);
    }
    let variant = if command == "dimension.diff" { args.require("to")? } else { args.require("variant")? };
    let selector = args.selector()?;
    let records = selected_records(session, &selector)?;
    let mut items = Vec::new();
    for record in &records {
        let ty = session.schema().resolve_type(record.actual_type()).ok_or_else(|| Failure::missing("record type is unavailable"))?;
        for field in ty.all_fields().filter(|f| f.dimension.as_ref().is_some_and(|d| d.dimension.as_str() == name)) {
            if args.get("field").is_some_and(|f| f != field.name.as_str()) { continue; }
            let current = field_value(record, &field.name, name, variant);
            if command == "dimension.missing" && current["explicit"] == true { continue; }
            if command == "dimension.diff" {
                let from = args.get("from").unwrap_or("base");
                let previous = if from == "base" { json!({"explicit": false, "effective": record.field(&field.name)}) }
                    else { field_value(record, &field.name, name, from) };
                let state_changed = previous["explicit"] != current["explicit"];
                let value_changed = previous["effective"] != current["effective"];
                if state_changed || value_changed { items.push(json!({"type": record.actual_type(), "key": record.key(), "field": field.name,
                    "before": previous, "after": current, "coverage_changed": state_changed, "value_changed": value_changed})); }
            } else { items.push(current); }
        }
    }
    if command == "dimension.get" {
        let mut value = items.into_iter().next().ok_or_else(|| Failure::missing("record or dimension-bound field does not exist"))?;
        value["variant_exists"] = json!(dimension.variants.iter().any(|v| v == variant));
        return Ok(value);
    }
    super::json(paginate(items, &selector, &snapshot.id)?)
}
