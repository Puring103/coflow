use coflow_cfd::{CfdAst, CfdBlockEntry, CfdRecord, CfdValue};
use coflow_cft::{record_key_ident_error, CftSchema, CftValueType, Span};
use coflow_data_model::{LoadedDictKeyDraft, LoadedRecordDraft, LoadedValueDraft};
use std::collections::{BTreeMap, BTreeSet};

use crate::{CfdTextDiagnostic, CfdTextDiagnostics, CfdTextErrorCode, CfdTextSpan};

#[derive(Debug, Clone)]
pub(super) struct ParsedLoadedRecordDraft {
    pub(super) record: LoadedRecordDraft,
    pub(super) span: CfdTextSpan,
}

pub(super) fn lower_records(
    schema: &CftSchema,
    ast: &CfdAst,
) -> Result<Vec<ParsedLoadedRecordDraft>, CfdTextDiagnostics> {
    let mut records = Vec::with_capacity(ast.records.len());
    let mut diagnostics = Vec::new();
    for record in &ast.records {
        match lower_record(schema, record) {
            Ok(record) => records.push(record),
            Err(error) => diagnostics.extend(error.diagnostics),
        }
    }
    finish(records, diagnostics)
}

fn lower_record(
    schema: &CftSchema,
    record: &CfdRecord,
) -> Result<ParsedLoadedRecordDraft, CfdTextDiagnostics> {
    validate_record_key(&record.key, record.key_span)?;
    if let Some((group_type, span)) = &record.group_type {
        validate_group_type(schema, group_type, *span)?;
        validate_actual_type(schema, group_type, &record.type_name, record.type_span)?;
    } else {
        validate_record_type(schema, &record.type_name, record.type_span)?;
    }
    let fields = lower_object_entries(schema, &record.type_name, &record.entries)?;
    Ok(ParsedLoadedRecordDraft {
        record: LoadedRecordDraft::with_spreads(
            record.key.clone(),
            record.type_name.clone(),
            fields.spreads,
            fields.fields,
        ),
        span: text_span(record.span),
    })
}

struct ObjectFields {
    spreads: Vec<LoadedValueDraft>,
    fields: BTreeMap<String, LoadedValueDraft>,
}

fn lower_object_entries(
    schema: &CftSchema,
    type_name: &str,
    entries: &[CfdBlockEntry],
) -> Result<ObjectFields, CfdTextDiagnostics> {
    let schema_type = schema.resolve_type(type_name).ok_or_else(|| {
        error(
            CfdTextErrorCode::UnknownType,
            format!("unknown type `{type_name}`"),
            Span::default(),
        )
    })?;
    let fields_by_name = schema_type
        .all_fields()
        .map(|field| (field.name.as_str(), field))
        .collect::<BTreeMap<_, _>>();
    let mut spreads = Vec::new();
    let mut values = BTreeMap::new();
    let mut seen = BTreeSet::new();
    let mut diagnostics = Vec::new();
    for entry in entries {
        match entry {
            CfdBlockEntry::Spread(value, _) => match lower_spread(
                schema,
                value,
                &CftValueType::Object(schema_type.name.clone()),
            ) {
                Ok(value) => spreads.push(value),
                Err(error) => diagnostics.extend(error.diagnostics),
            },
            CfdBlockEntry::Field(field) => {
                if field.name == "id" {
                    diagnostics.extend(
                        error(
                            CfdTextErrorCode::ReservedIdField,
                            "`id` is reserved for the record key",
                            field.name_span,
                        )
                        .diagnostics,
                    );
                    continue;
                }
                if !seen.insert(field.name.clone()) {
                    diagnostics.extend(
                        error(
                            CfdTextErrorCode::DuplicateField,
                            format!("duplicate field `{}`", field.name),
                            field.name_span,
                        )
                        .diagnostics,
                    );
                    continue;
                }
                let Some(meta) = fields_by_name.get(field.name.as_str()) else {
                    diagnostics.extend(
                        error(
                            CfdTextErrorCode::UnknownField,
                            format!("unknown field `{}` on type `{type_name}`", field.name),
                            field.name_span,
                        )
                        .diagnostics,
                    );
                    continue;
                };
                match lower_value(schema, &field.value, &meta.value_type) {
                    Ok(value) => {
                        values.insert(field.name.clone(), value);
                    }
                    Err(error) => diagnostics.extend(error.diagnostics),
                }
            }
        }
    }
    finish(
        ObjectFields {
            spreads,
            fields: values,
        },
        diagnostics,
    )
}

