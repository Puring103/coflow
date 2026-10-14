use serde_json::{Value, json};
use crate::{ReadOnlyProjectSession, CfdRecord};
use super::{Arguments, Result, Failure, query::{selected_records, paginate}};

pub fn field_value(session: &ReadOnlyProjectSession, record: &CfdRecord, field: &str, dimension: &str, variant: &str) -> Result<Value> {
    let base = record.field(field);
    let explicit = record.dimension_field(field).filter(|f| f.dimension.as_str() == dimension)
        .and_then(|f| f.variants.get(variant));
    let effective = explicit.map(|v| &v.value).or(base);
    let expected = &session.schema().field(record.actual_type(), field).ok_or_else(|| Failure::missing("unknown dimension field"))?.value_type;
    Ok(json!({"type": record.actual_type(), "key": record.key(), "field": field,
        "dimension": dimension, "variant": variant, "base": super::model_value::encode(session.schema(), expected, base)?, "explicit": explicit.is_some(),
        "override": super::model_value::encode(session.schema(), expected, explicit.map(|v| &v.value))?, "effective": super::model_value::encode(session.schema(), expected, effective)?,
        "rendered": effective.and_then(super::model_value::rendered),
        "origin": if explicit.is_some() { "dimension_override" } else { "base" },
        "location": explicit.map(|v| &v.origin)}))
}

pub fn query(command: &str, session: &ReadOnlyProjectSession, args: &Arguments) -> Result<Value> {
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
        return super::json(paginate(items, &args.selector()?)?);
    }
    let variant = if command == "dimension.diff" {
        comparison_variant(args, "to")?
    } else { Some(args.require("variant")?) };
    let from = if command == "dimension.diff" { comparison_variant(args, "from")? } else { None };
    let selector = args.selector()?;
    let records = selected_records(session, &selector)?;
    let mut items = Vec::new();
    for record in &records {
        let ty = session.schema().resolve_type(record.actual_type()).ok_or_else(|| Failure::missing("record type is unavailable"))?;
        for field in ty.all_fields().filter(|f| f.dimension.as_ref().is_some_and(|d| d.dimension.as_str() == name)) {
            if args.get("field").is_some_and(|f| f != field.name.as_str()) { continue; }
            let current = view_value(session, record, &field.name, name, variant)?;
            if command == "dimension.missing" && current["explicit"] == true { continue; }
            if command == "dimension.diff" {
                let previous = view_value(session, record, &field.name, name, from)?;
                let state_changed = previous["explicit"] != current["explicit"];
                let value_changed = previous["effective"] != current["effective"];
                let rendered_changed = previous["rendered"] != current["rendered"];
                if state_changed || value_changed || rendered_changed { items.push(json!({"type": record.actual_type(), "key": record.key(), "field": field.name,
                    "before": previous, "after": current, "coverage_changed": state_changed, "value_changed": value_changed, "rendered_changed": rendered_changed})); }
            } else { items.push(current); }
        }
    }
    if command == "dimension.get" {
        let mut value = items.into_iter().next().ok_or_else(|| Failure::missing("record or dimension-bound field does not exist"))?;
        value["variant_exists"] = json!(dimension.variants.iter().any(|v| Some(v.as_str()) == variant));
        return Ok(value);
    }
    super::json(paginate(items, &selector)?)
}

// 基础视角使用独立开关，变体名称（包括 base）始终按原名解释。
fn comparison_variant<'a>(args: &'a Arguments, side: &str) -> Result<Option<&'a str>> {
    let flag = args.flag(&format!("{side}-base"));
    match (flag, args.get(side)) {
        (true, None) => Ok(None),
        (false, Some(variant)) => Ok(Some(variant)),
        _ => Err(Failure::request(format!("specify either --{side} or --{side}-base"))),
    }
}

fn view_value(session: &ReadOnlyProjectSession, record: &CfdRecord, field: &str, dimension: &str, variant: Option<&str>) -> Result<Value> {
    if let Some(variant) = variant { return field_value(session, record, field, dimension, variant); }
    let base = record.field(field);
    let expected = &session.schema().field(record.actual_type(), field).ok_or_else(|| Failure::missing("unknown field"))?.value_type;
    Ok(json!({"explicit":false,"effective":super::model_value::encode(session.schema(), expected, base)?,
        "rendered":base.and_then(super::model_value::rendered),"origin":"base"}))
}
