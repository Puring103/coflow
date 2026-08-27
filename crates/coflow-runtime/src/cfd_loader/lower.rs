use crate::data_model::{
    LoadedDictKeyDraft, LoadedFieldReference, LoadedFormatSegment, LoadedFormattedString,
    LoadedRecordDraft, LoadedValueDraft,
};
use coflow_language::cfd::{
    CfdAst, CfdBitExpr, CfdBitExprKind, CfdBitOp, CfdField, CfdFormatSegment, CfdRecord, CfdValue,
};
use coflow_language::{record_key_ident_error, CftSchema, CftValueType, Span};
use std::collections::{BTreeMap, BTreeSet};

use super::{CfdTextDiagnostic, CfdTextDiagnostics, CfdTextErrorCode, CfdTextSpan};

#[derive(Debug, Clone)]
pub(super) struct ParsedLoadedRecordDraft {
    pub(super) record: LoadedRecordDraft,
    pub(super) span: CfdTextSpan,
}

#[derive(Debug, Clone, Default)]
struct CfdNameResolver {
    namespace: Option<String>,
    uses: BTreeMap<String, String>,
}

impl CfdNameResolver {
    fn new(schema: &CftSchema, ast: &CfdAst) -> Result<Self, CfdTextDiagnostics> {
        let namespace = ast.namespace.as_ref().map(|item| item.path.clone());
        let mut uses = BTreeMap::new();
        let mut diagnostics = Vec::new();
        for declaration in &ast.uses {
            if !symbol_exists(schema, &declaration.path) {
                diagnostics.push(CfdTextDiagnostic::error(
                    CfdTextErrorCode::Syntax,
                    format!("unknown use target `{}`", declaration.path),
                    text_span(declaration.path_span),
                ));
                continue;
            }
            let local_name = declaration.local_name();
            let local_symbol = namespace
                .as_ref()
                .map_or_else(|| local_name.to_string(), |namespace| {
                    format!("{namespace}::{local_name}")
                });
            if symbol_exists(schema, &local_symbol) {
                diagnostics.push(CfdTextDiagnostic::error(
                    CfdTextErrorCode::Syntax,
                    format!("use name `{local_name}` conflicts with `{local_symbol}`"),
                    text_span(declaration.span),
                ));
                continue;
            }
            if let Some(existing) = uses.insert(local_name.to_string(), declaration.path.clone()) {
                diagnostics.push(CfdTextDiagnostic::error(
                    CfdTextErrorCode::Syntax,
                    format!(
                        "use name `{local_name}` refers to both `{existing}` and `{}`",
                        declaration.path
                    ),
                    text_span(declaration.span),
                ));
            }
        }
        finish(Self { namespace, uses }, diagnostics)
    }

    fn root() -> Self {
        Self::default()
    }

    fn resolve(&self, name: &str) -> String {
        if let Some((head, tail)) = name.split_once("::") {
            return self
                .uses
                .get(head)
                .map_or_else(|| name.to_string(), |target| format!("{target}::{tail}"));
        }
        if let Some(target) = self.uses.get(name) {
            return target.clone();
        }
        self.namespace
            .as_ref()
            .map_or_else(|| name.to_string(), |namespace| format!("{namespace}::{name}"))
    }
}

fn symbol_exists(schema: &CftSchema, name: &str) -> bool {
    schema.resolve_type(name).is_some()
        || schema.resolve_enum(name).is_some()
        || schema.resolve_const(name).is_some()
}

pub(super) fn lower_records(
    schema: &CftSchema,
    ast: &CfdAst,
) -> Result<Vec<ParsedLoadedRecordDraft>, CfdTextDiagnostics> {
    let names = CfdNameResolver::new(schema, ast)?;
    let mut records = Vec::with_capacity(ast.records.len());
    let mut diagnostics = Vec::new();
    for record in &ast.records {
        match lower_record(schema, &names, record) {
            Ok(record) => records.push(record),
            Err(error) => diagnostics.extend(error.diagnostics),
        }
    }
    finish(records, diagnostics)
}

