use coflow_api::{DiagnosticSet, WriteFieldPathSegment};
use coflow_cfd::ast::{CfdBlock, CfdBlockEntry, CfdRecord as AstRecord, CfdValue as AstValue};
use coflow_cft::Span;
use coflow_cft::{CftSchema, CftValueType};

use super::diag;
use super::schema_nav::{
    concrete_type_for_block, dict_key_path_matches, type_after_dict_key_segment,
    type_after_field_segment, type_after_field_segment_for_ref, type_after_index_segment,
};

pub(super) enum WriteTarget {
    Replace {
        span: Span,
        ty: CftValueType,
    },
    InsertTopLevel {
        ty: CftValueType,
    },
    InsertNested {
        block_span: Span,
        depth: usize,
        field_name: String,
        ty: CftValueType,
    },
}

pub(super) fn locate_target(
    schema: &CftSchema,
    actual_type: &str,
    record: &AstRecord,
    path: &[WriteFieldPathSegment],
) -> Result<WriteTarget, DiagnosticSet> {
    let WriteFieldPathSegment::Field(name) = &path[0] else {
        return Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            "top-level path must start with a field name",
        )));
    };
    let Some(field) = find_field_in_record(record, name) else {
        if path.len() > 1 {
            return Err(DiagnosticSet::one(diag(
                "CFD-WRITE",
                format!("top-level field `{name}` not found in record"),
            )));
        }
        let Some(ty) = type_after_field_segment(schema, actual_type, name) else {
            return Err(DiagnosticSet::one(diag(
                "CFD-WRITE",
                format!("field `{name}` not found on type `{actual_type}`"),
            )));
        };
        return Ok(WriteTarget::InsertTopLevel { ty });
    };
    let Some(next_type) = type_after_field_segment(schema, actual_type, name) else {
        return Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            format!("field `{name}` not found on type `{actual_type}`"),
        )));
    };
    if path.len() == 1 {
        return Ok(WriteTarget::Replace {
            span: full_value_span(&field.value),
            ty: next_type,
        });
    }
    locate_target_in_value(schema, &next_type, &field.value, &path[1..], 1)
}

fn full_value_span(value: &AstValue) -> Span {
    if let AstValue::Block(b) = value {
        if let Some((_, tm_span)) = &b.type_marker {
            return Span::new(tm_span.start, b.span.end);
        }
    }
    value.span()
}

fn find_field_in_record<'a>(record: &'a AstRecord, name: &str) -> Option<&'a coflow_cfd::CfdField> {
    record.fields.iter().find(|f| f.name == name).or_else(|| {
        record.entries.iter().find_map(|e| match e {
            CfdBlockEntry::Field(f) if f.name == name => Some(f),
            _ => None,
        })
    })
}

fn locate_target_in_value(
    schema: &CftSchema,
    current_type: &CftValueType,
    value: &AstValue,
    path: &[WriteFieldPathSegment],
    depth: usize,
) -> Result<WriteTarget, DiagnosticSet> {
    if path.is_empty() {
        return Ok(WriteTarget::Replace {
            span: full_value_span(value),
            ty: current_type.clone(),
        });
    }
    match (&path[0], value) {
        (WriteFieldPathSegment::Field(name), AstValue::Block(block)) => {
            let block_type = concrete_type_for_block(
                schema,
                current_type,
                block.type_marker.as_ref().map(|t| t.0.as_str()),
            );
            locate_field_target(schema, &block_type, block, name, path, depth)
        }
        (WriteFieldPathSegment::Index(index), AstValue::Array(items, _)) => {
            locate_array_target(schema, current_type, items, *index, path, depth)
        }
        (WriteFieldPathSegment::DictKey(key), AstValue::Block(block)) => {
            locate_dict_target(schema, current_type, block, key, path, depth)
        }
        _ => Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            format!("cannot navigate path segment {:?} in value", path[0]),
        ))),
    }
}

#[allow(clippy::option_if_let_else)]
fn locate_field_target(
    schema: &CftSchema,
    current_type: &CftValueType,
    block: &CfdBlock,
    name: &str,
    path: &[WriteFieldPathSegment],
    depth: usize,
) -> Result<WriteTarget, DiagnosticSet> {
    let Some(next_type) = type_after_field_segment_for_ref(schema, current_type, name) else {
        return Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            format!("field `{name}` cannot be selected from this value"),
        )));
    };
    let field = block.entries.iter().find_map(|entry| match entry {
        CfdBlockEntry::Field(field) if field.name == name => Some(field),
        _ => None,
    });
    match field {
        Some(field) if path.len() == 1 => Ok(WriteTarget::Replace {
            span: full_value_span(&field.value),
            ty: next_type,
        }),
        Some(field) => {
            locate_target_in_value(schema, &next_type, &field.value, &path[1..], depth + 1)
        }
        None if path.len() == 1 => Ok(WriteTarget::InsertNested {
            block_span: block.span,
            depth,
            field_name: name.to_string(),
            ty: next_type,
        }),
        None => Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            format!(
                "field `{name}` is inherited from a `...spread` and the editor \
                 cannot drill further into it; edit the source record directly"
            ),
        ))),
    }
}

fn locate_array_target(
    schema: &CftSchema,
    current_type: &CftValueType,
    items: &[AstValue],
    index: usize,
    path: &[WriteFieldPathSegment],
    depth: usize,
) -> Result<WriteTarget, DiagnosticSet> {
    let Some(next_type) = type_after_index_segment(current_type) else {
        return Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            format!("array index `{index}` cannot be selected from this value"),
        )));
    };
    let item = items.get(index).ok_or_else(|| {
        DiagnosticSet::one(diag("CFD-WRITE", format!("index {index} out of bounds")))
    })?;
    if path.len() == 1 {
        Ok(WriteTarget::Replace {
            span: full_value_span(item),
            ty: next_type,
        })
    } else {
        locate_target_in_value(schema, &next_type, item, &path[1..], depth + 1)
    }
}

