use coflow_language::cfd::{CfdAst, CfdField, CfdValue};
use coflow_language::cft::{CftSchema, CftValueType};

use super::span_contains;

/// Returns the CFT type name under a CFD cursor position.
pub fn definition_type_name(ast: &CfdAst, offset: usize) -> Option<&str> {
    for record in &ast.records {
        if span_contains(record.type_span, offset) {
            return Some(&record.type_name);
        }
        for field in &record.fields {
            if let Some(type_name) = type_name_in_value(&field.value, offset) {
                return Some(type_name);
            }
        }
    }
    None
}

fn type_name_in_value(value: &CfdValue, offset: usize) -> Option<&str> {
    match value {
        CfdValue::Block(block) => {
            if let Some((name, span)) = &block.type_marker {
                if span_contains(*span, offset) {
                    return Some(name.as_str());
                }
            }
            for field in &block.fields {
                if let Some(type_name) = type_name_in_value(&field.value, offset) {
                    return Some(type_name);
                }
            }
            None
        }
        CfdValue::Array(items, _) => {
            for item in items {
                if let Some(type_name) = type_name_in_value(item, offset) {
                    return Some(type_name);
                }
            }
            None
        }
        _ => None,
    }
}

/// Definition: return the owning type and field name when the cursor is on a
/// CFD record field.
pub fn definition_field_name<'a>(
    ast: &'a CfdAst,
    schema: Option<&CftSchema>,
    offset: usize,
) -> Option<(String, &'a str)> {
    for record in &ast.records {
        let type_name = record.type_name.clone();
        for field in &record.fields {
            if let Some(field) = field_name_in_field(field, schema, type_name.clone(), offset) {
                return Some(field);
            }
        }
    }
    None
}

fn field_name_in_field<'a>(
    field: &'a CfdField,
    schema: Option<&CftSchema>,
    owner_type: String,
    offset: usize,
) -> Option<(String, &'a str)> {
    field_name_in_fields(std::slice::from_ref(field), schema, owner_type, offset)
}

fn field_name_in_fields<'a>(
    fields: &'a [CfdField],
    schema: Option<&CftSchema>,
    owner_type: String,
    offset: usize,
) -> Option<(String, &'a str)> {
    for field in fields {
        if span_contains(field.name_span, offset) {
            return Some((owner_type, &field.name));
        }
        let next_owner = schema
            .and_then(|schema| schema.resolve_type(&owner_type))
            .and_then(|ty| {
                ty.all_fields()
                    .find(|schema_field| schema_field.name.as_str() == field.name)
            })
            .and_then(|schema_field| named_type_name(&schema_field.value_type))
            .map(str::to_string);
        if let Some(next_owner) = next_owner {
            if let Some(result) = field_name_in_value(&field.value, schema, next_owner, offset) {
                return Some(result);
            }
        }
    }
    None
}

fn field_name_in_value<'a>(
    value: &'a CfdValue,
    schema: Option<&CftSchema>,
    owner_type: String,
    offset: usize,
) -> Option<(String, &'a str)> {
    match value {
        CfdValue::Block(block) => {
            let owner_type = block
                .type_marker
                .as_ref()
                .map_or(owner_type, |(name, _)| name.clone());
            for field in &block.fields {
                let result = field_name_in_fields(
                    std::slice::from_ref(field),
                    schema,
                    owner_type.clone(),
                    offset,
                );
                if result.is_some() {
                    return result;
                }
            }
            None
        }
        CfdValue::Array(items, _) => {
            for item in items {
                if let Some(result) = field_name_in_value(item, schema, owner_type.clone(), offset)
                {
                    return Some(result);
                }
            }
            None
        }
        _ => None,
    }
}

fn named_type_name(ty: &CftValueType) -> Option<&str> {
    match ty {
        CftValueType::Object(name) => Some(name),
        _ => None,
    }
}

/// Definition: return the expected schema type and key under a reference.
pub fn definition_ref_target(
    ast: &CfdAst,
    schema: Option<&CftSchema>,
    offset: usize,
) -> Option<(String, String)> {
    let schema = schema?;
    for record in &ast.records {
        for field in &record.fields {
            if let Some(target) = ref_target_in_field(field, schema, &record.type_name, offset) {
                return Some(target);
            }
        }
    }
    None
}

fn ref_target_in_field(
    field: &CfdField,
    schema: &CftSchema,
    owner_type: &str,
    offset: usize,
) -> Option<(String, String)> {
    let owner = schema.resolve_type(owner_type)?;
    let field_type = &owner
        .all_fields()
        .find(|candidate| candidate.name.as_str() == field.name)?
        .value_type;
    ref_target_in_value(&field.value, schema, field_type, offset)
}

fn ref_target_in_value(
    value: &CfdValue,
    schema: &CftSchema,
    expected_type: &CftValueType,
    offset: usize,
) -> Option<(String, String)> {
    match value {
        CfdValue::Ref(r) => {
            if span_contains(r.key.1, offset) {
                reference_target_type(expected_type)
                    .map(|target_type| (target_type.to_string(), r.key.0.clone()))
            } else {
                None
            }
        }
        CfdValue::Block(block) => {
            if let CftValueType::Dict(_, value_type) = expected_type {
                for field in &block.fields {
                    if let Some(target) =
                        ref_target_in_value(&field.value, schema, value_type, offset)
                    {
                        return Some(target);
                    }
                }
                return None;
            }
            let owner_type = block
                .type_marker
                .as_ref()
                .map(|(name, _)| name.as_str())
                .or_else(|| reference_target_type(expected_type))?;
            for field in &block.fields {
                if let Some(target) = ref_target_in_field(field, schema, owner_type, offset) {
                    return Some(target);
                }
            }
            None
        }
        CfdValue::Array(items, _) => {
            let CftValueType::Array(item_type) = expected_type else {
                return None;
            };
            for item in items {
                if let Some(target) = ref_target_in_value(item, schema, item_type, offset) {
                    return Some(target);
                }
            }
            None
        }
        CfdValue::OptionSome(value, _) => {
            let CftValueType::Option(inner) = expected_type else {
                return None;
            };
            ref_target_in_value(value, schema, inner, offset)
        }
        CfdValue::ResultOk(value, _) => {
            let CftValueType::Result(ok, _) = expected_type else {
                return None;
            };
            ref_target_in_value(value, schema, ok, offset)
        }
        CfdValue::ResultErr(value, _) => {
            let CftValueType::Result(_, error) = expected_type else {
                return None;
            };
            ref_target_in_value(value, schema, error, offset)
        }
        _ => None,
    }
}

fn reference_target_type(ty: &CftValueType) -> Option<&str> {
    match ty {
        CftValueType::Object(name) | CftValueType::RecordRef(name) => Some(name),
        _ => None,
    }
}
