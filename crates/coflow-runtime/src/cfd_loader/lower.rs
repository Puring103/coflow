use crate::data_model::{
    DimensionValueDraft, LoadedDictKeyDraft, LoadedFieldReference, LoadedFormatSegment,
    LoadedFormattedString, LoadedRecordDraft, LoadedValueDraft,
};
use coflow_language::cfd::{
    CfdAst, CfdBitExpr, CfdBitExprKind, CfdBitOp, CfdField, CfdFormatSegment, CfdRecord, CfdValue,
};
use coflow_language::cft::{
    CftSchema, CftValueType, DimensionName, FieldName, RecordKey, TypeName, VariantName,
};
use coflow_language::lexical::record_key_ident_error;
use coflow_language::source::Span;
use std::collections::{BTreeMap, BTreeSet};

use super::{CfdTextDiagnostic, CfdTextDiagnostics, CfdTextErrorCode, CfdTextSpan};

#[derive(Debug, Clone)]
pub(super) struct ParsedLoadedRecordDraft {
    pub(super) record: LoadedRecordDraft,
    pub(super) span: CfdTextSpan,
}

#[cfg(test)]
pub(super) fn lower_records(
    schema: &CftSchema,
    ast: &CfdAst,
) -> Result<Vec<ParsedLoadedRecordDraft>, CfdTextDiagnostics> {
    let (records, diagnostics) = lower_records_with_mode(schema, ast, false);
    finish(records, diagnostics)
}

pub(super) fn lower_records_partial(
    schema: &CftSchema,
    ast: &CfdAst,
) -> (Vec<ParsedLoadedRecordDraft>, Vec<CfdTextDiagnostic>) {
    lower_records_with_mode(schema, ast, true)
}

pub(super) fn lower_dimensions(
    schema: &CftSchema,
    ast: &CfdAst,
) -> (
    Vec<(DimensionValueDraft, CfdTextSpan)>,
    Vec<CfdTextDiagnostic>,
) {
    let mut values = Vec::new();
    let mut diagnostics = Vec::new();
    for block in &ast.dimensions {
        let (Ok(dimension), Ok(variant)) = (
            DimensionName::new(block.dimension.clone()),
            VariantName::new(block.variant.clone()),
        ) else {
            diagnostics.extend(
                error(
                    CfdTextErrorCode::TypeMismatch,
                    "invalid dimension or variant name",
                    block.span,
                )
                .diagnostics,
            );
            continue;
        };
        if schema.resolve_dimension(dimension.as_str()).is_none() {
            diagnostics.extend(
                error(
                    CfdTextErrorCode::TypeMismatch,
                    format!("unknown dimension `{dimension}`"),
                    block.dimension_span,
                )
                .diagnostics,
            );
            continue;
        }
        for record in &block.records {
            let lowered = match lower_record(schema, record, true) {
                Ok(lowered) => lowered.record,
                Err(error) => {
                    diagnostics.extend(error.diagnostics);
                    continue;
                }
            };
            // 覆盖只携带明确写出的字段，不补齐基础记录的默认值和必填字段。
            for field in &record.fields {
                let Some(value) = lowered.fields.get(&field.name) else {
                    continue;
                };
                let (Ok(source_type), Ok(source_key), Ok(name)) = (
                    TypeName::new(lowered.actual_type.clone()),
                    RecordKey::new(lowered.key.clone()),
                    FieldName::new(field.name.clone()),
                ) else {
                    continue;
                };
                values.push((
                    DimensionValueDraft {
                        source_type,
                        source_key,
                        field: name,
                        dimension: dimension.clone(),
                        variant: variant.clone(),
                        value: value.clone(),
                        origin: crate::RecordOrigin::None,
                    },
                    text_span(field.span),
                ));
            }
        }
    }
    (values, diagnostics)
}

fn lower_records_with_mode(
    schema: &CftSchema,
    ast: &CfdAst,
    preserve_repairable_values: bool,
) -> (Vec<ParsedLoadedRecordDraft>, Vec<CfdTextDiagnostic>) {
    let mut records = Vec::with_capacity(ast.records.len());
    let mut diagnostics = Vec::new();
    for record in &ast.records {
        match lower_record(schema, record, preserve_repairable_values) {
            Ok(record) => records.push(record),
            Err(error) => diagnostics.extend(error.diagnostics),
        }
    }
    (records, diagnostics)
}

