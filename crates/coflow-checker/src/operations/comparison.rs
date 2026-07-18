use std::cmp::Ordering;

use coflow_cft::{CftSchema, CftSchemaBinOp, CftSchemaCmpOp, CftSchemaUnaryOp};
use coflow_data_model::CfdErrorCode;

use super::builtins;
use super::diagnostics::{bin_op_str, format_value_for_message, unary_op_str};
use super::value::{values_equal, EvalValue, LocatedEvalValue, ScalarValue, ValueLocation};

#[derive(Debug)]
pub(crate) struct OpsError {
    code: CfdErrorCode,
    location: Box<Option<ValueLocation>>,
    message: String,
}

impl OpsError {
    pub(crate) fn new(
        code: CfdErrorCode,
        location: Option<ValueLocation>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            location: Box::new(location),
            message: message.into(),
        }
    }

    pub(crate) fn eval_type(location: Option<ValueLocation>, message: impl Into<String>) -> Self {
        Self::new(CfdErrorCode::CheckEvalTypeError, location, message)
    }

    pub(crate) fn into_parts(self) -> (CfdErrorCode, Option<ValueLocation>, String) {
        (self.code, *self.location, self.message)
    }
}

pub(crate) type OpsResult<T> = Result<T, OpsError>;

pub(crate) fn checked_int<'model>(
    value: Option<i64>,
    location: Option<ValueLocation>,
    message: impl Into<String>,
) -> OpsResult<LocatedEvalValue<'model>> {
    value
        .map(|value| LocatedEvalValue::new(EvalValue::int(value), location.clone()))
        .ok_or_else(|| OpsError::new(CfdErrorCode::CheckEvalTypeError, location, message))
}

pub(crate) fn checked_shift<'model>(
    op: fn(i64, u32) -> Option<i64>,
    lhs: i64,
    rhs: i64,
    location: Option<ValueLocation>,
    message: impl Into<String>,
) -> OpsResult<LocatedEvalValue<'model>> {
    let Some(rhs) = rhs.try_into().ok() else {
        return Err(OpsError::new(
            CfdErrorCode::CheckEvalTypeError,
            location,
            message,
        ));
    };
    checked_int(op(lhs, rhs), location, message)
}

pub(crate) fn expect_bool_operand(
    value: &LocatedEvalValue<'_>,
    side: &str,
) -> OpsResult<(bool, Option<ValueLocation>)> {
    let location = value.location.clone();
    let bad_value = value.value.clone();
    let Some(ScalarValue::Bool(value)) = value.value.scalar() else {
        return Err(OpsError::eval_type(
            location,
            format!(
                "{side}操作数不是 bool: 实际为 {}",
                format_value_for_message(&bad_value)
            ),
        ));
    };
    Ok((value, location))
}

pub(crate) fn compare(
    op: CftSchemaCmpOp,
    lhs: &EvalValue<'_>,
    rhs: &EvalValue<'_>,
    location: Option<ValueLocation>,
) -> OpsResult<bool> {
    if matches!(lhs.scalar(), Some(ScalarValue::Float(value)) if value.is_nan())
        || matches!(rhs.scalar(), Some(ScalarValue::Float(value)) if value.is_nan())
    {
        return Err(OpsError::new(
            CfdErrorCode::CheckEvalTypeError,
            location,
            format!(
                "float 比较失败: {} cmp {}",
                format_value_for_message(lhs),
                format_value_for_message(rhs)
            ),
        ));
    }
    Ok(match op {
        CftSchemaCmpOp::Eq => values_equal(lhs, rhs),
        CftSchemaCmpOp::Ne => !values_equal(lhs, rhs),
        CftSchemaCmpOp::Lt => compare_order(lhs, rhs, location)?.is_lt(),
        CftSchemaCmpOp::Le => !compare_order(lhs, rhs, location)?.is_gt(),
        CftSchemaCmpOp::Gt => compare_order(lhs, rhs, location)?.is_gt(),
        CftSchemaCmpOp::Ge => !compare_order(lhs, rhs, location)?.is_lt(),
    })
}

