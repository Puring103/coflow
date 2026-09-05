use crate::data_model::{
    LoadedDictKeyDraft, LoadedFieldReference, LoadedFormatSegment, LoadedFormattedString,
    LoadedFunction, LoadedRecordDraft, LoadedValueDraft,
};
use coflow_language::cfd::{
    CfdAst, CfdBitExpr, CfdBitExprKind, CfdBitOp, CfdField, CfdFormatSegment, CfdRecord, CfdValue,
};
use coflow_language::cft::{CftFunctionParameter, CftSchema, CftValueType};
use coflow_language::lexical::record_key_ident_error;
use coflow_language::source::Span;
use std::collections::{BTreeMap, BTreeSet};

use super::{CfdTextDiagnostic, CfdTextDiagnostics, CfdTextErrorCode, CfdTextSpan};

#[derive(Debug, Clone)]
pub(super) struct ParsedLoadedRecordDraft {
    pub(super) record: LoadedRecordDraft,
    pub(super) span: CfdTextSpan,
}

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
        validate_actual_type(schema, &group_type, &type_name, record.type_span)?;
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

pub(crate) fn lower_value(
    schema: &CftSchema,
    value: &CfdValue,
    ty: &CftValueType,
) -> Result<LoadedValueDraft, CfdTextDiagnostics> {
    lower_value_resolved(schema, value, ty, false)
}

fn lower_value_resolved(
    schema: &CftSchema,
    value: &CfdValue,
    ty: &CftValueType,
    preserve_repairable_values: bool,
) -> Result<LoadedValueDraft, CfdTextDiagnostics> {
    match ty {
        CftValueType::Int => lower_int(value),
        CftValueType::Float => lower_float(value),
        CftValueType::Bool => lower_bool(value),
        CftValueType::String => lower_string(value),
        CftValueType::Enum(name) => {
            lower_enum(schema, value, name, preserve_repairable_values)
        }
        CftValueType::Object(name) => {
            lower_object(schema, value, name, preserve_repairable_values)
        }
        CftValueType::RecordRef(name) => lower_ref(schema, value, name),
        CftValueType::Array(inner) => {
            lower_array(schema, value, inner, preserve_repairable_values)
        }
        CftValueType::Dict(key, item) => {
            lower_dict(schema, value, key, item, preserve_repairable_values)
        }
        CftValueType::Option(inner) => match value {
            CfdValue::OptionNone(_) => Ok(LoadedValueDraft::OptionNone),
            CfdValue::OptionSome(value, _) => lower_value_resolved(schema, value, inner, preserve_repairable_values)
                .map(|value| LoadedValueDraft::OptionSome(Box::new(value))),
            value => lower_value_resolved(schema, value, inner, preserve_repairable_values)
                .map(|value| LoadedValueDraft::OptionSome(Box::new(value))),
        },
        CftValueType::Result(ok, error_type) => match value {
            CfdValue::ResultOk(value, _) => lower_value_resolved(schema, value, ok, preserve_repairable_values)
                .map(|value| LoadedValueDraft::ResultOk(Box::new(value))),
            CfdValue::ResultErr(value, _) => lower_value_resolved(schema, value, error_type, preserve_repairable_values)
                .map(|value| LoadedValueDraft::ResultErr(Box::new(value))),
            _ => Err(error(
                CfdTextErrorCode::TypeMismatch,
                format!("expected `Ok(...)` or `Err(...)` for `{ty}`"),
                value.span(),
            )),
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
    let (parameters, result) = FunctionSignatureParser::new(schema, &function.source)
        .parse()
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
        source: function.source.clone(),
    }))
}

struct FunctionSignatureParser<'a> {
    schema: &'a CftSchema,
    source: &'a str,
    pos: usize,
}

impl<'a> FunctionSignatureParser<'a> {
    fn new(schema: &'a CftSchema, source: &'a str) -> Self {
        Self {
            schema,
            source,
            pos: 0,
        }
    }

    fn parse(mut self) -> Result<(Vec<CftFunctionParameter>, CftValueType), String> {
        self.skip_ws();
        self.expect_word("fn")?;
        self.skip_ws();
        self.expect('(')?;
        let parameters = self.parse_parameters(true)?;
        self.skip_ws();
        self.expect_text("->")?;
        let result = self.parse_type()?;
        self.skip_ws();
        self.expect('{')?;
        Ok((parameters, result))
    }

    fn parse_parameters(
        &mut self,
        require_names: bool,
    ) -> Result<Vec<CftFunctionParameter>, String> {
        let mut parameters = Vec::new();
        let mut parameter_names = BTreeSet::new();
        self.skip_ws();
        if self.eat(')') {
            return Ok(parameters);
        }
        loop {
            self.skip_ws();
            let saved = self.pos;
            let candidate = self.parse_name();
            self.skip_ws();
            let (name, value_type) = if !candidate.is_empty() && self.eat(':') {
                if !parameter_names.insert(candidate.clone()) {
                    return Err(format!("duplicate function parameter `{candidate}`"));
                }
                (Some(candidate), self.parse_type()?)
            } else {
                self.pos = saved;
                if require_names {
                    return Err("function value parameters must have names".to_string());
                }
                (None, self.parse_type()?)
            };
            parameters.push(CftFunctionParameter { name, value_type });
            self.skip_ws();
            if self.eat(')') {
                break;
            }
            self.expect(',')?;
        }
        Ok(parameters)
    }

