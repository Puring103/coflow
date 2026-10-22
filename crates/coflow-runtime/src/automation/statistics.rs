use super::{
    query::{paginate, selected_records, value_at},
    Arguments, Failure, InputSnapshot, Result,
};
use crate::{CfdValue, ReadOnlyProjectSession};
use serde_json::Value;
use std::collections::BTreeMap;

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
pub struct RecordStatistics {
    pub group: String,
    pub count: usize,
}

#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct ReferenceStatistics {
    #[serde(rename = "type")]
    pub actual_type: String,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub incoming: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outgoing: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zero_incoming: Option<bool>,
}

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

#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum StatisticNumber {
    Integer(String),
    Float(f64),
}

#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum StatisticMean {
    Rational {
        numerator: String,
        denominator: usize,
    },
    Float(f64),
}

#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct StatisticQuantile {
    pub q: f64,
    pub value: Option<StatisticNumber>,
}

#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct LengthFrequency {
    pub length: usize,
    pub count: usize,
}

#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct ValueFrequency {
    pub value: super::ModelValue,
    pub count: usize,
    pub ratio: Option<f64>,
}

#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct FieldStatistics {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub some: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub none: Option<usize>,
    pub option_layer: usize,
    pub numeric_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<Option<StatisticNumber>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<Option<StatisticNumber>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sum: Option<StatisticNumber>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mean: Option<Option<StatisticMean>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub empty_count: Option<usize>,
    pub length_unit: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub length_min: Option<Option<usize>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub length_max: Option<Option<usize>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub length_distribution: Option<Vec<LengthFrequency>>,
    pub precision: String,
    pub quantile_method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantiles: Option<Vec<StatisticQuantile>>,
    pub path_unreachable: usize,
    pub option_unreachable: usize,
    pub rejected_records: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distribution: Option<super::Page<ValueFrequency>>,
}

impl FieldStatistics {
    fn select_metrics(&mut self, metrics: &[String]) {
        // 外层 Option 控制是否输出指标；内层 Option 保留“没有样本”的 null。
        macro_rules! select { ($($field:ident),*) => { $(if !metrics.iter().any(|name| name == stringify!($field)) { self.$field = None; })* }; }
        select!(
            count,
            min,
            max,
            sum,
            mean,
            quantiles,
            some,
            none,
            empty_count,
            length_distribution,
            length_min,
            length_max
        );
    }
}

