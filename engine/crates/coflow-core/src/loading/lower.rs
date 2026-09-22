use crate::schema::{CftFunctionParameter, CftSchema, CftValueType};
use crate::{
    LoadedDictKeyDraft, LoadedFormattedString, LoadedFunction, LoadedRecordDraft, LoadedValueDraft,
};
use coflow_language::cfd::{
    CfdAst, CfdBitExpr, CfdBitExprKind, CfdBitOp, CfdField, CfdRecord, CfdValue,
};
use coflow_language::lexical::record_key_ident_error;
use coflow_language::source::Span;
use std::collections::{BTreeMap, BTreeSet};

use super::{CfdTextDiagnostic, CfdTextDiagnostics, CfdTextErrorCode, CfdTextSpan};

#[derive(Debug, Clone)]
pub struct ParsedLoadedRecordDraft {
    pub record: LoadedRecordDraft,
    pub span: CfdTextSpan,
}

pub fn lower_records(
    schema: &CftSchema,
    ast: &CfdAst,
) -> Result<Vec<ParsedLoadedRecordDraft>, CfdTextDiagnostics> {
    let (records, diagnostics) = lower_records_with_mode(schema, ast, false);
    finish(records, diagnostics)
}

pub fn lower_records_partial(
    schema: &CftSchema,
    ast: &CfdAst,
) -> (Vec<ParsedLoadedRecordDraft>, Vec<CfdTextDiagnostic>) {
    lower_records_with_mode(schema, ast, true)
}

fn lower_records_with_mode(
    schema: &CftSchema,
    ast: &CfdAst,
    preserve_repairable_values: bool,
) -> (Vec<ParsedLoadedRecordDraft>, Vec<CfdTextDiagnostic>) {
    let mut original_sources = BTreeMap::new();
    for record in &ast.records {
        for field in &record.fields {
            collect_callable_sources(&field.value, &mut original_sources);
        }
    }
    let mut records = Vec::with_capacity(ast.records.len());
    let mut diagnostics = Vec::new();
    let mut imports = BTreeMap::new();
    for full in &ast.imports {
        let short = full.rsplit("::").next().unwrap_or(full);
        if imports.insert(short.to_string(), full.clone()).is_some() {
            diagnostics.push(CfdTextDiagnostic::error(
                CfdTextErrorCode::Syntax,
                "conflicting import",
                CfdTextSpan::default(),
            ));
        }
        if schema.resolve_type(full).is_none()
            && schema.resolve_enum(full).is_none()
            && schema.resolve_const(full).is_none()
            && !full
                .rsplit_once("::")
                .is_some_and(|(owner, _)| schema.resolve_enum(owner).is_some())
            && !matches!(
                full.as_str(),
                "Coflow::Check::require" | "Coflow::Check::records"
            )
        {
            diagnostics.push(CfdTextDiagnostic::error(
                CfdTextErrorCode::UnknownType,
                "unknown import",
                CfdTextSpan::default(),
            ));
        }
    }
    for record in &ast.records {
        let mut record = record.clone();
        record.type_name = resolve_import(&record.type_name, &imports);
        match schema.resolve_record_type_name(&record.type_name) {
            Ok(name) => record.type_name = name,
            Err(message) => {
                diagnostics.push(CfdTextDiagnostic::error(
                    CfdTextErrorCode::UnknownType,
                    message,
                    text_span(record.type_span),
                ));
                continue;
            }
        }
        let context = record.type_name.clone();
        for field in &mut record.fields {
            resolve_source_value(schema, &mut field.value, Some(&context), &imports);
        }
        match lower_record(schema, &record, preserve_repairable_values) {
            Ok(mut record) => {
                for value in record.record.fields.values_mut() {
                    attach_imports(value, &imports, &original_sources);
                }
                records.push(record);
            }
            Err(error) => diagnostics.extend(error.diagnostics),
        }
    }
    (records, diagnostics)
}

