use crate::ast::CmpOp;
use crate::value::{CfcValue, CfcValueRef};

use crate::CfcNominalType;

#[derive(Debug, Clone)]
pub(super) enum EvalValue {
    Null,
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    Enum {
        enum_type: CfcNominalType,
        variant: String,
        value: i64,
    },
    Ref(CfcValueRef),
    EnumType {
        module: crate::ModuleId,
        name: String,
    },
    ModuleNamespace {
        module: crate::ModuleId,
        allow_data: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NumberKind {
    Int,
    Float,
}

impl EvalValue {
    pub(super) fn type_name(&self) -> &'static str {
        match self {
            EvalValue::Null => "null",
            EvalValue::Int(_) => "int",
            EvalValue::Float(_) => "float",
            EvalValue::Bool(_) => "bool",
            EvalValue::String(_) => "string",
            EvalValue::Enum { .. } => "enum",
            EvalValue::Ref(value) => value.borrow().type_name(),
            EvalValue::EnumType { .. } => "enum type",
            EvalValue::ModuleNamespace { .. } => "module namespace",
        }
    }

    pub(super) fn into_ref(self) -> Result<CfcValueRef, String> {
        match self {
            EvalValue::Ref(value) => Ok(value),
            other => Err(format!("expected reference, found {}", other.type_name())),
        }
    }

    pub(super) fn into_bool(self) -> Result<bool, String> {
        match self {
            EvalValue::Bool(value) => Ok(value),
            EvalValue::Ref(value) => match &*value.borrow() {
                CfcValue::Union { value, .. } => match &*value.borrow() {
                    CfcValue::Bool(value) => Ok(*value),
                    other => Err(format!("expected bool, found {}", other.type_name())),
                },
                CfcValue::Bool(value) => Ok(*value),
                other => Err(format!("expected bool, found {}", other.type_name())),
            },
            other => Err(format!("expected bool, found {}", other.type_name())),
        }
    }

    pub(super) fn into_i64(self) -> Result<i64, String> {
        match self {
            EvalValue::Int(value) => Ok(value),
            EvalValue::Ref(value) => match &*value.borrow() {
                CfcValue::Union { value, .. } => match &*value.borrow() {
                    CfcValue::Int(value) => Ok(*value),
                    other => Err(format!("expected int, found {}", other.type_name())),
                },
                CfcValue::Int(value) => Ok(*value),
                other => Err(format!("expected int, found {}", other.type_name())),
            },
            other => Err(format!("expected int, found {}", other.type_name())),
        }
    }

    #[allow(clippy::cast_precision_loss)]
    pub(super) fn into_f64(self) -> Result<f64, String> {
        match self {
            EvalValue::Int(value) => Ok(value as f64),
            EvalValue::Float(value) => Ok(value),
            EvalValue::Ref(value) => match &*value.borrow() {
                CfcValue::Union { value, .. } => match &*value.borrow() {
                    CfcValue::Int(value) => Ok(*value as f64),
                    CfcValue::Float(value) => Ok(*value),
                    other => Err(format!(
                        "expected numeric value, found {}",
                        other.type_name()
                    )),
                },
                CfcValue::Int(value) => Ok(*value as f64),
                CfcValue::Float(value) => Ok(*value),
                other => Err(format!(
                    "expected numeric value, found {}",
                    other.type_name()
                )),
            },
            other => Err(format!(
                "expected numeric value, found {}",
                other.type_name()
            )),
        }
    }

    pub(super) fn into_string(self) -> Result<String, String> {
        match self {
            EvalValue::String(value) => Ok(value),
            EvalValue::Ref(value) => match &*value.borrow() {
                CfcValue::Union { value, .. } => match &*value.borrow() {
                    CfcValue::String(value) => Ok(value.clone()),
                    other => Err(format!("expected string, found {}", other.type_name())),
                },
                CfcValue::String(value) => Ok(value.clone()),
                other => Err(format!("expected string, found {}", other.type_name())),
            },
            other => Err(format!("expected string, found {}", other.type_name())),
        }
    }

    pub(super) fn number_kind(&self) -> Result<NumberKind, String> {
        match self {
            EvalValue::Int(_) => Ok(NumberKind::Int),
            EvalValue::Float(_) => Ok(NumberKind::Float),
            EvalValue::Ref(value) => match &*value.borrow() {
                CfcValue::Union { value, .. } => match &*value.borrow() {
                    CfcValue::Int(_) => Ok(NumberKind::Int),
                    CfcValue::Float(_) => Ok(NumberKind::Float),
                    other => Err(format!(
                        "expected numeric value, found {}",
                        other.type_name()
                    )),
                },
                CfcValue::Int(_) => Ok(NumberKind::Int),
                CfcValue::Float(_) => Ok(NumberKind::Float),
                other => Err(format!(
                    "expected numeric value, found {}",
                    other.type_name()
                )),
            },
            other => Err(format!(
                "expected numeric value, found {}",
                other.type_name()
            )),
        }
    }
}

pub(super) fn numeric_bin(
    lhs: EvalValue,
    rhs: EvalValue,
    int_op: impl FnOnce(i64, i64) -> Option<i64>,
    float_op: impl FnOnce(f64, f64) -> f64,
    operation: &str,
) -> Result<EvalValue, String> {
    match (lhs.number_kind()?, rhs.number_kind()?) {
        (NumberKind::Int, NumberKind::Int) => int_op(lhs.into_i64()?, rhs.into_i64()?)
            .map(EvalValue::Int)
            .ok_or_else(|| format!("integer {operation} overflow")),
        _ => Ok(EvalValue::Float(float_op(lhs.into_f64()?, rhs.into_f64()?))),
    }
}

pub(super) fn shift_amount(value: i64) -> Result<u32, String> {
    let amount =
        u32::try_from(value).map_err(|_| "shift amount must be nonnegative".to_string())?;
    if amount >= i64::BITS {
        return Err(format!("shift amount `{amount}` is out of range"));
    }
    Ok(amount)
}

pub(super) fn is_string_value(value: &EvalValue) -> bool {
    match value {
        EvalValue::String(_) => true,
        EvalValue::Ref(value) => matches!(&*value.borrow(), CfcValue::String(_)),
        _ => false,
    }
}

pub(super) fn compare_values(op: CmpOp, lhs: &EvalValue, rhs: &EvalValue) -> Result<bool, String> {
    match op {
        CmpOp::Eq => equal_values(lhs, rhs),
        CmpOp::Ne => Ok(!equal_values(lhs, rhs)?),
        CmpOp::Lt | CmpOp::Le | CmpOp::Gt | CmpOp::Ge => compare_ordered(op, lhs, rhs),
    }
}

pub(super) fn eval_value_equals_ref(value: &EvalValue, other: &CfcValueRef) -> bool {
    equal_values(value, &EvalValue::Ref(other.clone())).unwrap_or(false)
}

#[allow(clippy::float_cmp)]
fn equal_values(lhs: &EvalValue, rhs: &EvalValue) -> Result<bool, String> {
    if is_null_value(lhs) || is_null_value(rhs) {
        return Ok(is_null_value(lhs) && is_null_value(rhs));
    }
    match (materialize(lhs)?, materialize(rhs)?) {
        (EvalValue::Int(a), EvalValue::Int(b)) => Ok(a == b),
        (EvalValue::Float(a), EvalValue::Float(b)) => Ok(a == b),
        (EvalValue::Bool(a), EvalValue::Bool(b)) => Ok(a == b),
        (EvalValue::String(a), EvalValue::String(b)) => Ok(a == b),
        (
            EvalValue::Enum {
                enum_type: a_ty,
                variant: a_variant,
                ..
            },
            EvalValue::Enum {
                enum_type: b_ty,
                variant: b_variant,
                ..
            },
        ) => Ok(a_ty == b_ty && a_variant == b_variant),
        (a, b) => Err(format!(
            "cannot compare {} and {} for equality",
            a.type_name(),
            b.type_name()
        )),
    }
}

fn is_null_value(value: &EvalValue) -> bool {
    match value {
        EvalValue::Null => true,
        EvalValue::Ref(value) => matches!(&*value.borrow(), CfcValue::Null),
        _ => false,
    }
}

#[allow(clippy::cast_precision_loss)]
fn compare_ordered(op: CmpOp, lhs: &EvalValue, rhs: &EvalValue) -> Result<bool, String> {
    match (materialize(lhs)?, materialize(rhs)?) {
        (EvalValue::Int(a), EvalValue::Int(b)) => Ok(apply_cmp(op, &a, &b)),
        (EvalValue::Int(a), EvalValue::Float(b)) => Ok(apply_cmp(op, &(a as f64), &b)),
        (EvalValue::Float(a), EvalValue::Int(b)) => Ok(apply_cmp(op, &a, &(b as f64))),
        (EvalValue::Float(a), EvalValue::Float(b)) => Ok(apply_cmp(op, &a, &b)),
        (
            EvalValue::Enum {
                enum_type: a_ty,
                value: a_value,
                ..
            },
            EvalValue::Enum {
                enum_type: b_ty,
                value: b_value,
                ..
            },
        ) if a_ty == b_ty => Ok(apply_cmp(op, &a_value, &b_value)),
        (
            EvalValue::Enum {
                enum_type: a_ty, ..
            },
            EvalValue::Enum {
                enum_type: b_ty, ..
            },
        ) => Err(format!(
            "cannot compare enum `{}` with enum `{}`",
            a_ty.name, b_ty.name
        )),
        (a, b) => Err(format!(
            "cannot order compare {} and {}",
            a.type_name(),
            b.type_name()
        )),
    }
}

fn apply_cmp<T: PartialOrd>(op: CmpOp, lhs: &T, rhs: &T) -> bool {
    match op {
        CmpOp::Eq => lhs == rhs,
        CmpOp::Ne => lhs != rhs,
        CmpOp::Lt => lhs < rhs,
        CmpOp::Le => lhs <= rhs,
        CmpOp::Gt => lhs > rhs,
        CmpOp::Ge => lhs >= rhs,
    }
}

fn materialize(value: &EvalValue) -> Result<EvalValue, String> {
    match value {
        EvalValue::Ref(value) => match &*value.borrow() {
            CfcValue::Union { value, .. } => materialize(&EvalValue::Ref(value.clone())),
            CfcValue::Int(value) => Ok(EvalValue::Int(*value)),
            CfcValue::Float(value) => Ok(EvalValue::Float(*value)),
            CfcValue::Bool(value) => Ok(EvalValue::Bool(*value)),
            CfcValue::String(value) => Ok(EvalValue::String(value.clone())),
            CfcValue::Null => Ok(EvalValue::Null),
            CfcValue::Enum {
                enum_type,
                variant,
                value,
            } => Ok(EvalValue::Enum {
                enum_type: enum_type.clone(),
                variant: variant.clone(),
                value: *value,
            }),
            other => Err(format!(
                "expected scalar value, found {}",
                other.type_name()
            )),
        },
        EvalValue::EnumType { .. } | EvalValue::ModuleNamespace { .. } => Err(format!(
            "expected scalar value, found {}",
            value.type_name()
        )),
        other => Ok(other.clone()),
    }
}
