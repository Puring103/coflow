use coflow_api::DiagnosticSet;
use coflow_cfd::ast::{CfdBlockEntry, CfdRecord as AstRecord};
use coflow_cft::{CftField, CftSchema, CftSchemaDefaultValue, CftValueType};
use coflow_data_model::{CfdDictKey, CfdEnumValue, CfdObject, CfdValue};
use std::collections::{BTreeMap, BTreeSet};

use super::schema_nav::{non_nullable, object_type_name, type_after_field_segment};
use super::{diag, raw_span};

pub(super) fn cfd_top_level_fields(records: &[AstRecord], actual_type: &str) -> Vec<String> {
    let mut fields = BTreeSet::new();
    for record in records
        .iter()
        .filter(|record| record.type_name == actual_type)
    {
        for field in &record.fields {
            fields.insert(field.name.clone());
        }
    }
    let mut out = vec!["id".to_string()];
    out.extend(fields);
    out
}

pub(super) fn rewrite_cfd_records(
    source: &str,
    records: &[AstRecord],
    actual_type: &str,
    schema: &CftSchema,
) -> Result<String, DiagnosticSet> {
    let schema_type = schema.resolve_type(actual_type).ok_or_else(|| {
        DiagnosticSet::one(diag(
            "CFD-TABLE",
            format!("unknown CFT type `{actual_type}`"),
        ))
    })?;
    let fields = schema_type
        .all_fields()
        .map(|field| (field.name.to_string(), field))
        .collect::<BTreeMap<_, _>>();
    let mut replacements = Vec::new();
    for record in records
        .iter()
        .filter(|record| record.type_name == actual_type)
    {
        replacements.push((
            record.span,
            render_cfd_record(source, record, schema, &fields),
        ));
    }
    super::patch::replace_spans(source, &replacements)
}

fn render_cfd_record(
    source: &str,
    record: &AstRecord,
    schema: &CftSchema,
    fields: &BTreeMap<String, &CftField>,
) -> String {
    let existing = record
        .fields
        .iter()
        .map(|field| (field.name.clone(), raw_span(source, field.value.span())))
        .collect::<BTreeMap<_, _>>();
    let mut out = format!(
        "{}: {} {{\n",
        format_record_key(&record.key),
        record.type_name
    );
    for entry in &record.entries {
        let CfdBlockEntry::Spread(_, span) = entry else {
            continue;
        };
        out.push_str("  ");
        out.push_str(raw_span(source, *span).trim());
        out.push_str(",\n");
    }
    for (field_name, field) in fields {
        let value = existing
            .get(field_name)
            .cloned()
            .unwrap_or_else(|| default_cfd_value(schema, field));
        out.push_str("  ");
        out.push_str(field_name);
        out.push_str(": ");
        out.push_str(&value);
        out.push_str(",\n");
    }
    out.push_str("}\n");
    out
}

fn format_record_key(key: &str) -> String {
    if coflow_cft::is_cft_identifier(key) {
        key.to_string()
    } else {
        format!("{key:?}")
    }
}

fn default_cfd_value(schema: &CftSchema, field: &CftField) -> String {
    let value = field.default.as_ref().map_or_else(
        || value_from_type_default(schema, &field.value_type),
        |default| value_from_schema_default(schema, &field.value_type, default),
    );
    serialize_value_for_type(&value, None, Some(&field.value_type), 2)
}

fn value_from_schema_default(
    schema: &CftSchema,
    ty: &CftValueType,
    default: &CftSchemaDefaultValue,
) -> CfdValue {
    match default {
        CftSchemaDefaultValue::Null => CfdValue::Null,
        CftSchemaDefaultValue::Int(value) => CfdValue::Int(*value),
        CftSchemaDefaultValue::Float(value) => CfdValue::Float(*value),
        CftSchemaDefaultValue::Bool(value) => CfdValue::Bool(*value),
        CftSchemaDefaultValue::String(value) => CfdValue::String(value.clone()),
        CftSchemaDefaultValue::Enum {
            enum_name,
            variant,
            value,
        } => CfdValue::Enum(CfdEnumValue {
            enum_name: enum_name.to_string(),
            variant: Some(variant.to_string()),
            value: *value,
        }),
        CftSchemaDefaultValue::EmptyArray => CfdValue::Array(Vec::new()),
        CftSchemaDefaultValue::EmptyObject => value_from_type_default(schema, ty),
    }
}