pub(crate) fn lower_value(
    schema: &CftSchema,
    value: &CfdValue,
    ty: &CftValueType,
) -> Result<LoadedValueDraft, CfdTextDiagnostics> {
    if let CftValueType::Nullable(inner) = ty {
        if matches!(value, CfdValue::Null(_)) {
            return Ok(LoadedValueDraft::Null);
        }
        return lower_value(schema, value, inner);
    }
    if matches!(value, CfdValue::Null(_)) {
        return Err(error(
            CfdTextErrorCode::TypeMismatch,
            "unexpected null value",
            value.span(),
        ));
    }
    match ty {
        CftValueType::Int => lower_int(value),
        CftValueType::Float => lower_float(value),
        CftValueType::Bool => lower_bool(value),
        CftValueType::String => lower_string(value),
        CftValueType::Enum(name) => lower_enum(schema, value, name),
        CftValueType::Object(name) => lower_object(schema, value, name),
        CftValueType::RecordRef(name) => lower_ref(value, name),
        CftValueType::Array(inner) => lower_array(schema, value, inner),
        CftValueType::Dict(key, item) => lower_dict(schema, value, key, item),
        CftValueType::Nullable(inner) => lower_value(schema, value, inner),
    }
}

fn scalar<'a>(value: &'a CfdValue, expected: &str) -> Result<(&'a str, Span), CfdTextDiagnostics> {
    let CfdValue::Scalar(text, span) = value else {
        return Err(error(
            CfdTextErrorCode::TypeMismatch,
            format!("expected {expected}"),
            value.span(),
        ));
    };
    Ok((text, *span))
}

fn lower_int(value: &CfdValue) -> Result<LoadedValueDraft, CfdTextDiagnostics> {
    let (text, span) = scalar(value, "int")?;
    text.parse::<i64>()
        .map(LoadedValueDraft::Int)
        .map_err(|_| error(CfdTextErrorCode::TypeMismatch, "expected int", span))
}

fn lower_float(value: &CfdValue) -> Result<LoadedValueDraft, CfdTextDiagnostics> {
    let (text, span) = scalar(value, "float")?;
    let number = text
        .parse::<f64>()
        .map_err(|_| error(CfdTextErrorCode::TypeMismatch, "expected float", span))?;
    if !number.is_finite() {
        return Err(error(
            CfdTextErrorCode::TypeMismatch,
            "float value must be finite",
            span,
        ));
    }
    Ok(LoadedValueDraft::Float(number))
}

fn lower_bool(value: &CfdValue) -> Result<LoadedValueDraft, CfdTextDiagnostics> {
    let (text, span) = scalar(value, "bool")?;
    match text.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "y" => Ok(LoadedValueDraft::Bool(true)),
        "false" | "0" | "no" | "n" => Ok(LoadedValueDraft::Bool(false)),
        _ => Err(error(CfdTextErrorCode::TypeMismatch, "expected bool", span)),
    }
}

fn lower_string(value: &CfdValue) -> Result<LoadedValueDraft, CfdTextDiagnostics> {
    match value {
        CfdValue::QuotedString(text, _) => Ok(LoadedValueDraft::String(text.clone())),
        _ => Err(error(
            CfdTextErrorCode::TypeMismatch,
            "expected string",
            value.span(),
        )),
    }
}

fn lower_enum(
    schema: &CftSchema,
    value: &CfdValue,
    enum_name: &str,
) -> Result<LoadedValueDraft, CfdTextDiagnostics> {
    let (raw, span) = scalar(value, "enum value")?;
    let variant = raw
        .strip_prefix(enum_name)
        .and_then(|rest| rest.strip_prefix('.'))
        .unwrap_or(raw);
    let valid = schema.resolve_enum(enum_name).is_some_and(|schema_enum| {
        schema_enum
            .variants
            .iter()
            .any(|candidate| candidate.name.as_str() == variant)
    });
    if !valid {
        return Err(error(
            CfdTextErrorCode::InvalidEnumVariant,
            format!("unknown enum variant `{enum_name}.{variant}`"),
            span,
        ));
    }
    Ok(LoadedValueDraft::enum_variant(enum_name, variant))
}