fn lower_record(
    schema: &CftSchema,
    names: &CfdNameResolver,
    record: &CfdRecord,
) -> Result<ParsedLoadedRecordDraft, CfdTextDiagnostics> {
    validate_record_key(&record.key, record.key_span)?;
    let type_name = names.resolve(&record.type_name);
    if let Some((group_type, span)) = &record.group_type {
        let group_type = names.resolve(group_type);
        validate_group_type(schema, &group_type, *span)?;
        validate_actual_type(schema, &group_type, &type_name, record.type_span)?;
    } else {
        validate_record_type(schema, &type_name, record.type_span)?;
    }
    let fields = lower_object_fields(schema, names, &type_name, &record.fields)?;
    Ok(ParsedLoadedRecordDraft {
        record: LoadedRecordDraft::new(record.key.clone(), type_name, fields),
        span: text_span(record.span),
    })
}

fn lower_object_fields(
    schema: &CftSchema,
    names: &CfdNameResolver,
    type_name: &str,
    fields: &[CfdField],
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
        match lower_value_resolved(schema, names, &field.value, &meta.value_type) {
            Ok(value) => {
                values.insert(field.name.clone(), value);
            }
            Err(error) => diagnostics.extend(error.diagnostics),
        }
    }
    finish(values, diagnostics)
}

pub(crate) fn lower_value(
    schema: &CftSchema,
    value: &CfdValue,
    ty: &CftValueType,
) -> Result<LoadedValueDraft, CfdTextDiagnostics> {
    lower_value_resolved(schema, &CfdNameResolver::root(), value, ty)
}

fn lower_value_resolved(
    schema: &CftSchema,
    names: &CfdNameResolver,
    value: &CfdValue,
    ty: &CftValueType,
) -> Result<LoadedValueDraft, CfdTextDiagnostics> {
    match ty {
        CftValueType::Int => lower_int(value),
        CftValueType::Float => lower_float(value),
        CftValueType::Bool => lower_bool(value),
        CftValueType::String => lower_string(names, value),
        CftValueType::Enum(name) => lower_enum(schema, names, value, name),
        CftValueType::Object(name) => lower_object(schema, names, value, name),
        CftValueType::RecordRef(name) => lower_ref(schema, names, value, name),
        CftValueType::Array(inner) => lower_array(schema, names, value, inner),
        CftValueType::Dict(key, item) => lower_dict(schema, names, value, key, item),
        CftValueType::Option(inner) => match value {
            CfdValue::OptionNone(_) => Ok(LoadedValueDraft::OptionNone),
            CfdValue::OptionSome(value, _) => lower_value_resolved(schema, names, value, inner)
                .map(|value| LoadedValueDraft::OptionSome(Box::new(value))),
            value => lower_value_resolved(schema, names, value, inner)
                .map(|value| LoadedValueDraft::OptionSome(Box::new(value))),
        },
        CftValueType::Result(ok, error_type) => match value {
            CfdValue::ResultOk(value, _) => lower_value_resolved(schema, names, value, ok)
                .map(|value| LoadedValueDraft::ResultOk(Box::new(value))),
            CfdValue::ResultErr(value, _) => lower_value_resolved(schema, names, value, error_type)
                .map(|value| LoadedValueDraft::ResultErr(Box::new(value))),
            _ => Err(error(
                CfdTextErrorCode::TypeMismatch,
                format!("expected `Ok(...)` or `Err(...)` for `{ty}`"),
                value.span(),
            )),
        },
        CftValueType::Function(_, _) | CftValueType::Unit => Err(error(
            CfdTextErrorCode::TypeMismatch,
            format!("CFD value syntax for `{ty}` is not implemented yet"),
            value.span(),
        )),
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

fn lower_string(
    names: &CfdNameResolver,
    value: &CfdValue,
) -> Result<LoadedValueDraft, CfdTextDiagnostics> {
    match value {
        CfdValue::QuotedString(text, _) => Ok(LoadedValueDraft::String(text.clone())),
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
                                type_name: reference
                                    .type_name
                                    .as_deref()
                                    .map(|name| names.resolve(name)),
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
            "expected string",
            value.span(),
        )),
    }
}