fn value_from_type_default(schema: &CftSchema, ty: &CftValueType) -> CfdValue {
    match ty {
        CftValueType::Int => CfdValue::Int(0),
        CftValueType::Float => CfdValue::Float(0.0),
        CftValueType::Bool => CfdValue::Bool(false),
        CftValueType::String => CfdValue::String(String::new()),
        CftValueType::RecordRef(_) | CftValueType::Nullable(_) => CfdValue::Null,
        CftValueType::Array(_) => CfdValue::Array(Vec::new()),
        CftValueType::Dict(_, _) => CfdValue::Dict(Vec::new()),
        CftValueType::Enum(name) => schema
            .resolve_enum(name)
            .and_then(|enm| enm.variants.first())
            .map_or_else(
                || {
                    CfdValue::Enum(CfdEnumValue {
                        enum_name: name.to_string(),
                        variant: None,
                        value: 0,
                    })
                },
                |variant| {
                    CfdValue::Enum(CfdEnumValue {
                        enum_name: name.to_string(),
                        variant: Some(variant.name.to_string()),
                        value: variant.value,
                    })
                },
            ),
        CftValueType::Object(name) => {
            let fields = schema
                .resolve_type(name)
                .map(|schema_type| {
                    schema_type
                        .all_fields()
                        .map(|field| {
                            (
                                field.name.to_string(),
                                value_from_type_default(schema, &field.value_type),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
            CfdValue::Object(Box::new(CfdObject::new(name.to_string(), fields)))
        }
    }
}

pub(super) fn added_columns(new_header: &[String], old_header: &[String]) -> Vec<String> {
    let old = old_header.iter().collect::<BTreeSet<_>>();
    new_header
        .iter()
        .filter(|header| !old.contains(header))
        .cloned()
        .collect()
}

pub(super) fn removed_columns(new_header: &[String], old_header: &[String]) -> Vec<String> {
    let new = new_header.iter().collect::<BTreeSet<_>>();
    old_header
        .iter()
        .filter(|header| !new.contains(header))
        .cloned()
        .collect()
}

/// Serialize a `CfdValue` to CFD source text.
///
/// `depth` controls indentation for nested object bodies. Refs are always
/// emitted as `&key`; the target type is supplied by the surrounding schema
/// context rather than by the value syntax.
#[must_use]
pub(super) fn serialize_value(v: &CfdValue, depth: usize) -> String {
    serialize_value_for_type(v, None, None, depth)
}

pub(super) fn serialize_value_for_type(
    v: &CfdValue,
    schema: Option<&CftSchema>,
    expected: Option<&CftValueType>,
    depth: usize,
) -> String {
    let indent = "  ".repeat(depth);
    let outer = "  ".repeat(depth.saturating_sub(1));
    match v {
        CfdValue::Null => "null".to_string(),
        CfdValue::Bool(v) => v.to_string(),
        CfdValue::Int(v) => v.to_string(),
        CfdValue::Float(v) => {
            let s = v.to_string();
            if s.contains('.') || s.contains('e') || s.contains('E') {
                s
            } else {
                format!("{s}.0")
            }
        }
        CfdValue::String(v) => format!("{v:?}"),
        CfdValue::Enum(e) => e
            .variant
            .clone()
            .unwrap_or_else(|| format!("{}({})", e.enum_name, e.value)),
        CfdValue::Ref(target_key)
            if matches!(
                expected.map(non_nullable),
                Some(CftValueType::RecordRef(_))
            ) =>
        {
            format!("&{target_key}")
        }
        CfdValue::Ref(target_key) => format!("&{target_key}"),
        CfdValue::Object(boxed) => {
            let body = boxed
                .fields
                .iter()
                .fold(String::new(), |mut acc, (name, value)| {
                    use std::fmt::Write;
                    let field_type = schema
                        .zip(object_type_name(expected, &boxed.actual_type))
                        .and_then(|(schema, type_name)| {
                            type_after_field_segment(schema, type_name, name)
                        });
                    let _ = writeln!(
                        acc,
                        "{indent}{name}: {},",
                        serialize_value_for_type(value, schema, field_type.as_ref(), depth + 1)
                    );
                    acc
                });
            format!("{} {{\n{body}{outer}}}", boxed.actual_type)
        }
        CfdValue::Array(items) => {
            let item_type = expected.and_then(|ty| match non_nullable(ty) {
                CftValueType::Array(inner) => Some(inner.as_ref()),
                _ => None,
            });
            let elems: Vec<String> = items
                .iter()
                .map(|i| serialize_value_for_type(i, schema, item_type, depth))
                .collect();
            format!("[{}]", elems.join(", "))
        }
        CfdValue::Dict(entries) => {
            let item_type = expected.and_then(|ty| match non_nullable(ty) {
                CftValueType::Dict(_, item) => Some(item.as_ref()),
                _ => None,
            });
            let pairs: Vec<String> = entries
                .iter()
                .map(|(k, v)| {
                    let key = match k {
                        CfdDictKey::String(s) => format!("{s:?}"),
                        CfdDictKey::Int(n) => n.to_string(),
                        CfdDictKey::Enum(e) => e
                            .variant
                            .clone()
                            .unwrap_or_else(|| format!("{}({})", e.enum_name, e.value)),
                    };
                    format!(
                        "{key}: {}",
                        serialize_value_for_type(v, schema, item_type, depth)
                    )
                })
                .collect();
            format!("{{{}}}", pairs.join(", "))
        }
    }
}
