use std::cmp::Ordering;

use coflow_diagnostics::CfdErrorCode;
use coflow_language::cft::{
    CftSchema, CftSchemaBinOp, CftSchemaCmpOp, CftSchemaUnaryOp, CftValueType,
};

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
    if let Some(ty @ (CftValueType::Float | CftValueType::Double)) = lhs
        .numeric_type()
        .zip(rhs.numeric_type())
        .and_then(|(a, b)| a.promote_numeric(&b))
    {
        return lhs
            .numeric_float(&ty)
            .unwrap()
            .partial_cmp(&rhs.numeric_float(&ty).unwrap())
            .ok_or_else(|| OpsError::eval_type(location, "numeric comparison failed"));
    }

    if matches!(lhs.scalar(), Some(ScalarValue::Null))
        || matches!(rhs.scalar(), Some(ScalarValue::Null))
    {
        return Err(OpsError::new(
            CfdErrorCode::CheckEvalTypeError,
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
        (Some(ScalarValue::Bool(lhs)), Some(ScalarValue::Bool(rhs))) => Ok(lhs.cmp(&rhs)),
        (Some(ScalarValue::String(lhs)), Some(ScalarValue::String(rhs))) => Ok(lhs.cmp(rhs)),
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

fn unary_op_raw<'model>(
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
            Ok(LocatedEvalValue::new(EvalValue::double(-value), location))
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
fn eager_bin_op_raw<'model>(
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
            CfdErrorCode::CheckEvalTypeError,
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
        (CftSchemaBinOp::Add, Some(ScalarValue::Float(lhs)), Some(ScalarValue::Float(rhs))) => Ok(
            LocatedEvalValue::new(EvalValue::double(lhs + rhs), location),
        ),
        (CftSchemaBinOp::Sub, Some(ScalarValue::Float(lhs)), Some(ScalarValue::Float(rhs))) => Ok(
            LocatedEvalValue::new(EvalValue::double(lhs - rhs), location),
        ),
        (CftSchemaBinOp::Mul, Some(ScalarValue::Float(lhs)), Some(ScalarValue::Float(rhs))) => Ok(
            LocatedEvalValue::new(EvalValue::double(lhs * rhs), location),
        ),
        (CftSchemaBinOp::Div, Some(ScalarValue::Float(lhs)), Some(ScalarValue::Float(rhs))) => Ok(
            LocatedEvalValue::new(EvalValue::double(lhs / rhs), location),
        ),
        (CftSchemaBinOp::Pow, Some(ScalarValue::Float(lhs)), Some(ScalarValue::Float(rhs))) => Ok(
            LocatedEvalValue::new(EvalValue::double(lhs.powf(rhs)), location),
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

// 先按共同类型转换操作数，再按结果类型检查范围并舍入，禁止运行时悄悄升级位宽。
pub(crate) fn normalize_numeric<'a>(
    mut result: LocatedEvalValue<'a>,
    ty: Option<CftValueType>,
) -> OpsResult<LocatedEvalValue<'a>> {
    result.value = match (ty, result.value.scalar()) {
        (Some(CftValueType::Int), Some(ScalarValue::Int(v))) => {
            if i32::try_from(v).is_err() {
                return Err(OpsError::eval_type(
                    result.location,
                    "32-bit integer overflow",
                ));
            }
            EvalValue::int(v)
        }
        (Some(CftValueType::Long), Some(ScalarValue::Int(v))) => EvalValue::long(v),
        (Some(CftValueType::Float), Some(ScalarValue::Float(v))) => EvalValue::float(v),
        (Some(CftValueType::Double), Some(ScalarValue::Float(v))) => EvalValue::double(v),
        _ => return Ok(result),
    };
    Ok(result)
}

pub(crate) fn unary_op<'a>(
    schema: &CftSchema,
    op: CftSchemaUnaryOp,
    value: LocatedEvalValue<'a>,
) -> OpsResult<LocatedEvalValue<'a>> {
    let ty = value.value.numeric_type();
    normalize_numeric(unary_op_raw(schema, op, value)?, ty)
}

pub(crate) fn eager_bin_op<'a>(
    schema: &CftSchema,
    op: CftSchemaBinOp,
    lhs: &EvalValue<'a>,
    rhs: &EvalValue<'a>,
    location: Option<ValueLocation>,
) -> OpsResult<LocatedEvalValue<'a>> {
    let ty = lhs
        .numeric_type()
        .zip(rhs.numeric_type())
        .and_then(|(a, b)| a.promote_numeric(&b));
    if ty == Some(CftValueType::Float) {
        let left = lhs.numeric_float(&CftValueType::Float).unwrap() as f32;
        let right = rhs.numeric_float(&CftValueType::Float).unwrap() as f32;
        let result = match op {
            CftSchemaBinOp::Add => left + right,
            CftSchemaBinOp::Sub => left - right,
            CftSchemaBinOp::Mul => left * right,
            CftSchemaBinOp::Div => left / right,
            CftSchemaBinOp::Pow => left.powf(right),
            _ => {
                return Err(OpsError::eval_type(
                    location,
                    "operator requires integer operands",
                ))
            }
        };
        return Ok(LocatedEvalValue::new(
            EvalValue::float(f64::from(result)),
            location,
        ));
    }
    if let Some(ty @ CftValueType::Double) = &ty {
        let left = EvalValue::double(lhs.numeric_float(ty).unwrap());
        let right = EvalValue::double(rhs.numeric_float(ty).unwrap());
        return normalize_numeric(
            eager_bin_op_raw(schema, op, &left, &right, location)?,
            Some(ty.clone()),
        );
    }
    if ty == Some(CftValueType::Int) && matches!(op, CftSchemaBinOp::Shl | CftSchemaBinOp::Shr) {
        if let Some(ScalarValue::Int(shift)) = rhs.scalar() {
            if !(0..32).contains(&shift) {
                return Err(OpsError::eval_type(
                    location,
                    "int shift count must be between 0 and 31",
                ));
            }
        }
    }
    normalize_numeric(eager_bin_op_raw(schema, op, lhs, rhs, location)?, ty)
}