fn resolve_import(name: &str, imports: &BTreeMap<String, String>) -> String {
    let (first, rest) = name.split_once("::").map_or((name, ""), |(a, b)| (a, b));
    imports.get(first).map_or_else(
        || name.to_string(),
        |full| {
            if rest.is_empty() {
                full.clone()
            } else {
                format!("{full}::{rest}")
            }
        },
    )
}
fn resolve_source_value(
    schema: &CftSchema,
    value: &mut CfdValue,
    context: Option<&str>,
    imports: &BTreeMap<String, String>,
) {
    match value {
        CfdValue::Ref(reference) => {
            if let Some((name, _)) = &mut reference.type_name {
                *name = resolve_import(name, imports);
            } else if let Some(context) = context {
                reference.type_name = Some((context.to_string(), reference.span));
            }
        }
        CfdValue::Block(block) => {
            if let Some((name, _)) = &mut block.type_marker {
                *name = resolve_import(name, imports);
            }
            let nested_context = if block.type_marker.is_some() {
                None
            } else {
                context
            };
            for field in &mut block.fields {
                resolve_source_value(schema, &mut field.value, nested_context, imports);
            }
        }
        CfdValue::Array(values, _) => {
            for value in values {
                resolve_source_value(schema, value, context, imports);
            }
        }
        CfdValue::Scalar(text, _) => *text = resolve_import(text, imports),
        CfdValue::BitExpr(expr) => resolve_bit_imports(expr, imports),
        CfdValue::Function(function) => {
            if let Ok(mut signature) =
                coflow_language::cft::syntax::parser::parse_type_prefix(&function.source)
            {
                let end = signature.span.end;
                resolve_type_imports(&mut signature, imports);
                if let Ok(resolved) = schema.resolve_type_ref(&signature) {
                    function.source = format!("{resolved}{}", &function.source[end..]);
                }
            }
        }
        _ => {}
    }
}

fn resolve_bit_imports(expr: &mut CfdBitExpr, imports: &BTreeMap<String, String>) {
    match &mut expr.kind {
        CfdBitExprKind::Value(name) => *name = resolve_import(name, imports),
        CfdBitExprKind::Binary { lhs, rhs, .. } => {
            resolve_bit_imports(lhs, imports);
            resolve_bit_imports(rhs, imports);
        }
    }
}
fn resolve_type_imports(
    ty: &mut coflow_language::cft::syntax::ast::TypeRef,
    imports: &BTreeMap<String, String>,
) {
    use coflow_language::cft::syntax::ast::TypeRefKind;
    match &mut ty.kind {
        TypeRefKind::Named(name) => *name = resolve_import(name, imports),
        TypeRefKind::Array(inner) | TypeRefKind::Option(inner) => {
            resolve_type_imports(inner, imports)
        }
        TypeRefKind::Dict(key, value) => {
            resolve_type_imports(key, imports);
            resolve_type_imports(value, imports);
        }
        TypeRefKind::Function(args, result) => {
            for arg in args {
                resolve_type_imports(&mut arg.value_type, imports);
            }
            resolve_type_imports(result, imports);
        }
        _ => {}
    }
}

fn lower_record(
    schema: &CftSchema,
    record: &CfdRecord,
    preserve_repairable_values: bool,
) -> Result<ParsedLoadedRecordDraft, CfdTextDiagnostics> {
    validate_record_key(&record.key, record.key_span)?;
    let type_name = record.type_name.clone();
    validate_concrete_type(schema, &type_name, record.type_span)?;
    let metadata = schema.resolve_type(&type_name).ok_or_else(|| {
        error(
            CfdTextErrorCode::UnknownType,
            "unknown record type",
            record.type_span,
        )
    })?;
    if metadata.kind == coflow_language::cft::syntax::ast::TypeKind::Data || metadata.is_host {
        return Err(error(
            CfdTextErrorCode::TypeMismatch,
            "CFD records require a non-Host table or singleton",
            record.type_span,
        ));
    }
    let (fields, dimension_values) = lower_object_fields(
        schema,
        &type_name,
        &record.key,
        &record.fields,
        preserve_repairable_values,
    )?;
    let mut draft = LoadedRecordDraft::new(record.key.clone(), type_name, fields);
    draft.dimension_values = dimension_values;
    Ok(ParsedLoadedRecordDraft {
        record: draft,
        span: text_span(record.span),
    })
}

fn lower_object_fields(
    schema: &CftSchema,
    type_name: &str,
    record_key: &str,
    fields: &[CfdField],
    preserve_repairable_values: bool,
) -> Result<
    (
        BTreeMap<String, LoadedValueDraft>,
        Vec<crate::DimensionValueDraft>,
    ),
    CfdTextDiagnostics,
