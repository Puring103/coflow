use coflow_cft::{
    CftSchemaBinOp, CftSchemaCheckExpr, CftSchemaCheckExprKind, CftSchemaCheckFormatSegment,
    CftSchemaCheckMessageKind, CftSchemaCheckStmt, CftSchemaCmpOp, CftSchemaQuantifierKind,
    CftSchemaTypePredicate, CftSchemaUnaryOp, CftValueType,
};
use coflow_data_model::{CfdErrorCode, CfdPath, CfdPathSegment, DimensionFieldLookupError};

use crate::eval::{EvalValue, ScalarValue, ValueLocation};

pub(crate) mod explanations;
pub(crate) mod trace;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckDiagnosticContext {
    When {
        expression: String,
    },
    Quantifier {
        kind: String,
        binding: String,
        item: String,
    },
    Dimension {
        dimension: String,
        variant: String,
    },
}

impl CheckDiagnosticContext {
    #[must_use]
    pub fn human_message(&self) -> String {
        match self {
            Self::When { expression } => format!("在 when {expression} 内"),
            Self::Quantifier { binding, item, .. } => format!("绑定 {binding} 位于 {item}"),
            Self::Dimension { dimension, variant } => format!("{dimension}={variant}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckDiagnostic {
    pub diagnostic: coflow_data_model::CfdDiagnostic,
    pub contexts: Vec<CheckDiagnosticContext>,
    pub is_custom_message: bool,
}

impl CheckDiagnostic {
    #[must_use]
    pub fn into_legacy_diagnostic(mut self) -> coflow_data_model::CfdDiagnostic {
        for context in &self.contexts {
            match context {
                CheckDiagnosticContext::Dimension { .. } => {
                    self.diagnostic.message =
                        format!("[{}] {}", context.human_message(), self.diagnostic.message);
                }
                _ => {
                    self.diagnostic.message.push_str("\n上下文: ");
                    self.diagnostic.message.push_str(&context.human_message());
                }
            }
        }
        self.diagnostic
    }
}

impl From<coflow_data_model::CfdDiagnostic> for CheckDiagnostic {
    fn from(diagnostic: coflow_data_model::CfdDiagnostic) -> Self {
        Self {
            diagnostic,
            contexts: Vec::new(),
            is_custom_message: false,
        }
    }
}

#[derive(Debug)]
pub(crate) struct CheckExplanation {
    pub(crate) code: CfdErrorCode,
    pub(crate) expression: String,
    pub(crate) actual: Option<String>,
    pub(crate) expected: Option<String>,
    pub(crate) location: Option<ValueLocation>,
}

impl CheckExplanation {
    pub(crate) fn new(
        code: CfdErrorCode,
        expression: impl Into<String>,
        location: Option<ValueLocation>,
    ) -> Self {
        Self {
            code,
            expression: expression.into(),
            actual: None,
            expected: None,
            location,
        }
    }

    pub(crate) fn with_actual(mut self, actual: impl Into<String>) -> Self {
        self.actual = Some(actual.into());
        self
    }

    pub(crate) fn with_expected(mut self, expected: impl Into<String>) -> Self {
        self.expected = Some(expected.into());
        self
    }

    pub(crate) fn message(&self) -> String {
        let mut out = format!("校验失败: {}", self.expression);
        if let Some(actual) = &self.actual {
            out.push_str("\n实际值: ");
            out.push_str(actual);
        }
        if let Some(expected) = &self.expected {
            out.push_str("\n期望: ");
            out.push_str(expected);
        }
        out
    }
}

pub(crate) fn dimension_lookup_error_message(
    actual_type: &str,
    field_name: &str,
    variant: &str,
    err: DimensionFieldLookupError,
) -> String {
    match err {
        DimensionFieldLookupError::UnknownRecord => {
            format!("维度字段 `{actual_type}.{field_name}` 的所属记录不存在")
        }
        DimensionFieldLookupError::NotDimensional => {
            format!("字段 `{actual_type}.{field_name}` 不是维度字段")
        }
        DimensionFieldLookupError::DimensionMismatch => {
            format!("字段 `{actual_type}.{field_name}` 不属于当前维度")
        }
        DimensionFieldLookupError::UnknownVariant => {
            format!("维度字段 `{actual_type}.{field_name}` 缺少 variant `{variant}`")
        }
    }
}

pub(crate) fn unary_op_str(op: CftSchemaUnaryOp) -> &'static str {
    match op {
        CftSchemaUnaryOp::Not => "!",
        CftSchemaUnaryOp::BitNot => "~",
        CftSchemaUnaryOp::Neg => "-",
    }
}

pub(crate) fn bin_op_str(op: CftSchemaBinOp) -> &'static str {
    match op {
        CftSchemaBinOp::Or => "||",
        CftSchemaBinOp::And => "&&",
        CftSchemaBinOp::BitOr => "|",
        CftSchemaBinOp::BitXor => "^",
        CftSchemaBinOp::BitAnd => "&",
        CftSchemaBinOp::Add => "+",
        CftSchemaBinOp::Sub => "-",
        CftSchemaBinOp::Shl => "<<",
        CftSchemaBinOp::Shr => ">>",
        CftSchemaBinOp::Mul => "*",
        CftSchemaBinOp::Div => "/",
        CftSchemaBinOp::IntDiv => "//",
        CftSchemaBinOp::Mod => "%",
        CftSchemaBinOp::Pow => "**",
    }
}

pub(crate) fn cmp_op_str(op: CftSchemaCmpOp) -> &'static str {
    match op {
        CftSchemaCmpOp::Eq => "==",
        CftSchemaCmpOp::Ne => "!=",
        CftSchemaCmpOp::Lt => "<",
        CftSchemaCmpOp::Le => "<=",
        CftSchemaCmpOp::Gt => ">",
        CftSchemaCmpOp::Ge => ">=",
    }
}