pub fn query(
    command: &str,
    session: &ReadOnlyProjectSession,
    snapshot: &InputSnapshot,
    args: &Arguments,
) -> Result<Value> {
    let selector = args.selector()?;
    let records = selected_records(session, &selector)?;
    let rejected = session.queries().rejected_records().len();
    let dimensions = session.queries().dimensions();
    match command {
        "stats.project" => super::json(ProjectStatistics {
            types: session.schema().all_types().count(),
            records: records.len(),
            files: snapshot
                .files
                .iter()
                .filter(|f| f.kind == "data" || f.kind == "schema")
                .count(),
            dimensions: dimensions.len(),
            variants: dimensions.iter().map(|d| d.variants.len()).sum::<usize>(),
            references: super::references::edges(session).len(),
            diagnostics: session.queries().diagnostics().as_set().diagnostics.len(),
            rejected_records: rejected,
        }),
        "stats.records" => {
            let group = args.get("group-by").unwrap_or("type");
            if !["type", "file"].contains(&group) {
                return Err(Failure::request("records group-by must be type or file"));
            }
            let mut groups = BTreeMap::<String, usize>::new();
            for record in records {
                let key = if group == "file" {
                    session
                        .queries()
                        .file_for_record(record.actual_type(), record.id())
                        .unwrap_or_default()
                } else {
                    record.actual_type()
                };
                *groups.entry(key.into()).or_default() += 1;
            }
            super::json(paginate(
                groups
                    .into_iter()
                    .map(|(group, count)| RecordStatistics { group, count })
                    .collect(),
                &selector,
            )?)
        }
        "stats.field" => {
            if args.get("group-by").is_some_and(|group| group != "value") {
                return Err(Failure::request("field group-by must be value"));
            }
            let field = args.require("field")?;
            let types = super::query::selected_types(session, &selector)?;
            if types.is_empty() {
                return Err(Failure::missing(
                    "no record type is available for the statistics field",
                ));
            }
            let range = super::query::record_range(session, &selector)?;
            let declared = super::query::range_field_types(session, &types, &range, field)?;
            let mut base_types = Vec::new();
            let mut option_depths = Vec::new();
            for expected in &declared {
                let mut expected = expected;
                let mut depth = 0;
                while let coflow_language::cft::CftValueType::Option(inner) = expected {
                    expected = inner;
                    depth += 1;
                }
                base_types.push(expected);
                option_depths.push(depth);
            }
            let numeric = base_types.iter().all(|expected| {
                matches!(
                    expected,
                    coflow_language::cft::CftValueType::Int
                        | coflow_language::cft::CftValueType::Float
                )
            });
            let length = base_types.iter().all(|expected| {
                matches!(
                    expected,
                    coflow_language::cft::CftValueType::String
                        | coflow_language::cft::CftValueType::Array(_)
                        | coflow_language::cft::CftValueType::Dict(_, _)
                )
            });
            let layer = args.number("option-layer", 1)?;
            if args.get("option-layer").is_some()
                && option_depths.iter().any(|depth| *depth < layer)
            {
                return Err(Failure::request(
                    "option-layer exceeds the selected field's Option depth",
                ));
            }
            let distribution = args.get("group-by") == Some("value")
                || base_types.iter().all(|expected| {
                    matches!(
                        expected,
                        coflow_language::cft::CftValueType::Enum(_)
                            | coflow_language::cft::CftValueType::Bool
                    )
                });
            let mut values = Vec::new();
            let mut encoded_values = Vec::new();
            let mut unreachable = 0;
            for record in &records {
                if let Some(value) = value_at(session, record, field)? {
                    values.push(value);
                    if distribution {
                        let path = super::query::resolved_record_path(session, record, field)?;
                        let expected = crate::write_rules::expected_type_for_record_path(
                            session.schema(),
                            record,
                            &path,
                            "CLI-PATH",
                            "CLI",
                        )?;
                        encoded_values.push(super::ModelValue::from_model(
                            session.schema(),
                            &expected,
                            value,
                        )?);
                    }
                } else {
                    unreachable += 1;
                }
            }
            let quantiles = if args.get("quantiles").is_some() {
                args.csv("quantiles")
                    .into_iter()
                    .map(|v| {
                        v.parse::<f64>()
                            .map_err(|_| Failure::request("quantiles must be numbers in [0,1]"))
                    })
                    .collect::<Result<Vec<_>>>()?
            } else {
                vec![0.5, 0.9, 0.95, 0.99]
            };
            if quantiles
                .iter()
                .any(|q| !q.is_finite() || !(0.0..=1.0).contains(q))
            {
                return Err(Failure::request("quantiles must be numbers in [0,1]"));
            }
            let mut result = aggregate(&values, layer, &quantiles)?;
            result.path_unreachable = unreachable;
            result.rejected_records = rejected;
            if distribution {
                let mut groups = BTreeMap::<String, (super::ModelValue, usize)>::new();
                for encoded in encoded_values {
                    let entry = groups
                        .entry(serde_json::to_string(&encoded)?)
                        .or_insert((encoded, 0));
                    entry.1 += 1;
                }
                result.distribution = Some(paginate(
                    groups
                        .into_values()
                        .map(|(value, count)| ValueFrequency {
                            value,
                            count,
                            ratio: if values.is_empty() {
                                None
                            } else {
                                Some(count as f64 / values.len() as f64)
                            },
                        })
                        .collect(),
                    &selector,
                )?);
            }
            if args.get("metrics").is_some() {
                let metrics = args.csv("metrics");
                for metric in &metrics {
                    if ![
                        "count",
                        "min",
                        "max",
                        "sum",
                        "mean",
                        "quantiles",
                        "some",
                        "none",
                        "empty_count",
                        "length_distribution",
                        "length_min",
                        "length_max",
                    ]
                    .contains(&metric.as_str())
                    {
                        return Err(Failure::request(format!("unknown metric `{metric}`")));
                    }
                }
                for metric in &metrics {
                    if ["min", "max", "sum", "mean", "quantiles"].contains(&metric.as_str())
                        && !numeric
                    {
                        return Err(Failure::request("numeric metrics require numeric fields"));
                    }
                    if [
                        "empty_count",
                        "length_distribution",
                        "length_min",
                        "length_max",
                    ]
                    .contains(&metric.as_str())
                        && !length
                    {
                        return Err(Failure::request(
                            "length metrics require string or collection fields",
                        ));
                    }
                    if ["some", "none"].contains(&metric.as_str())
                        && option_depths.iter().any(|depth| *depth < layer)
                    {
                        return Err(Failure::request(
                            "Option metrics require the selected Option layer",
                        ));
                    }
                }
                result.select_metrics(&metrics);
            } else {
                if !numeric {
                    result.min = None;
                    result.max = None;
                    result.sum = None;
                    result.mean = None;
                    result.quantiles = None;
                }
                if !length {
                    result.empty_count = None;
                    result.length_distribution = None;
                    result.length_min = None;
                    result.length_max = None;
                }
                if option_depths.iter().any(|depth| *depth < layer) {
                    result.some = None;
                    result.none = None;
                }
            }
            result.length_unit = if base_types.iter().all(|expected| {
                matches!(
                    expected,
                    coflow_language::cft::CftValueType::String
                )
            }) {
                "unicode_scalar_values"
            } else {
                "elements"
            }
            .into();
            super::json(result)
        }
        "stats.dimension" => {
            if args
                .get("group-by")
                .is_some_and(|group| !["all", "type", "file"].contains(&group))
            {
                return Err(Failure::request(
                    "dimension group-by must be all, type or file",
                ));
            }
            let name = args.require("dimension")?;
            let variant = args.require("variant")?;
            if !dimensions.iter().any(|d| d.name == name) {
                return Err(Failure::missing(format!("unknown dimension `{name}`")));
            }
            if let Some(field) = args.get("field") {
                super::dimension::validate_bound_field(session, &selector, name, field)?;
            }
            let mut groups = BTreeMap::<String, [usize; 3]>::new();
            for record in &records {
                let group = match args.get("group-by") {
                    Some("type") => record.actual_type(),
                    Some("file") => session
                        .queries()
                        .file_for_record(record.actual_type(), record.id())
                        .unwrap_or_default(),
                    _ => "all",
                };
                let counts = groups.entry(group.into()).or_default();
                if let Some(ty) = session.schema().resolve_type(record.actual_type()) {
                    for field in ty.all_fields().filter(|f| {
                        f.dimension
                            .as_ref()
                            .is_some_and(|d| d.dimension.as_str() == name)
                    }) {
                        if args.get("field").is_some_and(|f| f != field.name.as_str()) {
                            continue;
                        }
                        counts[0] += 1;
                        if let Some(value) = record
                            .dimension_field(&field.name)
                            .and_then(|d| d.variants.get(variant))
                        {
                            counts[1] += 1;
                            if matches!(value.value, CfdValue::OptionNone) {
                                counts[2] += 1;
                            }
                        }
                    }
                }
            }
            if groups.is_empty() {
                groups.insert("all".into(), [0; 3]);
            }
            let items = groups
                .into_iter()
                .map(|(group, [eligible, explicit, none])| DimensionStatistics {
                    group,
                    eligible_fields: eligible,
                    explicit,
                    fallback: eligible - explicit,
                    explicit_none: none,
                    zero_denominator: eligible == 0,
                    coverage: if eligible == 0 {
                        None
                    } else {
                        Some(explicit as f64 / eligible as f64)
                    },
                })
                .collect();
            super::json(paginate(items, &selector)?)
        }
        "stats.refs" => {
            let direction = args.get("direction").unwrap_or("both");
            if !["incoming", "outgoing", "both"].contains(&direction) {
                return Err(Failure::request(
                    "direction must be incoming, outgoing or both",
                ));
            }
            let edges = super::references::edges(session);
            let items = records
                .iter()
                .map(|record| {
                    let id = (record.actual_type().to_string(), record.id().to_string());
                    let incoming = edges
                        .iter()
                        .filter(|e| super::references::pair(&e.target) == id)
                        .count();
                    let outgoing = edges
                        .iter()
                        .filter(|e| super::references::pair(&e.host) == id)
                        .count();
                    ReferenceStatistics {
                        actual_type: record.actual_type().to_string(),
                        id: record.id().to_string(),
                        incoming: (direction != "outgoing").then_some(incoming),
                        outgoing: (direction != "incoming").then_some(outgoing),
                        zero_incoming: (direction != "outgoing").then_some(incoming == 0),
                    }
                })
                .collect();
            super::json(paginate(items, &selector)?)
        }
        _ => Err(Failure::request("unknown statistics command")),
    }
}

