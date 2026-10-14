use std::collections::BTreeMap;
use serde_json::{json, Value};
use crate::{CfdValue, ReadOnlyProjectSession};
use super::{Arguments, InputSnapshot, Result, Failure, query::{selected_records, value_at, paginate}};

/// 项目统计与分组统计使用公共 DTO，数值指标的动态筛选单独处理。
#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct ProjectStatistics {
    pub types: usize,
    pub records: usize,
    pub files: usize,
    pub dimensions: usize,
    pub variants: usize,
    pub references: usize,
    pub diagnostics: usize,
    pub rejected_records: usize,
}

#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct RecordStatistics { pub group: String, pub count: usize }

#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct DimensionStatistics {
    pub group: String,
    pub eligible_fields: usize,
    pub explicit: usize,
    pub fallback: usize,
    pub explicit_none: usize,
    pub zero_denominator: bool,
    pub coverage: Option<f64>,
}

pub fn query(command: &str, session: &ReadOnlyProjectSession, snapshot: &InputSnapshot, args: &Arguments) -> Result<Value> {
    let selector = args.selector()?;
    let records = selected_records(session, &selector)?;
    let rejected = session.queries().rejected_records().len();
    let dimensions = session.queries().dimensions();
    match command {
        "stats.project" => super::json(ProjectStatistics { types: session.schema().all_types().count(), records: records.len(),
            files: snapshot.files.iter().filter(|f| f.kind == "data" || f.kind == "schema").count(),
            dimensions: dimensions.len(), variants: dimensions.iter().map(|d| d.variants.len()).sum::<usize>(),
            references: super::references::edges(session).len(), diagnostics: session.queries().diagnostics().as_set().diagnostics.len(),
            rejected_records: rejected }),
        "stats.records" => {
            let group = args.get("group-by").unwrap_or("type");
            if !["type", "file"].contains(&group) { return Err(Failure::request("records group-by must be type or file")); }
            let mut groups = BTreeMap::<String, usize>::new();
            for record in records {
                let key = if group == "file" { session.queries().file_for_record(record.actual_type(), record.key()).unwrap_or_default() } else { record.actual_type() };
                *groups.entry(key.into()).or_default() += 1;
            }
            super::json(paginate(groups.into_iter().map(|(group, count)| RecordStatistics { group, count }).collect(), &selector)?)
        }
        "stats.field" => {
            let field = args.require("field")?;
            let mut values = Vec::new(); let mut encoded_values = Vec::new(); let mut unreachable = 0;
            for record in &records {
                super::query::validate_field(session, record.actual_type(), field)?;
                if let Some(value) = value_at(session, record, field)? {
                    values.push(value);
                    if args.get("group-by") == Some("value") {
                        let path = super::query::resolved_path(session, record.actual_type(), field)?;
                        let expected = crate::write_rules::expected_type_for_record_path(session.schema(), record, &path, "CLI-PATH", "CLI")?;
                        encoded_values.push(super::model_value::encode(session.schema(), &expected, Some(value))?);
                    }
                } else { unreachable += 1; }
            }
            let quantiles = if args.get("quantiles").is_some() { args.csv("quantiles").into_iter().map(|v|v.parse::<f64>().map_err(|_|Failure::request("quantiles must be numbers in [0,1]"))).collect::<Result<Vec<_>>>()? } else { vec![0.5,0.9,0.95,0.99] };
            if quantiles.iter().any(|q|!q.is_finite()||!(0.0..=1.0).contains(q)){return Err(Failure::request("quantiles must be numbers in [0,1]"));}
            let mut result = aggregate(&values, args.number("option-layer", 1)?, &quantiles)?;
            result["path_unreachable"] = json!(unreachable);
            result["rejected_records"] = json!(rejected);
            if args.get("group-by") == Some("value") {
                let mut groups = BTreeMap::<String, (Value, usize)>::new();
                for encoded in encoded_values {
                    let entry = groups.entry(encoded.to_string()).or_insert((encoded, 0)); entry.1 += 1;
                }
                result["distribution"] = super::json(paginate(groups.into_values().map(|(value,count)| json!({"value": value, "count": count,
                    "ratio": if values.is_empty() { None } else { Some(count as f64 / values.len() as f64) }})).collect(), &selector)?)?;
            }
            if args.get("metrics").is_some(){
                let metrics=args.csv("metrics");
                for metric in &metrics{if !["count","min","max","sum","mean","quantiles","some","none","empty_count","length_distribution"].contains(&metric.as_str()){return Err(Failure::request(format!("unknown metric `{metric}`")));}}
                if let Some(object)=result.as_object_mut(){object.retain(|name,_|metrics.contains(name)||["numeric_count","path_unreachable","rejected_records","option_layer","precision","quantile_method","length_unit","distribution"].contains(&name.as_str()));}
            }
            Ok(result)
        }
        "stats.dimension" => {
            let name = args.require("dimension")?; let variant = args.require("variant")?;
            if !dimensions.iter().any(|d| d.name == name) { return Err(Failure::missing(format!("unknown dimension `{name}`"))); }
            let mut groups = BTreeMap::<String, [usize; 3]>::new();
            for record in &records {
                let group = match args.get("group-by") { Some("type") => record.actual_type(), Some("file") => session.queries().file_for_record(record.actual_type(), record.key()).unwrap_or_default(), _ => "all" };
                let counts = groups.entry(group.into()).or_default();
                if let Some(ty) = session.schema().resolve_type(record.actual_type()) {
                    for field in ty.all_fields().filter(|f| f.dimension.as_ref().is_some_and(|d| d.dimension.as_str() == name)) {
                        if args.get("field").is_some_and(|f| f != field.name.as_str()) { continue; }
                        counts[0] += 1;
                        if let Some(value) = record.dimension_field(&field.name).and_then(|d| d.variants.get(variant)) {
                            counts[1] += 1; if matches!(value.value, CfdValue::OptionNone) { counts[2] += 1; }
                        }
                    }
                }
            }
            if groups.is_empty() { groups.insert("all".into(), [0;3]); }
            let items = groups.into_iter().map(|(group, [eligible, explicit, none])| DimensionStatistics { group, eligible_fields: eligible,
                explicit, fallback: eligible-explicit, explicit_none: none, zero_denominator: eligible == 0,
                coverage: if eligible == 0 { None } else { Some(explicit as f64 / eligible as f64) } }).collect();
            super::json(paginate(items, &selector)?)
        }
        "stats.refs" => {
            let edges = super::references::edges(session);
            let items = records.iter().map(|record| {
                let id = (record.actual_type().to_string(), record.key().to_string());
                let incoming = edges.iter().filter(|e| super::references::pair(&e.target) == id).count();
                let outgoing = edges.iter().filter(|e| super::references::pair(&e.host) == id).count();
                json!({"type": record.actual_type(), "key": record.key(), "incoming": incoming, "outgoing": outgoing, "zero_incoming": incoming == 0})
            }).collect();
            super::json(paginate(items, &selector)?)
        }
        _ => Err(Failure::request("unknown statistics command")),
    }
}