pub(crate) fn render_stmt(stmt: &CftSchemaCheckStmt) -> String {
    match stmt {
        CftSchemaCheckStmt::Expr {
            condition, message, ..
        } => {
            let rendered = render_expr(condition);
            message.as_ref().map_or(rendered.clone(), |message| {
                let message = match &message.kind {
                    CftSchemaCheckMessageKind::String(value) => format!("{value:?}"),
                    CftSchemaCheckMessageKind::Formatted(segments) => {
                        render_formatted_string(segments)
                    }
                };
                format!("{rendered}: {message}")
            })
        }
        CftSchemaCheckStmt::Quantifier {
            kind,
            binding,
            collection,
            body,
            ..
        } => {
            let kind = match kind {
                CftSchemaQuantifierKind::All => "all",
                CftSchemaQuantifierKind::Any => "any",
                CftSchemaQuantifierKind::None => "none",
            };
            let body = body.iter().map(render_stmt).collect::<Vec<_>>().join("; ");
            format!(
                "{kind} {binding} in {} {{ {body}; }}",
                render_expr(collection)
            )
        }
        CftSchemaCheckStmt::When {
            condition, body, ..
        } => {
            let body = body.iter().map(render_stmt).collect::<Vec<_>>().join("; ");
            format!("when {} {{ {body}; }}", render_expr(condition))
        }
    }
}

pub(crate) fn render_expr(expr: &CftSchemaCheckExpr) -> String {
    match &expr.kind {
        CftSchemaCheckExprKind::Int(value) => value.to_string(),
        CftSchemaCheckExprKind::Float(value) => value.to_string(),
        CftSchemaCheckExprKind::Bool(value) => value.to_string(),
        CftSchemaCheckExprKind::Null => "null".to_string(),
        CftSchemaCheckExprKind::String(value) => format!("\"{value}\""),
        CftSchemaCheckExprKind::FormattedString(segments) => render_formatted_string(segments),
        CftSchemaCheckExprKind::Name(name) => name.clone(),
        CftSchemaCheckExprKind::Field { expr, name } => {
            format!("{}.{}", render_expr(expr), name)
        }
        CftSchemaCheckExprKind::SafeField { expr, name } => {
            format!("{}?.{}", render_expr(expr), name)
        }
        CftSchemaCheckExprKind::Index { expr, index } => {
            format!("{}[{}]", render_expr(expr), render_expr(index))
        }
        CftSchemaCheckExprKind::SafeIndex { expr, index } => {
            format!("{}?[{}]", render_expr(expr), render_expr(index))
        }
        CftSchemaCheckExprKind::Coalesce { lhs, rhs } => {
            format!("{} ?? {}", render_expr(lhs), render_expr(rhs))
        }
        CftSchemaCheckExprKind::Is { expr, predicate } => {
            let predicate = match predicate {
                CftSchemaTypePredicate::Type(name) => name.as_str(),
                CftSchemaTypePredicate::Null => "null",
            };
            format!("{} is {predicate}", render_expr(expr))
        }
        CftSchemaCheckExprKind::Call { name, args } => {
            let args = args.iter().map(render_expr).collect::<Vec<_>>().join(", ");
            format!("{name}({args})")
        }
        CftSchemaCheckExprKind::MethodCall {
            receiver,
            name,
            args,
        } => {
            let args = args.iter().map(render_expr).collect::<Vec<_>>().join(", ");
            format!("{}.{name}({args})", render_expr(receiver))
        }
        CftSchemaCheckExprKind::BinOp { op, lhs, rhs } => {
            format!(
                "{} {} {}",
                render_expr(lhs),
                bin_op_str(*op),
                render_expr(rhs)
            )
        }
        CftSchemaCheckExprKind::Unary { op, expr } => {
            format!("{}{}", unary_op_str(*op), render_expr(expr))
        }
        CftSchemaCheckExprKind::CmpChain { first, rest } => {
            let mut out = render_expr(first);
            for (op, expr) in rest {
                out.push(' ');
                out.push_str(cmp_op_str(*op));
                out.push(' ');
                out.push_str(&render_expr(expr));
            }
            out
        }
    }
}