pub(crate) fn compare_order(
    lhs: &EvalValue<'_>,
    rhs: &EvalValue<'_>,
    location: Option<ValueLocation>,
) -> OpsResult<Ordering> {
    if matches!(lhs.scalar(), Some(ScalarValue::Null))
        || matches!(rhs.scalar(), Some(ScalarValue::Null))
    {
        return Err(OpsError::new(
            CfdErrorCode::CheckNullAccess,
            location,
            format!(
                "不能对 null 做有序比较: {} cmp {}",
                format_value_for_message(lhs),
                format_value_for_message(rhs)
            ),
        ));
    }
    match (lhs.scalar(), rhs.scalar()) {
        (Some(ScalarValue::Int(lhs)), Some(ScalarValue::Int(rhs))) => Ok(lhs.cmp(&rhs)),
        (Some(ScalarValue::Float(lhs)), Some(ScalarValue::Float(rhs))) => {
            lhs.partial_cmp(&rhs).ok_or_else(|| {
                OpsError::new(
                    CfdErrorCode::CheckEvalTypeError,
                    location,
                    format!("float 比较失败: {lhs} cmp {rhs}"),
                )
            })
        }
        (Some(ScalarValue::Enum(lhs)), Some(ScalarValue::Enum(rhs)))
            if lhs.enum_name == rhs.enum_name =>
        {
            Ok(lhs.value.cmp(&rhs.value))
        }
        _ => Err(OpsError::new(
            CfdErrorCode::CheckEvalTypeError,
            location,
            format!(
                "值不可做有序比较: {} cmp {}",
                format_value_for_message(lhs),
                format_value_for_message(rhs)
            ),
        )),
    }
}

pub(crate) fn unary_op<'model>(
    schema: &CftSchema,
    op: CftSchemaUnaryOp,
    value: LocatedEvalValue<'model>,
) -> OpsResult<LocatedEvalValue<'model>> {
    let location = value.location;
    match (op, value.value.scalar()) {
        (CftSchemaUnaryOp::Not, Some(ScalarValue::Bool(value))) => {
            Ok(LocatedEvalValue::new(EvalValue::bool(!value), location))
        }
        (CftSchemaUnaryOp::Neg, Some(ScalarValue::Int(value))) => checked_int(
            value.checked_neg(),
            location,
            format!("整数取负溢出: -({value})"),
        ),
        (CftSchemaUnaryOp::Neg, Some(ScalarValue::Float(value))) => {
            Ok(LocatedEvalValue::new(EvalValue::float(-value), location))
        }
        (CftSchemaUnaryOp::BitNot, Some(ScalarValue::Int(value))) => {
            Ok(LocatedEvalValue::new(EvalValue::int(!value), location))
        }
        (CftSchemaUnaryOp::BitNot, Some(ScalarValue::Enum(value))) => Ok(LocatedEvalValue::new(
            EvalValue::enum_value(builtins::enum_with_value(
                schema,
                &value.enum_name,
                !value.value,
            )),
            location,
        )),
        (op, _) => Err(OpsError::eval_type(
            location,
            format!(
                "不支持的一元运算: {} 作用于 {}",
                unary_op_str(op),
                format_value_for_message(&value.value)
            ),
        )),
    }
}