fn aggregate(
    values: &[&CfdValue],
    option_layer: usize,
    quantiles: &[f64],
) -> Result<FieldStatistics> {
    if option_layer == 0 {
        return Err(Failure::request("option-layer starts at 1"));
    }
    let mut none = 0;
    let mut some = 0;
    let mut option_unreachable = 0;
    let mut lengths = Vec::new();
    let mut numeric = Vec::new();
    let mut int_sum = 0i128;
    let mut all_int = true;
    let mut integers = Vec::new();
    for value in values {
        let mut current = *value;
        for layer in 1..=option_layer {
            match current {
                CfdValue::OptionNone => {
                    if layer == option_layer {
                        none += 1;
                    } else {
                        option_unreachable += 1;
                    }
                    break;
                }
                CfdValue::OptionSome(inner) => {
                    if layer == option_layer {
                        some += 1;
                    }
                    current = inner;
                }
                _ => break,
            }
        }
        while let CfdValue::OptionSome(inner) = current {
            current = inner;
        }
        match current {
            CfdValue::Int(v) => {
                int_sum = int_sum
                    .checked_add(i128::from(*v))
                    .ok_or_else(|| Failure::request("integer aggregate overflow"))?;
                integers.push(*v);
                numeric.push(*v as f64);
            }
            CfdValue::Float(v) => {
                all_int = false;
                numeric.push(*v);
            }
            CfdValue::String(v) => lengths.push(v.chars().count()),
            CfdValue::FormattedString(v) => lengths.push(v.rendered.chars().count()),
            CfdValue::Array(v) => lengths.push(v.len()),
            CfdValue::Dict(v) => lengths.push(v.len()),
            _ => {}
        }
    }
    numeric.sort_by(f64::total_cmp);
    lengths.sort_unstable();
    integers.sort_unstable();
    let sum: f64 = numeric.iter().sum();
    if !sum.is_finite() {
        return Err(Failure::request("floating aggregate overflow"));
    }
    let mut distribution = BTreeMap::<usize, usize>::new();
    for length in &lengths {
        *distribution.entry(*length).or_default() += 1;
    }
    let endpoints = |first: bool| {
        if all_int {
            (if first {
                integers.first()
            } else {
                integers.last()
            })
            .map(|value| StatisticNumber::Integer(value.to_string()))
        } else {
            (if first {
                numeric.first()
            } else {
                numeric.last()
            })
            .copied()
            .map(StatisticNumber::Float)
        }
    };
    // 整数端点与和保持十进制编码，均值保持精确分数。
    let quantiles = quantiles
        .iter()
        .map(|q| {
            let index = ((*q * numeric.len() as f64).ceil() as usize).saturating_sub(1);
            let value = if all_int {
                integers
                    .get(index)
                    .map(|value| StatisticNumber::Integer(value.to_string()))
            } else {
                numeric.get(index).copied().map(StatisticNumber::Float)
            };
            StatisticQuantile { q: *q, value }
        })
        .collect();
    Ok(FieldStatistics {
        count: Some(values.len()),
        some: Some(some),
        none: Some(none),
        option_layer,
        numeric_count: numeric.len(),
        min: Some(endpoints(true)),
        max: Some(endpoints(false)),
        sum: Some(if all_int {
            StatisticNumber::Integer(int_sum.to_string())
        } else {
            StatisticNumber::Float(sum)
        }),
        mean: Some(if numeric.is_empty() {
            None
        } else if all_int {
            Some(StatisticMean::Rational {
                numerator: int_sum.to_string(),
                denominator: numeric.len(),
            })
        } else {
            Some(StatisticMean::Float(sum / numeric.len() as f64))
        }),
        empty_count: Some(lengths.iter().filter(|length| **length == 0).count()),
        length_unit: "unicode_scalar_values_or_elements".into(),
        length_min: Some(lengths.first().copied()),
        length_max: Some(lengths.last().copied()),
        length_distribution: Some(
            distribution
                .into_iter()
                .map(|(length, count)| LengthFrequency { length, count })
                .collect(),
        ),
        precision: if all_int {
            "exact_integer_and_rational"
        } else {
            "ieee754_binary64_checked_finite"
        }
        .into(),
        quantile_method: "nearest_rank".into(),
        quantiles: Some(quantiles),
        path_unreachable: 0,
        option_unreachable,
        rejected_records: 0,
        distribution: None,
    })
}