fn lower_record(
    schema: &CftSchema,
    record: &CfdRecord,
    preserve_repairable_values: bool,
) -> Result<ParsedLoadedRecordDraft, CfdTextDiagnostics> {
    validate_record_key(&record.key, record.key_span)?;
    let type_name = record.type_name.clone();
    if let Some((group_type, span)) = &record.group_type {
        validate_group_type(schema, group_type, *span)?;
        validate_actual_type(schema, group_type, &type_name, record.type_span)?;
    } else {
        validate_record_type(schema, &type_name, record.type_span)?;
    }
    let fields = lower_object_fields(
        schema,
        &type_name,
        &record.fields,
        preserve_repairable_values,
    )?;
    Ok(ParsedLoadedRecordDraft {
        record: LoadedRecordDraft::new(record.key.clone(), type_name, fields),
        span: text_span(record.span),
    })
}

fn lower_object_fields(
    schema: &CftSchema,
    type_name: &str,
    fields: &[CfdField],
    preserve_repairable_values: bool,
) -> Result<BTreeMap<String, LoadedValueDraft>, CfdTextDiagnostics> {
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
    let mut values = BTreeMap::new();
    let mut seen = BTreeSet::new();
    let mut diagnostics = Vec::new();
    for field in fields {
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
        match lower_value_resolved(
            schema,
            &field.value,
            &meta.value_type,
            preserve_repairable_values,
        ) {
            Ok(value) => {
                values.insert(field.name.clone(), value);
            }
            Err(error) => diagnostics.extend(error.diagnostics),
        }
    }
    finish(values, diagnostics)
}