#[allow(clippy::too_many_lines)]
pub(crate) fn eager_bin_op<'model>(
    schema: &CftSchema,
    op: CftSchemaBinOp,
    lhs: &EvalValue<'model>,
    rhs: &EvalValue<'model>,
    location: Option<ValueLocation>,
) -> OpsResult<LocatedEvalValue<'model>> {
    if matches!(lhs.scalar(), Some(ScalarValue::Null))
        || matches!(rhs.scalar(), Some(ScalarValue::Null))
    {
        return Err(OpsError::new(
            CfdErrorCode::CheckNullAccess,
            location,
            format!(
                "不能对 null 执行二元运算: {} {} {}",
                format_value_for_message(lhs),
                bin_op_str(op),
                format_value_for_message(rhs)
            ),
        ));
    }
    match (op, lhs.scalar(), rhs.scalar()) {
        (CftSchemaBinOp::Add, Some(ScalarValue::Int(lhs)), Some(ScalarValue::Int(rhs))) => {
            checked_int(
                lhs.checked_add(rhs),
                location,
                format!("整数加法溢出: {lhs} + {rhs}"),
            )
        }
        (CftSchemaBinOp::Sub, Some(ScalarValue::Int(lhs)), Some(ScalarValue::Int(rhs))) => {
            checked_int(
                lhs.checked_sub(rhs),
                location,
                format!("整数减法溢出: {lhs} - {rhs}"),
            )
        }
        (CftSchemaBinOp::Mul, Some(ScalarValue::Int(lhs)), Some(ScalarValue::Int(rhs))) => {
            checked_int(
                lhs.checked_mul(rhs),
                location,
                format!("整数乘法溢出: {lhs} * {rhs}"),
            )
        }
        (CftSchemaBinOp::Div, Some(ScalarValue::Int(lhs)), Some(ScalarValue::Int(rhs))) => {
            checked_int(
                lhs.checked_div(rhs),
                location,
                format!("整数除法失败: {lhs} / {rhs}"),
            )
        }
        (CftSchemaBinOp::IntDiv, Some(ScalarValue::Int(lhs)), Some(ScalarValue::Int(rhs))) => {
            checked_int(
                lhs.checked_div(rhs),
                location,
                format!("整数整除失败: {lhs} // {rhs}"),
            )
        }
        (CftSchemaBinOp::Mod, Some(ScalarValue::Int(lhs)), Some(ScalarValue::Int(rhs))) => {
            checked_int(
                lhs.checked_rem(rhs),
                location,
                format!("整数取模失败: {lhs} % {rhs}"),
            )
        }
        (CftSchemaBinOp::Pow, Some(ScalarValue::Int(lhs)), Some(ScalarValue::Int(rhs))) => {
            let value = rhs.try_into().ok().and_then(|rhs| lhs.checked_pow(rhs));
            checked_int(value, location, format!("整数幂运算失败: {lhs} ** {rhs}"))
        }
        (CftSchemaBinOp::Shl, Some(ScalarValue::Int(lhs)), Some(ScalarValue::Int(rhs))) => {
            checked_shift(
                i64::checked_shl,
                lhs,
                rhs,
                location,
                format!("整数左移失败: {lhs} << {rhs}"),
            )
        }
        (CftSchemaBinOp::Shr, Some(ScalarValue::Int(lhs)), Some(ScalarValue::Int(rhs))) => {
            checked_shift(
                i64::checked_shr,
                lhs,
                rhs,
                location,
                format!("整数右移失败: {lhs} >> {rhs}"),
            )
        }
        (CftSchemaBinOp::Add, Some(ScalarValue::Float(lhs)), Some(ScalarValue::Float(rhs))) => {
            Ok(LocatedEvalValue::new(EvalValue::float(lhs + rhs), location))
        }
        (CftSchemaBinOp::Sub, Some(ScalarValue::Float(lhs)), Some(ScalarValue::Float(rhs))) => {
            Ok(LocatedEvalValue::new(EvalValue::float(lhs - rhs), location))
        }
        (CftSchemaBinOp::Mul, Some(ScalarValue::Float(lhs)), Some(ScalarValue::Float(rhs))) => {
            Ok(LocatedEvalValue::new(EvalValue::float(lhs * rhs), location))
        }
        (CftSchemaBinOp::Div, Some(ScalarValue::Float(lhs)), Some(ScalarValue::Float(rhs))) => {
            Ok(LocatedEvalValue::new(EvalValue::float(lhs / rhs), location))
        }
        (CftSchemaBinOp::Pow, Some(ScalarValue::Float(lhs)), Some(ScalarValue::Float(rhs))) => Ok(
            LocatedEvalValue::new(EvalValue::float(lhs.powf(rhs)), location),
        ),
        (CftSchemaBinOp::BitOr, Some(ScalarValue::Int(lhs)), Some(ScalarValue::Int(rhs))) => {
            Ok(LocatedEvalValue::new(EvalValue::int(lhs | rhs), location))
        }
        (CftSchemaBinOp::BitXor, Some(ScalarValue::Int(lhs)), Some(ScalarValue::Int(rhs))) => {
            Ok(LocatedEvalValue::new(EvalValue::int(lhs ^ rhs), location))
        }
        (CftSchemaBinOp::BitAnd, Some(ScalarValue::Int(lhs)), Some(ScalarValue::Int(rhs))) => {
            Ok(LocatedEvalValue::new(EvalValue::int(lhs & rhs), location))
        }
        (CftSchemaBinOp::BitOr, Some(ScalarValue::Enum(lhs)), Some(ScalarValue::Enum(rhs)))
            if lhs.enum_name == rhs.enum_name =>
        {
            let value = lhs.value | rhs.value;
            Ok(LocatedEvalValue::new(
                EvalValue::enum_value(builtins::enum_with_value(schema, &lhs.enum_name, value)),
                location,
            ))
        }
        (CftSchemaBinOp::BitXor, Some(ScalarValue::Enum(lhs)), Some(ScalarValue::Enum(rhs)))
            if lhs.enum_name == rhs.enum_name =>
        {
            let value = lhs.value ^ rhs.value;
            Ok(LocatedEvalValue::new(
                EvalValue::enum_value(builtins::enum_with_value(schema, &lhs.enum_name, value)),
                location,
            ))
        }
        (CftSchemaBinOp::BitAnd, Some(ScalarValue::Enum(lhs)), Some(ScalarValue::Enum(rhs)))
            if lhs.enum_name == rhs.enum_name =>
        {
            let value = lhs.value & rhs.value;
            Ok(LocatedEvalValue::new(
                EvalValue::enum_value(builtins::enum_with_value(schema, &lhs.enum_name, value)),
                location,
            ))
        }
        (op, _, _) => Err(OpsError::eval_type(
            location,
            format!(
                "不支持的二元运算: {} {} {}",
                format_value_for_message(lhs),
                bin_op_str(op),
                format_value_for_message(rhs)
            ),
        )),
    }
}