fn render_formatted_string(segments: &[CftSchemaCheckFormatSegment]) -> String {
    let mut out = String::from("f\"");
    for segment in segments {
        match segment {
            CftSchemaCheckFormatSegment::Text(value, _) => {
                for ch in value.chars() {
                    match ch {
                        '{' => out.push_str("{{"),
                        '}' => out.push_str("}}"),
                        '\\' => out.push_str("\\\\"),
                        '"' => out.push_str("\\\""),
                        '\n' => out.push_str("\\n"),
                        '\r' => out.push_str("\\r"),
                        '\t' => out.push_str("\\t"),
                        _ => out.push(ch),
                    }
                }
            }
            CftSchemaCheckFormatSegment::Expr(expr) => {
                out.push('{');
                out.push_str(&render_expr(expr));
                out.push('}');
            }
        }
    }
    out.push('"');
    out
}

pub(crate) fn format_cfd_path_for_message(path: &CfdPath) -> String {
    let mut out = String::new();
    for segment in &path.segments {
        match segment {
            CfdPathSegment::Field(name) => {
                if !out.is_empty() {
                    out.push('.');
                }
                out.push_str(name);
            }
            CfdPathSegment::Index(index) => {
                out.push('[');
                out.push_str(&index.to_string());
                out.push(']');
            }
            CfdPathSegment::DictKey(key) => {
                out.push('[');
                out.push_str(key);
                out.push(']');
            }
        }
    }
    if out.is_empty() {
        ".".to_string()
    } else {
        out
    }
}

/// Render a `EvalValue` as a short token for inclusion in a diagnostic
/// message: strings are quoted, collections summarize, records show their key.
pub(crate) fn format_value_for_message(value: &EvalValue<'_>) -> String {
    if let Some(scalar) = value.scalar() {
        return match scalar {
            ScalarValue::Null => "null".to_string(),
            ScalarValue::Bool(v) => v.to_string(),
            ScalarValue::Int(v) => v.to_string(),
            ScalarValue::Float(v) => v.to_string(),
            ScalarValue::String(s) => format!("\"{s}\""),
            ScalarValue::Enum(e) => match &e.variant {
                Some(variant) => format!("{}.{}", e.enum_name, variant),
                None => format!("{}({})", e.enum_name, e.value),
            },
        };
    }
    match value {
        EvalValue::Model(_) | EvalValue::DictKey(_) | EvalValue::Temporary(_) => {
            "<scalar>".to_string()
        }
        EvalValue::EnumNamespace(name) => name.to_string(),
        EvalValue::Record(_) => "<record>".to_string(),
        EvalValue::Entry(entry) => {
            format!("<entry key={}>", format_value_for_message(&entry.key))
        }
        EvalValue::Array { items, .. } => format!("<array len={}>", items.len()),
        EvalValue::Dict { entries, .. } => format!("<dict len={}>", entries.len()),
    }
}

pub(crate) fn value_type_is_float(ty: Option<&CftValueType>) -> bool {
    match ty {
        Some(CftValueType::Float) => true,
        Some(CftValueType::Nullable(inner)) => value_type_is_float(Some(inner)),
        _ => false,
    }
}