fn lower_object(
    schema: &CftSchema,
    value: &CfdValue,
    expected_type: &str,
) -> Result<LoadedValueDraft, CfdTextDiagnostics> {
    match value {
        CfdValue::Block(block) => {
            let (actual_type, declared) = if let Some((actual_type, span)) = &block.type_marker {
                validate_actual_type(schema, expected_type, actual_type, *span)?;
                (actual_type.as_str(), false)
            } else {
                (expected_type, true)
            };
            let fields = lower_object_entries(schema, actual_type, &block.entries)?;
            Ok(match (declared, fields.spreads.is_empty()) {
                (true, true) => LoadedValueDraft::object_with_declared_type(fields.fields),
                (true, false) => LoadedValueDraft::object_spread(fields.spreads, fields.fields),
                (false, true) => LoadedValueDraft::object(actual_type, fields.fields),
                (false, false) => LoadedValueDraft::object_spread_with_actual_type(
                    actual_type,
                    fields.spreads,
                    fields.fields,
                ),
            })
        }
        CfdValue::Ref(_) => Err(error(
            CfdTextErrorCode::TypeMismatch,
            "inline object fields do not accept record references",
            value.span(),
        )),
        CfdValue::Scalar(key, span) => Err(error(
            CfdTextErrorCode::ReferenceNeedsMarker,
            format!("object reference `{key}` must be written as `&{key}`"),
            *span,
        )),
        _ => Err(error(
            CfdTextErrorCode::TypeMismatch,
            format!("expected object `{expected_type}`"),
            value.span(),
        )),
    }
}

fn lower_ref(
    value: &CfdValue,
    _expected_type: &str,
) -> Result<LoadedValueDraft, CfdTextDiagnostics> {
    let CfdValue::Ref(reference) = value else {
        return Err(error(
            CfdTextErrorCode::Syntax,
            "invalid record reference",
            value.span(),
        ));
    };
    validate_record_key(&reference.key.0, reference.key.1)?;
    Ok(LoadedValueDraft::record_ref(reference.key.0.clone()))
}

fn lower_array(
    schema: &CftSchema,
    value: &CfdValue,
    inner: &CftValueType,
) -> Result<LoadedValueDraft, CfdTextDiagnostics> {
    let CfdValue::Array(items, _) = value else {
        return Err(error(
            CfdTextErrorCode::TypeMismatch,
            "expected array",
            value.span(),
        ));
    };
    let mut lowered = Vec::with_capacity(items.len());
    let mut diagnostics = Vec::new();
    for item in items {
        let result = if matches!(item, CfdValue::Spread(_, _)) {
            Err(error(
                CfdTextErrorCode::Syntax,
                "array spreads are not supported",
                item.span(),
            ))
        } else {
            lower_value(schema, item, inner)
        };
        match result {
            Ok(value) => lowered.push(value),
            Err(error) => diagnostics.extend(error.diagnostics),
        }
    }
    finish(LoadedValueDraft::Array(lowered), diagnostics)
}

fn lower_dict(
    schema: &CftSchema,
    value: &CfdValue,
    key_type: &CftValueType,
    value_type: &CftValueType,
) -> Result<LoadedValueDraft, CfdTextDiagnostics> {
    let CfdValue::Block(block) = value else {
        return Err(error(
            CfdTextErrorCode::TypeMismatch,
            "expected dict",
            value.span(),
        ));
    };
    if block.type_marker.is_some() {
        return Err(error(
            CfdTextErrorCode::TypeMismatch,
            "dict values do not accept type markers",
            block.span,
        ));
    }
    let dict_type = CftValueType::Dict(Box::new(key_type.clone()), Box::new(value_type.clone()));
    let mut spreads = Vec::new();
    let mut entries = Vec::new();
    let mut diagnostics = Vec::new();
    for entry in &block.entries {
        match entry {
            CfdBlockEntry::Spread(value, _) => match lower_spread(schema, value, &dict_type) {
                Ok(value) => spreads.push(value),
                Err(error) => diagnostics.extend(error.diagnostics),
            },
            CfdBlockEntry::Field(field) => {
                let key = lower_dict_key(schema, &field.name, field.name_span, key_type);
                let value = lower_value(schema, &field.value, value_type);
                match (key, value) {
                    (Ok(key), Ok(value)) => entries.push((key, value)),
                    (key, value) => {
                        if let Err(error) = key {
                            diagnostics.extend(error.diagnostics);
                        }
                        if let Err(error) = value {
                            diagnostics.extend(error.diagnostics);
                        }
                    }
                }
            }
        }
    }
    let value = if spreads.is_empty() {
        LoadedValueDraft::dict(entries)
    } else {
        LoadedValueDraft::dict_spread(spreads, entries)
    };
    finish(value, diagnostics)
}

