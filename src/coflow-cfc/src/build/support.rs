use super::BuildCtx;
use crate::ast::Expr;
use crate::error::{BuildError, BuildErrorKind};
use crate::value::{CfcNominalType, CfcValue, CfcValueRef};

impl BuildCtx<'_> {
    pub(super) fn ensure_basic_value(
        &mut self,
        expr: &Expr,
        value: CfcValueRef,
        expected: &str,
    ) -> Option<CfcValueRef> {
        let matches = {
            let borrowed = value.borrow();
            matches!(
                (&*borrowed, expected),
                (CfcValue::Null, "null")
                    | (CfcValue::Int(_), "int")
                    | (CfcValue::Float(_), "float")
                    | (CfcValue::Bool(_), "bool")
                    | (CfcValue::String(_), "string")
            )
        };
        if matches {
            Some(value)
        } else {
            self.type_error(expr, expected)
        }
    }

    pub(super) fn type_error<T>(&mut self, expr: &Expr, expected: &str) -> Option<T> {
        self.errors.push(BuildError::new(
            BuildErrorKind::TypeMismatch,
            format!("expected `{expected}`"),
            Some(expr.span),
        ));
        None
    }
}

pub(super) fn build_error(message: impl Into<String>) -> BuildError {
    BuildError::other(message, None)
}

pub(super) fn format_nominal(ty: &CfcNominalType) -> String {
    format!("{}.{}", ty.module, ty.name)
}

pub(super) fn value_signature(value: &CfcValueRef) -> ValueSignature {
    if value.is_pending() {
        return ValueSignature::Pending;
    }
    match &*value.borrow() {
        CfcValue::Null => ValueSignature::Null,
        CfcValue::Int(_) => ValueSignature::Int,
        CfcValue::Float(_) => ValueSignature::Float,
        CfcValue::Bool(_) => ValueSignature::Bool,
        CfcValue::String(_) => ValueSignature::String,
        CfcValue::Enum { enum_type, .. } => ValueSignature::Enum(enum_type.clone()),
        CfcValue::Object { type_name, .. } => ValueSignature::Object(type_name.clone()),
        CfcValue::Union { union_type, value } => ValueSignature::Union {
            union_type: union_type.clone(),
            value: Box::new(value_signature(value)),
        },
        CfcValue::Array(items) => items.first().map_or(
            ValueSignature::Array(Box::new(ValueSignature::Unknown)),
            |item| ValueSignature::Array(Box::new(value_signature(item))),
        ),
        CfcValue::Dict(entries) => entries.first().map_or(
            ValueSignature::Dict(
                Box::new(ValueSignature::Unknown),
                Box::new(ValueSignature::Unknown),
            ),
            |(key, value)| {
                ValueSignature::Dict(
                    Box::new(value_signature(key)),
                    Box::new(value_signature(value)),
                )
            },
        ),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ValueSignature {
    Pending,
    Unknown,
    Null,
    Int,
    Float,
    Bool,
    String,
    Enum(CfcNominalType),
    Object(Option<CfcNominalType>),
    Union {
        union_type: CfcNominalType,
        value: Box<ValueSignature>,
    },
    Array(Box<ValueSignature>),
    Dict(Box<ValueSignature>, Box<ValueSignature>),
}