pub(super) fn lower_value_resolved(
    schema: &CftSchema,
    value: &CfdValue,
    ty: &CftValueType,
    preserve_repairable_values: bool,
) -> Result<LoadedValueDraft, CfdTextDiagnostics> {
    if let CfdValue::Scalar(name, span) = value {
        if let Some(constant) = schema.resolve_const(name) {
            return super::constants::lower_constant(schema, &constant.value, ty, *span);
        }
    }
    match ty {
        CftValueType::Int => lower_int(value),
        CftValueType::Float => lower_float(value),
        CftValueType::Bool => lower_bool(value),
        CftValueType::String => lower_string(value),
        CftValueType::FormattedString => lower_formatted_string(value),
        CftValueType::Enum(name) => lower_enum(schema, value, name, preserve_repairable_values),
        CftValueType::Object(name) => lower_object(schema, value, name, preserve_repairable_values),
        CftValueType::RecordRef(name) => lower_ref(schema, value, name),
        CftValueType::Array(inner) => lower_array(schema, value, inner, preserve_repairable_values),
        CftValueType::Dict(key, item) => {
            lower_dict(schema, value, key, item, preserve_repairable_values)
        }
        CftValueType::Option(inner) => match value {
            CfdValue::OptionNone(_) => Ok(LoadedValueDraft::OptionNone),
            CfdValue::OptionSome(value, _) => {
                lower_value_resolved(schema, value, inner, preserve_repairable_values)
                    .map(|value| LoadedValueDraft::OptionSome(Box::new(value)))
            }
            value => lower_value_resolved(schema, value, inner, preserve_repairable_values)
                .map(|value| LoadedValueDraft::OptionSome(Box::new(value))),
        },
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
    match text {
        "true" => Ok(LoadedValueDraft::Bool(true)),
        "false" => Ok(LoadedValueDraft::Bool(false)),
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

fn lower_formatted_string(value: &CfdValue) -> Result<LoadedValueDraft, CfdTextDiagnostics> {
    match value {
        CfdValue::FormattedString(value) => {
            Ok(LoadedValueDraft::FormattedString(LoadedFormattedString {
                source: value.source.clone(),
                segments: value
                    .segments
                    .iter()
                    .map(|segment| match segment {
                        CfdFormatSegment::Text(text) => LoadedFormatSegment::Text(text.clone()),
                        CfdFormatSegment::Reference(reference) => {
                            LoadedFormatSegment::Reference(LoadedFieldReference {
                                type_name: reference.type_name.clone(),
                                key: reference.key.clone(),
                                path: reference.path.clone(),
                            })
                        }
                    })
                    .collect(),
            }))
        }
        _ => Err(error(
            CfdTextErrorCode::TypeMismatch,
            "expected fstring",
            value.span(),
        )),
    }
}

fn enum_variant(expected_enum: &str, raw: &str, span: Span) -> Result<String, CfdTextDiagnostics> {
    let Some((owner, variant)) = raw.rsplit_once("::") else {
        return Ok(raw.to_string());
    };
    if owner != expected_enum || variant.is_empty() {
        return Err(error(
            CfdTextErrorCode::InvalidEnumVariant,
            format!("expected `{expected_enum}` enum value, found `{raw}`"),
            span,
        ));
    }
    Ok(variant.to_string())
}

fn lower_enum(
    schema: &CftSchema,
    value: &CfdValue,
    enum_name: &str,
    preserve_repairable_values: bool,
) -> Result<LoadedValueDraft, CfdTextDiagnostics> {
    let schema_enum = schema.resolve_enum(enum_name).ok_or_else(|| {
        error(
            CfdTextErrorCode::InvalidEnumVariant,
            format!("unknown enum `{enum_name}`"),
            value.span(),
        )
    })?;
    if schema_enum.is_flag {
        let flag_value = match value {
            CfdValue::Scalar(raw, span) => lower_flag_operand(schema, enum_name, raw, *span)?,
            CfdValue::BitExpr(expr) => lower_flag_expr(schema, enum_name, expr)?,
            _ => {
                return Err(error(
                    CfdTextErrorCode::TypeMismatch,
                    format!("expected `{enum_name}` flag value"),
                    value.span(),
                ));
            }
        };
        validate_flag_mask(schema, enum_name, flag_value, value.span())?;
        return Ok(LoadedValueDraft::enum_value(enum_name, flag_value));
    }

    let (raw, span) = scalar(value, "enum value")?;
    let variant = enum_variant(enum_name, raw, span)?;
    let valid = schema.resolve_enum(enum_name).is_some_and(|schema_enum| {
        schema_enum
            .variants
            .iter()
            .any(|candidate| candidate.name.as_str() == variant.as_str())
    });
    if !valid {
        if preserve_repairable_values {
            return Ok(LoadedValueDraft::enum_variant(enum_name, variant));
        }
        return Err(error(
            CfdTextErrorCode::InvalidEnumVariant,
            format!("unknown enum variant `{enum_name}::{variant}`"),
            span,
        ));
    }
    Ok(LoadedValueDraft::enum_variant(enum_name, variant))
}

fn lower_flag_expr(
    schema: &CftSchema,
    enum_name: &str,
    expr: &CfdBitExpr,
) -> Result<i64, CfdTextDiagnostics> {
    match &expr.kind {
        CfdBitExprKind::Value(raw) => lower_flag_operand(schema, enum_name, raw, expr.span),
        CfdBitExprKind::Binary { op, lhs, rhs } => {
            let lhs = lower_flag_expr(schema, enum_name, lhs)?;
            let rhs = lower_flag_expr(schema, enum_name, rhs)?;
            Ok(match op {
                CfdBitOp::Or => lhs | rhs,
                CfdBitOp::Xor => lhs ^ rhs,
                CfdBitOp::And => lhs & rhs,
            })
        }
    }
}

fn lower_flag_operand(
    schema: &CftSchema,
    enum_name: &str,
    raw: &str,
    span: Span,
) -> Result<i64, CfdTextDiagnostics> {
    if let Ok(value) = raw.parse::<i64>() {
        validate_flag_mask(schema, enum_name, value, span)?;
        return Ok(value);
    }

    let variant = enum_variant(enum_name, raw, span)?;
    schema
        .enum_variant_value(enum_name, &variant)
        .ok_or_else(|| {
            error(
                CfdTextErrorCode::InvalidEnumVariant,
                format!("unknown enum variant `{enum_name}::{variant}`"),
                span,
            )
        })
}

fn validate_flag_mask(
    schema: &CftSchema,
    enum_name: &str,
    value: i64,
    span: Span,
) -> Result<(), CfdTextDiagnostics> {
    let declared_mask = schema.resolve_enum(enum_name).map_or(0, |schema_enum| {
        schema_enum
            .variants
            .iter()
            .fold(0_i64, |mask, variant| mask | variant.value)
    });
    if value < 0 {
        return Err(error(
            CfdTextErrorCode::InvalidEnumVariant,
            format!("flag enum `{enum_name}` value must be nonnegative"),
            span,
        ));
    }
    if value & !declared_mask != 0 {
        return Err(error(
            CfdTextErrorCode::InvalidEnumVariant,
            format!("flag enum `{enum_name}` value {value} contains undeclared bits"),
            span,
        ));
    }
    Ok(())
}

fn lower_object(
    schema: &CftSchema,
    value: &CfdValue,
    expected_type: &str,
    preserve_repairable_values: bool,
) -> Result<LoadedValueDraft, CfdTextDiagnostics> {
    match value {
        CfdValue::Block(block) => {
            let (actual_type, declared) = if let Some((actual_type, span)) = &block.type_marker {
                if !schema
                    .resolve_type(actual_type)
                    .is_some_and(|ty| ty.is_data())
                    || !schema.is_assignable(actual_type, expected_type)
                {
                    return Err(error(
                        CfdTextErrorCode::ObjectTypeMismatch,
                        "inline value requires assignable data",
                        *span,
                    ));
                }
                (actual_type.clone(), false)
            } else {
                (expected_type.to_string(), true)
            };
            let fields = lower_object_fields(
                schema,
                &actual_type,
                &block.fields,
                preserve_repairable_values,
            )?;
            Ok(if declared {
                LoadedValueDraft::object_with_declared_type(fields)
            } else {
                LoadedValueDraft::object(actual_type, fields)
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
    schema: &CftSchema,
    value: &CfdValue,
    expected_type: &str,
) -> Result<LoadedValueDraft, CfdTextDiagnostics> {
    let CfdValue::Ref(reference) = value else {
        return Err(error(
            CfdTextErrorCode::Syntax,
            "invalid record reference",
            value.span(),
        ));
    };
    if let Some((type_name, span)) = &reference.type_name {
        validate_actual_type(schema, expected_type, type_name, *span)?;
    }
    validate_record_key(&reference.key.0, reference.key.1)?;
    Ok(if let Some((type_name, _)) = &reference.type_name {
        LoadedValueDraft::TypedRecordRef {
            type_name: type_name.clone(),
            key: reference.key.0.clone(),
        }
    } else {
        LoadedValueDraft::record_ref(reference.key.0.clone())
    })
}

fn lower_array(
    schema: &CftSchema,
    value: &CfdValue,
    inner: &CftValueType,
    preserve_repairable_values: bool,
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
        let result = lower_value_resolved(schema, item, inner, preserve_repairable_values);
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
    preserve_repairable_values: bool,
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
    let mut entries = Vec::new();
    let mut diagnostics = Vec::new();
    for field in &block.fields {
        let key = lower_dict_key(schema, &field.name, field.name_span, key_type);
        let value =
            lower_value_resolved(schema, &field.value, value_type, preserve_repairable_values);
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
    finish(LoadedValueDraft::dict(entries), diagnostics)
}

fn lower_dict_key(
    schema: &CftSchema,
    raw: &str,
    span: Span,
    ty: &CftValueType,
) -> Result<LoadedDictKeyDraft, CfdTextDiagnostics> {
    match ty {
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
            if schema
                .resolve_enum(enum_name)
                .is_some_and(|meta| meta.is_flag)
                && raw.parse::<i64>().is_ok()
            {
                let value = lower_flag_operand(schema, enum_name, raw, span)?;
                return Ok(LoadedDictKeyDraft::EnumValue {
                    enum_name: enum_name.to_string(),
                    value,
                });
            }
            let variant = enum_variant(enum_name, raw, span)?;
            let valid = schema.resolve_enum(enum_name).is_some_and(|schema_enum| {
                schema_enum
                    .variants
                    .iter()
                    .any(|candidate| candidate.name.as_str() == variant.as_str())
            });
            if valid {
                Ok(LoadedDictKeyDraft::enum_variant(
                    enum_name.as_str(),
                    variant,
                ))
            } else {
                Err(error(
                    CfdTextErrorCode::InvalidEnumVariant,
                    format!("unknown enum variant `{enum_name}::{variant}`"),
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
    if schema
        .resolve_type(type_name)
        .is_some_and(|ty| ty.is_table())
    {
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
    if schema_type.is_data() {
        return Err(error(
            CfdTextErrorCode::TypeMismatch,
            "data cannot be a top-level record",
            span,
        ));
    }
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
    diagnostics: Vec<coflow_language::cfd::CfdSyntaxDiagnostic>,
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

pub(super) fn error(
    code: CfdTextErrorCode,
    message: impl Into<String>,
    span: Span,
) -> CfdTextDiagnostics {
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