fn lower_dict_key(
    schema: &CftSchema,
    raw: &str,
    span: Span,
    ty: &CftValueType,
) -> Result<LoadedDictKeyDraft, CfdTextDiagnostics> {
    match ty.non_nullable() {
        CftValueType::String => Ok(LoadedDictKeyDraft::String(raw.to_string())),
        CftValueType::Int => raw
            .parse::<i64>()
            .map(LoadedDictKeyDraft::Int)
            .map_err(|_| {
                error(
                    CfdTextErrorCode::TypeMismatch,
                    "expected int dict key",
                    span,
                )
            }),
        CftValueType::Enum(enum_name) => {
            let variant = raw
                .strip_prefix(enum_name.as_str())
                .and_then(|rest| rest.strip_prefix('.'))
                .unwrap_or(raw);
            let valid = schema.resolve_enum(enum_name).is_some_and(|schema_enum| {
                schema_enum
                    .variants
                    .iter()
                    .any(|candidate| candidate.name.as_str() == variant)
            });
            if valid {
                Ok(LoadedDictKeyDraft::enum_variant(
                    enum_name.as_str(),
                    variant,
                ))
            } else {
                Err(error(
                    CfdTextErrorCode::InvalidEnumVariant,
                    format!("unknown enum variant `{enum_name}.{variant}`"),
                    span,
                ))
            }
        }
        _ => Err(error(
            CfdTextErrorCode::TypeMismatch,
            "invalid dict key type",
            span,
        )),
    }
}

fn lower_spread(
    schema: &CftSchema,
    value: &CfdValue,
    ty: &CftValueType,
) -> Result<LoadedValueDraft, CfdTextDiagnostics> {
    if matches!(value, CfdValue::Ref(_)) {
        return lower_ref(value, "");
    }
    lower_value(schema, value, ty)
}

fn validate_record_key(key: &str, span: Span) -> Result<(), CfdTextDiagnostics> {
    if let Some(reason) = record_key_ident_error(key) {
        return Err(error(
            CfdTextErrorCode::Syntax,
            format!("invalid record key `{key}`: {reason}"),
            span,
        ));
    }
    Ok(())
}

fn validate_group_type(
    schema: &CftSchema,
    type_name: &str,
    span: Span,
) -> Result<(), CfdTextDiagnostics> {
    if schema.resolve_type(type_name).is_some() {
        Ok(())
    } else {
        Err(error(
            CfdTextErrorCode::UnknownType,
            format!("unknown type `{type_name}`"),
            span,
        ))
    }
}

fn validate_record_type(
    schema: &CftSchema,
    actual_type: &str,
    span: Span,
) -> Result<(), CfdTextDiagnostics> {
    let Some(schema_type) = schema.resolve_type(actual_type) else {
        return Err(error(
            CfdTextErrorCode::UnknownType,
            format!("unknown type `{actual_type}`"),
            span,
        ));
    };
    if schema_type.is_abstract {
        return Err(error(
            CfdTextErrorCode::AbstractObjectType,
            format!("abstract type `{actual_type}` cannot be instantiated"),
            span,
        ));
    }
    Ok(())
}

fn validate_actual_type(
    schema: &CftSchema,
    expected_type: &str,
    actual_type: &str,
    span: Span,
) -> Result<(), CfdTextDiagnostics> {
    validate_record_type(schema, actual_type, span)?;
    if !schema.is_assignable(actual_type, expected_type) {
        return Err(error(
            CfdTextErrorCode::ObjectTypeMismatch,
            format!("type `{actual_type}` is not assignable to `{expected_type}`"),
            span,
        ));
    }
    Ok(())
}

pub(super) fn syntax_diagnostics(
    diagnostics: Vec<coflow_cfd::CfdSyntaxDiagnostic>,
) -> CfdTextDiagnostics {
    CfdTextDiagnostics {
        diagnostics: diagnostics
            .into_iter()
            .map(|diagnostic| {
                CfdTextDiagnostic::error(
                    CfdTextErrorCode::Syntax,
                    diagnostic.message,
                    text_span(diagnostic.span),
                )
            })
            .collect(),
    }
}

fn error(code: CfdTextErrorCode, message: impl Into<String>, span: Span) -> CfdTextDiagnostics {
    CfdTextDiagnostics::one(CfdTextDiagnostic::error(code, message, text_span(span)))
}

fn finish<T>(value: T, diagnostics: Vec<CfdTextDiagnostic>) -> Result<T, CfdTextDiagnostics> {
    if diagnostics.is_empty() {
        Ok(value)
    } else {
        Err(CfdTextDiagnostics { diagnostics })
    }
}

const fn text_span(span: Span) -> CfdTextSpan {
    CfdTextSpan {
        start: span.start,
        end: span.end,
    }
}