fn aggregate(values: &[&CfdValue], option_layer: usize, quantiles:&[f64]) -> Result<Value> {
    if option_layer==0 {return Err(Failure::request("option-layer starts at 1"));}
    let mut none = 0; let mut some = 0; let mut lengths = Vec::new(); let mut numeric = Vec::new();
    let mut int_sum = 0i128; let mut all_int = true;let mut integers=Vec::new();
    for value in values {
        let mut current = *value;
        for layer in 1..=option_layer {
            match current {
                CfdValue::OptionNone => { if layer == option_layer { none += 1; } break; }
                CfdValue::OptionSome(inner) => { if layer == option_layer { some += 1; } current = inner; }
                _ => break,
            }
        }
        while let CfdValue::OptionSome(inner)=current{current=inner;}
        match current {
            CfdValue::Int(v) => { int_sum = int_sum.checked_add(i128::from(*v)).ok_or_else(|| Failure::request("integer aggregate overflow"))?; integers.push(*v);numeric.push(*v as f64); }
            CfdValue::Float(v) => { all_int = false; numeric.push(*v); }
            CfdValue::String(v) => lengths.push(v.chars().count()),
            CfdValue::FormattedString(v) => lengths.push(v.rendered.chars().count()),
            CfdValue::Array(v) => lengths.push(v.len()), CfdValue::Dict(v) => lengths.push(v.len()), _ => {}
        }
    }
    numeric.sort_by(f64::total_cmp); lengths.sort_unstable();integers.sort_unstable();
    let sum: f64 = numeric.iter().sum();
    if !sum.is_finite() { return Err(Failure::request("floating aggregate overflow")); }
    let mut result = json!({"count": values.len(), "some": some, "none": none, "option_layer": option_layer,
        "numeric_count": numeric.len(), "min": numeric.first(), "max": numeric.last(),
        "sum": if all_int { json!(int_sum.to_string()) } else { json!(sum) },
        "mean": if numeric.is_empty() { Value::Null } else if all_int { json!({"numerator": int_sum.to_string(), "denominator": numeric.len()}) } else { json!(sum / numeric.len() as f64) },
        "empty_count": lengths.iter().filter(|n| **n == 0).count(), "length_unit": "unicode_scalar_values_or_elements",
        "length_min": lengths.first(), "length_max": lengths.last()});
    let mut distribution=BTreeMap::<usize,usize>::new();for length in &lengths{*distribution.entry(*length).or_default()+=1;}
    result["length_distribution"]=json!(distribution.into_iter().map(|(length,count)|json!({"length":length,"count":count})).collect::<Vec<_>>());
    result["precision"]=json!(if all_int{"exact_integer_and_rational"}else{"ieee754_binary64_checked_finite"});
    result["quantile_method"]=json!("nearest_rank");
    result["quantiles"]=json!(quantiles.iter().map(|q|{
        let index=((*q*numeric.len() as f64).ceil() as usize).saturating_sub(1);
        let value=if all_int{json!(integers.get(index).map(ToString::to_string))}else{json!(numeric.get(index))};
        json!({"q":q,"value":value})
    }).collect::<Vec<_>>());
    // 整数端点保持原始编码；均值使用精确分数，避免 64 位整数经过浮点舍入。
    if all_int {
        result["min"] = json!(integers.first().map(ToString::to_string));
        result["max"] = json!(integers.last().map(ToString::to_string));
    }
    Ok(result)
}