fn enum_variant(
    names: &CfdNameResolver,
    expected_enum: &str,
    raw: &str,
    span: Span,
) -> Result<String, CfdTextDiagnostics> {
    let Some((owner, variant)) = raw.rsplit_once("::") else {
        return Ok(raw.to_string());
    };
    let resolved_owner = names.resolve(owner);
    if resolved_owner != expected_enum || variant.is_empty() {
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
    names: &CfdNameResolver,
    value: &CfdValue,
    enum_name: &str,
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
            CfdValue::Scalar(raw, span) => {
                lower_flag_operand(schema, names, enum_name, raw, *span)?
            }
            CfdValue::BitExpr(expr) => lower_flag_expr(schema, names, enum_name, expr)?,
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
    let variant = enum_variant(names, enum_name, raw, span)?;
    let valid = schema.resolve_enum(enum_name).is_some_and(|schema_enum| {
        schema_enum
            .variants
            .iter()
            .any(|candidate| candidate.name.as_str() == variant.as_str())
    });
    if !valid {
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
    names: &CfdNameResolver,
    enum_name: &str,
    expr: &CfdBitExpr,
) -> Result<i64, CfdTextDiagnostics> {
    match &expr.kind {
        CfdBitExprKind::Value(raw) => {
            lower_flag_operand(schema, names, enum_name, raw, expr.span)
        }
        CfdBitExprKind::Binary { op, lhs, rhs } => {
            let lhs = lower_flag_expr(schema, names, enum_name, lhs)?;
            let rhs = lower_flag_expr(schema, names, enum_name, rhs)?;
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
    names: &CfdNameResolver,
    enum_name: &str,
    raw: &str,
    span: Span,
) -> Result<i64, CfdTextDiagnostics> {
    if let Ok(value) = raw.parse::<i64>() {
        validate_flag_mask(schema, enum_name, value, span)?;
        return Ok(value);
    }

    let variant = enum_variant(names, enum_name, raw, span)?;
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
    names: &CfdNameResolver,
    value: &CfdValue,
    expected_type: &str,
) -> Result<LoadedValueDraft, CfdTextDiagnostics> {
    match value {
        CfdValue::Block(block) => {
            let (actual_type, declared) = if let Some((actual_type, span)) = &block.type_marker {
                let actual_type = names.resolve(actual_type);
                validate_actual_type(schema, expected_type, &actual_type, *span)?;
                (actual_type, false)
            } else {
                (expected_type.to_string(), true)
            };
            let fields = lower_object_fields(schema, names, &actual_type, &block.fields)?;
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
    names: &CfdNameResolver,
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
        let actual_type = names.resolve(type_name);
        validate_actual_type(schema, expected_type, &actual_type, *span)?;
    }
    validate_record_key(&reference.key.0, reference.key.1)?;
    Ok(LoadedValueDraft::record_ref(reference.key.0.clone()))
}

fn lower_array(
    schema: &CftSchema,
    names: &CfdNameResolver,
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
        let result = lower_value_resolved(schema, names, item, inner);
        match result {
            Ok(value) => lowered.push(value),
            Err(error) => diagnostics.extend(error.diagnostics),
        }
    }
    finish(LoadedValueDraft::Array(lowered), diagnostics)
}

fn lower_dict(
    schema: &CftSchema,
    names: &CfdNameResolver,
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
    let mut entries = Vec::new();
    let mut diagnostics = Vec::new();
    for field in &block.fields {
        let key = lower_dict_key(schema, names, &field.name, field.name_span, key_type);
        let value = lower_value_resolved(schema, names, &field.value, value_type);
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
    names: &CfdNameResolver,
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
            let variant = enum_variant(names, enum_name, raw, span)?;
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