fn locate_dict_target(
    schema: &CftSchema,
    current_type: &CftValueType,
    block: &CfdBlock,
    key: &str,
    path: &[WriteFieldPathSegment],
    depth: usize,
) -> Result<WriteTarget, DiagnosticSet> {
    let Some((key_type, next_type)) = type_after_dict_key_segment(current_type) else {
        return Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            format!("dict key `{key}` cannot be selected from this value"),
        )));
    };
    let Some(field) = block.entries.iter().find_map(|entry| match entry {
        CfdBlockEntry::Field(field) if dict_key_path_matches(&key_type, &field.name, key) => {
            Some(field)
        }
        _ => None,
    }) else {
        return Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            format!("dict key `{key}` not found in source block"),
        )));
    };
    if path.len() == 1 {
        Ok(WriteTarget::Replace {
            span: full_value_span(&field.value),
            ty: next_type,
        })
    } else {
        locate_target_in_value(schema, &next_type, &field.value, &path[1..], depth + 1)
    }
}

pub(super) fn spread_entries_at_path<'a>(
    schema: &CftSchema,
    actual_type: &str,
    record: &'a AstRecord,
    path: &[WriteFieldPathSegment],
) -> Result<&'a [CfdBlockEntry], DiagnosticSet> {
    if path.is_empty() {
        return Ok(record.entries.as_slice());
    }
    let Some(schema_type) = schema.resolve_type(actual_type) else {
        return Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            format!("unknown CFT type `{actual_type}`"),
        )));
    };
    let root_type = CftValueType::Object(schema_type.name.clone());
    let Some((value, value_type)) =
        value_at_spread_path_segment(schema, record.entries.as_slice(), &root_type, &path[0])?
    else {
        return Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            "spread rewrite site was not found",
        )));
    };
    block_entries_at_path(schema, value, &value_type, &path[1..])
}

fn block_entries_at_path<'a>(
    schema: &CftSchema,
    value: &'a AstValue,
    ty: &CftValueType,
    path: &[WriteFieldPathSegment],
) -> Result<&'a [CfdBlockEntry], DiagnosticSet> {
    if path.is_empty() {
        let AstValue::Block(block) = value else {
            return Err(DiagnosticSet::one(diag(
                "CFD-WRITE",
                "spread rewrite site is not an object block",
            )));
        };
        return Ok(block.entries.as_slice());
    }
    match value {
        AstValue::Block(block) => {
            let block_type = concrete_type_for_block(
                schema,
                ty,
                block.type_marker.as_ref().map(|t| t.0.as_str()),
            );
            let Some((next, next_type)) = value_at_spread_path_segment(
                schema,
                block.entries.as_slice(),
                &block_type,
                &path[0],
            )?
            else {
                return Err(DiagnosticSet::one(diag(
                    "CFD-WRITE",
                    "spread rewrite site was not found",
                )));
            };
            block_entries_at_path(schema, next, &next_type, &path[1..])
        }
        AstValue::Array(items, _) => {
            let WriteFieldPathSegment::Index(index) = path[0] else {
                return Err(DiagnosticSet::one(diag(
                    "CFD-WRITE",
                    format!("cannot navigate path segment {:?} in array value", path[0]),
                )));
            };
            let Some(item_type) = type_after_index_segment(ty) else {
                return Err(DiagnosticSet::one(diag(
                    "CFD-WRITE",
                    "array index cannot be selected from this value",
                )));
            };
            let Some(item) = items.get(index) else {
                return Err(DiagnosticSet::one(diag(
                    "CFD-WRITE",
                    format!("index {index} out of bounds while locating spread rewrite site"),
                )));
            };
            block_entries_at_path(schema, item, &item_type, &path[1..])
        }
        _ => Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            format!("cannot navigate path segment {:?} in value", path[0]),
        ))),
    }
}

fn value_at_spread_path_segment<'a>(
    schema: &CftSchema,
    entries: &'a [CfdBlockEntry],
    current_type: &CftValueType,
    segment: &WriteFieldPathSegment,
) -> Result<Option<(&'a AstValue, CftValueType)>, DiagnosticSet> {
    match segment {
        WriteFieldPathSegment::Field(field_name) => {
            let Some(next_type) =
                type_after_field_segment_for_ref(schema, current_type, field_name)
            else {
                return Err(DiagnosticSet::one(diag(
                    "CFD-WRITE",
                    format!("field `{field_name}` cannot be selected from this value"),
                )));
            };
            Ok(entries
                .iter()
                .find_map(|entry| match entry {
                    CfdBlockEntry::Field(field) if field.name == *field_name => Some(&field.value),
                    _ => None,
                })
                .map(|value| (value, next_type)))
        }
        WriteFieldPathSegment::DictKey(key) => {
            let Some((key_type, next_type)) = type_after_dict_key_segment(current_type) else {
                return Err(DiagnosticSet::one(diag(
                    "CFD-WRITE",
                    format!("dict key `{key}` cannot be selected from this value"),
                )));
            };
            Ok(entries
                .iter()
                .find_map(|entry| match entry {
                    CfdBlockEntry::Field(field)
                        if dict_key_path_matches(&key_type, &field.name, key) =>
                    {
                        Some(&field.value)
                    }
                    _ => None,
                })
                .map(|value| (value, next_type)))
        }
        WriteFieldPathSegment::Index(index) => Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            format!("array index `{index}` cannot be selected from an object block"),
        ))),
    }
}