    fn parse_type(&mut self) -> Result<CftValueType, String> {
        self.skip_ws();
        if self.eat('&') {
            let raw = self.required_name("record reference target")?;
            let schema_type = self
                .schema
                .resolve_type(&raw)
                .ok_or_else(|| format!("unknown record reference type `{raw}`"))?;
            return Ok(CftValueType::RecordRef(schema_type.name.clone()));
        }
        if self.eat('[') {
            let inner = self.parse_type()?;
            self.skip_ws();
            self.expect(']')?;
            return Ok(CftValueType::Array(Box::new(inner)));
        }
        if self.eat('{') {
            let key = self.parse_type()?;
            self.skip_ws();
            self.expect(':')?;
            let value = self.parse_type()?;
            self.skip_ws();
            self.expect('}')?;
            return Ok(CftValueType::Dict(Box::new(key), Box::new(value)));
        }
        if self.eat('(') {
            self.skip_ws();
            self.expect(')')?;
            return Ok(CftValueType::Unit);
        }
        let name = self.required_name("type")?;
        if name == "fn" {
            self.skip_ws();
            self.expect('(')?;
            let parameters = self.parse_parameters(false)?;
            self.skip_ws();
            self.expect_text("->")?;
            let result = self.parse_type()?;
            return Ok(CftValueType::Function(parameters, Box::new(result)));
        }
        if matches!(name.as_str(), "Option" | "Result") {
            self.skip_ws();
            self.expect('<')?;
            let first = self.parse_type()?;
            self.skip_ws();
            if name == "Option" {
                self.expect('>')?;
                return Ok(CftValueType::Option(Box::new(first)));
            }
            self.expect(',')?;
            let second = self.parse_type()?;
            self.skip_ws();
            self.expect('>')?;
            return Ok(CftValueType::Result(Box::new(first), Box::new(second)));
        }
        Ok(match name.as_str() {
            "int" => CftValueType::Int,
            "float" => CftValueType::Float,
            "bool" => CftValueType::Bool,
            "string" => CftValueType::String,
            _ => {
                if let Some(schema_enum) = self.schema.resolve_enum(&name) {
                    CftValueType::Enum(schema_enum.name.clone())
                } else if let Some(schema_type) = self.schema.resolve_type(&name) {
                    CftValueType::Object(schema_type.name.clone())
                } else {
                    return Err(format!("unknown function signature type `{name}`"));
                }
            }
        })
    }

    fn required_name(&mut self, expected: &str) -> Result<String, String> {
        let name = self.parse_name();
        if name.is_empty() {
            Err(format!("expected {expected} in function signature"))
        } else {
            Ok(name)
        }
    }

    fn parse_name(&mut self) -> String {
        self.skip_ws();
        let start = self.pos;
        while let Some(ch) = self.peek() {
            if ch.is_whitespace()
                || matches!(
                    ch,
                    '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | ',' | ':' | '&'
                )
            {
                break;
            }
            self.pos += ch.len_utf8();
        }
        self.source[start..self.pos].to_string()
    }

    fn expect_word(&mut self, expected: &str) -> Result<(), String> {
        if self.source[self.pos..].starts_with(expected) {
            self.pos += expected.len();
            Ok(())
        } else {
            Err(format!("expected `{expected}` in function signature"))
        }
    }

    fn expect_text(&mut self, expected: &str) -> Result<(), String> {
        self.skip_ws();
        if self.source[self.pos..].starts_with(expected) {
            self.pos += expected.len();
            Ok(())
        } else {
            Err(format!("expected `{expected}` in function signature"))
        }
    }

    fn expect(&mut self, expected: char) -> Result<(), String> {
        self.skip_ws();
        if self.eat(expected) {
            Ok(())
        } else {
            Err(format!("expected `{expected}` in function signature"))
        }
    }

    fn eat(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.pos += expected.len_utf8();
            true
        } else {
            false
        }
    }

    fn skip_ws(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.pos += self.peek().map_or(0, char::len_utf8);
        }
    }

    fn peek(&self) -> Option<char> {
        self.source[self.pos..].chars().next()
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
            "expected string",
            value.span(),
        )),
    }
}

fn enum_variant(
    expected_enum: &str,
    raw: &str,
    span: Span,
) -> Result<String, CfdTextDiagnostics> {
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
            CfdValue::Scalar(raw, span) => {
                lower_flag_operand(schema, enum_name, raw, *span)?
            }
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
                validate_actual_type(schema, expected_type, actual_type, *span)?;
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
    Ok(LoadedValueDraft::record_ref(reference.key.0.clone()))
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
        let value = lower_value_resolved(
            schema,
            &field.value,
            value_type,
            preserve_repairable_values,
        );
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