> {
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
    let mut dimension_values = Vec::new();
    let source_key =
        crate::RecordKey::new(record_key.to_string()).map_err(|error| CfdTextDiagnostics {
            diagnostics: vec![CfdTextDiagnostic::error(
                CfdTextErrorCode::TypeMismatch,
                error.to_string(),
                CfdTextSpan::default(),
            )],
        })?;
    for field in fields {
        if field.name == "id"
            && schema_type.kind != coflow_language::cft::syntax::ast::TypeKind::Data
        {
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
        match (&meta.dimension, &field.value) {
            (Some(binding), CfdValue::Dimension(dimension)) => {
                let mut entries = BTreeSet::new();
                let mut default = None;
                for entry in &dimension.fields {
                    if !entries.insert(entry.name.clone()) {
                        diagnostics.extend(
                            error(
                                CfdTextErrorCode::DuplicateField,
                                format!("duplicate dimension entry `{}`", entry.name),
                                entry.name_span,
                            )
                            .diagnostics,
                        );
                        continue;
                    }
                    match lower_value_resolved(
                        schema,
                        &entry.value,
                        &meta.value_type,
                        preserve_repairable_values,
                    ) {
                        Ok(value) if entry.name == "default" => default = Some(value),
                        Ok(value) => match crate::VariantName::new(entry.name.clone()) {
                            Ok(variant) => dimension_values.push(crate::DimensionValueDraft {
                                source_type: meta.declaring_type.clone(),
                                source_key: source_key.clone(),
                                field: meta.name.clone(),
                                dimension: binding.dimension.clone(),
                                variant,
                                value,
                                origin: crate::RecordOrigin::None,
                            }),
                            Err(_) => diagnostics.extend(
                                error(
                                    CfdTextErrorCode::TypeMismatch,
                                    format!("invalid dimension variant `{}`", entry.name),
                                    entry.name_span,
                                )
                                .diagnostics,
                            ),
                        },
                        Err(error) => diagnostics.extend(error.diagnostics),
                    }
                }
                if let Some(default) = default {
                    values.insert(field.name.clone(), default);
                } else {
                    diagnostics.extend(
                        error(
                            CfdTextErrorCode::TypeMismatch,
                            "dimension value requires `default`",
                            dimension.span,
                        )
                        .diagnostics,
                    );
                }
            }
            (Some(_), _) => diagnostics.extend(
                error(
                    CfdTextErrorCode::TypeMismatch,
                    format!(
                        "dimension field `{}` requires `dimension {{ ... }}`",
                        field.name
                    ),
                    field.value.span(),
                )
                .diagnostics,
            ),
            (None, CfdValue::Dimension(_)) => diagnostics.extend(
                error(
                    CfdTextErrorCode::TypeMismatch,
                    format!("field `{}` is not dimensional", field.name),
                    field.value.span(),
                )
                .diagnostics,
            ),
            (None, _) => match lower_value_resolved(
                schema,
                &field.value,
                &meta.value_type,
                preserve_repairable_values,
            ) {
                Ok(value) => {
                    values.insert(field.name.clone(), value);
                }
                Err(error) => diagnostics.extend(error.diagnostics),
            },
        }
    }
    if diagnostics.is_empty() {
        Ok((values, dimension_values))
    } else {
        Err(CfdTextDiagnostics { diagnostics })
    }
}

fn lower_value_resolved(
    schema: &CftSchema,
    value: &CfdValue,
    ty: &CftValueType,
    preserve_repairable_values: bool,
) -> Result<LoadedValueDraft, CfdTextDiagnostics> {
    if let CfdValue::Scalar(name, span) = value {
        if let Some(constant) = schema.resolve_const(name) {
            if !schema.value_type_assignable(&constant.value_type, ty) {
                return Err(error(
                    CfdTextErrorCode::TypeMismatch,
                    format!("constant `{name}` does not match `{ty}`"),
                    *span,
                ));
            }
            let mut result = super::constants::materialize(&constant.value)
                .map_err(|message| error(CfdTextErrorCode::TypeMismatch, message, *span))?;
            if matches!(ty, CftValueType::Float) {
                if let LoadedValueDraft::Int(value) = result {
                    result = LoadedValueDraft::Float(f64::from(value as f32));
                }
            }
            if matches!(ty, CftValueType::Option(_))
                && !matches!(constant.value_type, CftValueType::Option(_))
            {
                result = LoadedValueDraft::OptionSome(Box::new(result));
            }
            return Ok(result);
        }
    }
    match ty {
        CftValueType::Int => lower_int(value),
        CftValueType::Float => lower_float(value),
        CftValueType::Bool => lower_bool(value),
        CftValueType::String => match value {
            CfdValue::QuotedString(text, _) => Ok(LoadedValueDraft::String(text.clone())),
            _ => Err(error(
                CfdTextErrorCode::TypeMismatch,
                "expected string",
                value.span(),
            )),
        },
        CftValueType::FString => match value {
            CfdValue::FormattedString(_) => lower_string(value),
            _ => Err(error(
                CfdTextErrorCode::TypeMismatch,
                "expected fstring template",
                value.span(),
            )),
        },
        CftValueType::Enum(name) => lower_enum(schema, value, name, preserve_repairable_values),
        CftValueType::Object(name) => lower_object(schema, value, name, preserve_repairable_values),
        CftValueType::RecordRef(name) => lower_ref(schema, value, name),
        CftValueType::Array(inner) => lower_array(schema, value, inner, preserve_repairable_values),
        CftValueType::Dict(key, item) => {
            lower_dict(schema, value, key, item, preserve_repairable_values)
        }
        CftValueType::Option(inner) => match value {
            CfdValue::OptionNone(_) => Ok(LoadedValueDraft::OptionNone),
            value => lower_value_resolved(schema, value, inner, preserve_repairable_values)
                .map(|value| LoadedValueDraft::OptionSome(Box::new(value))),
        },
        CftValueType::Function(parameters, result) => {
            lower_function(schema, value, parameters, result)
        }
        CftValueType::Unit => Err(error(
            CfdTextErrorCode::TypeMismatch,
            format!("expected `{ty}`"),
            value.span(),
        )),
    }
}

fn lower_function(
    schema: &CftSchema,
    value: &CfdValue,
    expected_parameters: &[CftFunctionParameter],
    expected_result: &CftValueType,
) -> Result<LoadedValueDraft, CfdTextDiagnostics> {
    let CfdValue::Function(function) = value else {
        return Err(error(
            CfdTextErrorCode::TypeMismatch,
            "expected function",
            value.span(),
        ));
    };
    let (parameters, result) = parse_signature(schema, &function.source)
        .map_err(|message| error(CfdTextErrorCode::TypeMismatch, message, function.span))?;
    let actual = CftValueType::Function(parameters, Box::new(result));
    let expected = CftValueType::Function(
        expected_parameters.to_vec(),
        Box::new(expected_result.clone()),
    );
    if actual != expected {
        return Err(error(
            CfdTextErrorCode::TypeMismatch,
            format!("expected function `{expected}`, found `{actual}`"),
            function.span,
        ));
    }
    Ok(LoadedValueDraft::Function(LoadedFunction {
        from_default: false,
        location: Some(crate::ingest::CallableLocation {
            module: None,
            source: function.source.clone(),
            span: function.span,
            path: None,
        }),
        imports: Default::default(),
        constant_origin: None,
        source: function.source.clone(),
    }))
}

fn parse_signature(
    schema: &CftSchema,
    source: &str,
) -> Result<(Vec<CftFunctionParameter>, CftValueType), String> {
    let syntax = coflow_language::cft::syntax::parser::parse_type_prefix(source)
        .map_err(|e| format!("{e:?}"))?;
    let value_type = schema.resolve_type_ref(&syntax)?;
    if let CftValueType::Function(parameters, result) = value_type {
        if parameters.iter().any(|parameter| parameter.name.is_none()) {
            return Err("function literal parameters require names".into());
        }
        let mut names = BTreeSet::new();
        if parameters
            .iter()
            .filter_map(|p| p.name.as_ref())
            .any(|name| !names.insert(name))
        {
            return Err("duplicate function parameter name".into());
        }
        Ok((parameters, *result))
    } else {
        Err("expected function signature".into())
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
    coflow_language::lexical::validate_number_literal(text.strip_prefix('-').unwrap_or(text))
        .map_err(|failure| error(CfdTextErrorCode::TypeMismatch, failure.message, span))?;
    text.replace('_', "")
        .parse::<i32>()
        .map(|value| LoadedValueDraft::Int(i64::from(value)))
        .map_err(|_| error(CfdTextErrorCode::TypeMismatch, "expected int", span))
}

fn lower_float(value: &CfdValue) -> Result<LoadedValueDraft, CfdTextDiagnostics> {
    let (text, span) = scalar(value, "float")?;
    let number = coflow_language::lexical::parse_float_literal(text)
        .map_err(|failure| error(CfdTextErrorCode::TypeMismatch, failure.message, span))?;
    Ok(LoadedValueDraft::Float(f64::from(number)))
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
        CfdValue::FormattedString(value) => {
            Ok(LoadedValueDraft::FormattedString(LoadedFormattedString {
                from_default: false,
                location: Some(crate::ingest::CallableLocation {
                    module: None,
                    source: value.source.clone(),
                    span: value.span,
                    path: None,
                }),
                imports: Default::default(),
                constant_origin: None,
                source: value.source.clone(),
            }))
        }
        _ => Err(error(
            CfdTextErrorCode::TypeMismatch,
            "expected string",
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
            let actual_type = if let Some((actual_type, span)) = &block.type_marker {
                validate_actual_type(schema, expected_type, actual_type, *span)?;
                actual_type.clone()
            } else {
                return Err(error(
                    CfdTextErrorCode::TypeMismatch,
                    "inline data requires an explicit type name",
                    block.span,
                ));
            };
            let (fields, dimensions) = lower_object_fields(
                schema,
                &actual_type,
                "inline",
                &block.fields,
                preserve_repairable_values,
            )?;
            debug_assert!(
                dimensions.is_empty(),
                "data fields cannot declare dimensions"
            );
            Ok(LoadedValueDraft::object(actual_type, fields))
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
    _expected_type: &str,
) -> Result<LoadedValueDraft, CfdTextDiagnostics> {
    let CfdValue::Ref(reference) = value else {
        return Err(error(
            CfdTextErrorCode::Syntax,
            "invalid record reference",
            value.span(),
        ));
    };
    let Some((type_name, span)) = &reference.type_name else {
        return Err(error(
            CfdTextErrorCode::TypeMismatch,
            "reference needs a static record type",
            reference.span,
        ));
    };
    let target = schema.resolve_type(type_name).ok_or_else(|| {
        error(
            CfdTextErrorCode::UnknownType,
            "unknown reference type",
            *span,
        )
    })?;
    if target.kind == coflow_language::cft::syntax::ast::TypeKind::Data {
        return Err(error(
            CfdTextErrorCode::TypeMismatch,
            "data has no record identity",
            *span,
        ));
    }
    validate_record_key(&reference.key.0, reference.key.1)?;
    Ok(LoadedValueDraft::record_ref(format!(
        "{type_name}::{}",
        reference.key.0
    )))
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
        CftValueType::Bool => raw
            .parse::<bool>()
            .map(LoadedDictKeyDraft::Bool)
            .map_err(|_| {
                error(
                    CfdTextErrorCode::TypeMismatch,
                    "expected bool dictionary key",
                    span,
                )
            }),
        CftValueType::Int => raw
            .parse::<i32>()
            .map(|value| LoadedDictKeyDraft::Int(i64::from(value)))
            .map_err(|_| {
                error(
                    CfdTextErrorCode::TypeMismatch,
                    "expected int dict key",
                    span,
                )
            }),
        CftValueType::Enum(enum_name) => {
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

fn validate_concrete_type(
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
    validate_concrete_type(schema, actual_type, span)?;
    if !schema
        .resolve_type(actual_type)
        .is_some_and(|ty| ty.kind == coflow_language::cft::syntax::ast::TypeKind::Data)
    {
        return Err(error(
            CfdTextErrorCode::ObjectTypeMismatch,
            "inline values require a data type",
            span,
        ));
    }
    if !schema.is_assignable(actual_type, expected_type) {
        return Err(error(
            CfdTextErrorCode::ObjectTypeMismatch,
            format!("type `{actual_type}` is not assignable to `{expected_type}`"),
            span,
        ));
    }
    Ok(())
}

pub fn syntax_diagnostics(
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

fn attach_imports(
    value: &mut LoadedValueDraft,
    imports: &BTreeMap<String, String>,
    sources: &BTreeMap<usize, String>,
) {
    match value {
        LoadedValueDraft::Function(value) => {
            value.imports = imports.clone();
            if let Some(location) = &mut value.location {
                if let Some(source) = sources.get(&location.span.start) {
                    location.source = source.clone();
                }
            }
        }
        LoadedValueDraft::FormattedString(value) => {
            value.imports = imports.clone();
            if let Some(location) = &mut value.location {
                if let Some(source) = sources.get(&location.span.start) {
                    location.source = source.clone();
                }
            }
        }
        LoadedValueDraft::OptionSome(value) => attach_imports(value, imports, sources),
        LoadedValueDraft::Array(values) => {
            for value in values {
                attach_imports(value, imports, sources);
            }
        }
        LoadedValueDraft::Dict(values) => {
            for (_, value) in values {
                attach_imports(value, imports, sources);
            }
        }
        LoadedValueDraft::Object { fields, .. } => {
            for value in fields.values_mut() {
                attach_imports(value, imports, sources);
            }
        }
        _ => {}
    }
}

fn collect_callable_sources(value: &CfdValue, sources: &mut BTreeMap<usize, String>) {
    match value {
        CfdValue::Function(value) => {
            sources.insert(value.span.start, value.source.clone());
        }
        CfdValue::FormattedString(value) => {
            sources.insert(value.span.start, value.source.clone());
        }
        CfdValue::Array(values, _) => {
            for value in values {
                collect_callable_sources(value, sources);
            }
        }
        CfdValue::Block(block) => {
            for field in &block.fields {
                collect_callable_sources(&field.value, sources);
            }
        }
        _ => {}
    }
}
