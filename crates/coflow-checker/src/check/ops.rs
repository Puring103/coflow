use std::cmp::Ordering;

use coflow_cft::{CftSchema, CftSchemaBinOp, CftSchemaCmpOp, CftSchemaUnaryOp};
use coflow_data_model::CfdErrorCode;

use super::diagnostics::{bin_op_str, format_value_for_message, unary_op_str};
use super::enum_values;
use super::value::{values_equal, CheckValue, LocatedCheckValue, ValueLocation};

pub(super) struct OpsError {
    code: CfdErrorCode,
    location: Box<Option<ValueLocation>>,
    message: String,
}

impl OpsError {
    pub(super) fn new(
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

    pub(super) fn eval_type(location: Option<ValueLocation>, message: impl Into<String>) -> Self {
        Self::new(CfdErrorCode::CheckEvalTypeError, location, message)
    }

    pub(super) fn into_parts(self) -> (CfdErrorCode, Option<ValueLocation>, String) {
        (self.code, *self.location, self.message)
    }
}

pub(super) type OpsResult<T> = Result<T, OpsError>;

pub(super) fn checked_int(
    value: Option<i64>,
    location: Option<ValueLocation>,
    message: impl Into<String>,
) -> OpsResult<LocatedCheckValue> {
    value
        .map(|value| LocatedCheckValue::new(CheckValue::Int(value), location.clone()))
        .ok_or_else(|| OpsError::new(CfdErrorCode::CheckEvalTypeError, location, message))
}

pub(super) fn checked_shift(
    op: fn(i64, u32) -> Option<i64>,
    lhs: i64,
    rhs: i64,
    location: Option<ValueLocation>,
    message: impl Into<String>,
) -> OpsResult<LocatedCheckValue> {
    let Some(rhs) = rhs.try_into().ok() else {
        return Err(OpsError::new(
            CfdErrorCode::CheckEvalTypeError,
            location,
            message,
        ));
    };
    checked_int(op(lhs, rhs), location, message)
}

pub(super) fn expect_bool_operand(
    value: LocatedCheckValue,
    side: &str,
) -> OpsResult<(bool, Option<ValueLocation>)> {
    let location = value.location.clone();
    let bad_value = value.value.clone();
    let CheckValue::Bool(value) = value.value else {
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

pub(super) fn compare(
    op: CftSchemaCmpOp,
    lhs: &CheckValue,
    rhs: &CheckValue,
    location: Option<ValueLocation>,
) -> OpsResult<bool> {
    Ok(match op {
        CftSchemaCmpOp::Eq => values_equal(lhs, rhs),
        CftSchemaCmpOp::Ne => !values_equal(lhs, rhs),
        CftSchemaCmpOp::Lt => compare_order(lhs, rhs, location)?.is_lt(),
        CftSchemaCmpOp::Le => !compare_order(lhs, rhs, location)?.is_gt(),
        CftSchemaCmpOp::Gt => compare_order(lhs, rhs, location)?.is_gt(),
        CftSchemaCmpOp::Ge => !compare_order(lhs, rhs, location)?.is_lt(),
    })
}

pub(super) fn compare_order(
    lhs: &CheckValue,
    rhs: &CheckValue,
    location: Option<ValueLocation>,
) -> OpsResult<Ordering> {
    if matches!(lhs, CheckValue::Null) || matches!(rhs, CheckValue::Null) {
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
    match (lhs, rhs) {
        (CheckValue::Int(lhs), CheckValue::Int(rhs)) => Ok(lhs.cmp(rhs)),
        (CheckValue::Float(lhs), CheckValue::Float(rhs)) => lhs.partial_cmp(rhs).ok_or_else(|| {
            OpsError::new(
                CfdErrorCode::CheckEvalTypeError,
                location,
                format!("float 比较失败: {lhs} cmp {rhs}"),
            )
        }),
        (CheckValue::Enum(lhs), CheckValue::Enum(rhs)) if lhs.enum_name == rhs.enum_name => {
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

pub(super) fn unary_op(
    schema: &CftSchema,
    op: CftSchemaUnaryOp,
    value: LocatedCheckValue,
) -> OpsResult<LocatedCheckValue> {
    let location = value.location;
    match (op, value.value) {
        (CftSchemaUnaryOp::Not, CheckValue::Bool(value)) => {
            Ok(LocatedCheckValue::new(CheckValue::Bool(!value), location))
        }
        (CftSchemaUnaryOp::Neg, CheckValue::Int(value)) => checked_int(
            value.checked_neg(),
            location,
            format!("整数取负溢出: -({value})"),
        ),
        (CftSchemaUnaryOp::Neg, CheckValue::Float(value)) => {
            Ok(LocatedCheckValue::new(CheckValue::Float(-value), location))
        }
        (CftSchemaUnaryOp::BitNot, CheckValue::Int(value)) => {
            Ok(LocatedCheckValue::new(CheckValue::Int(!value), location))
        }
        (CftSchemaUnaryOp::BitNot, CheckValue::Enum(value)) => Ok(LocatedCheckValue::new(
            CheckValue::Enum(enum_values::enum_with_value(
                schema,
                &value.enum_name,
                !value.value,
            )),
            location,
        )),
        (op, value) => Err(OpsError::eval_type(
            location,
            format!(
                "不支持的一元运算: {} 作用于 {}",
                unary_op_str(op),
                format_value_for_message(&value)
            ),
        )),
    }
}

pub(super) fn eager_bin_op(
    schema: &CftSchema,
    op: CftSchemaBinOp,
    lhs: CheckValue,
    rhs: CheckValue,
    location: Option<ValueLocation>,
) -> OpsResult<LocatedCheckValue> {
    if matches!(lhs, CheckValue::Null) || matches!(rhs, CheckValue::Null) {
        return Err(OpsError::new(
            CfdErrorCode::CheckNullAccess,
            location,
            format!(
                "不能对 null 执行二元运算: {} {} {}",
                format_value_for_message(&lhs),
                bin_op_str(op),
                format_value_for_message(&rhs)
            ),
        ));
    }
    match (op, lhs, rhs) {
        (CftSchemaBinOp::Add, CheckValue::Int(lhs), CheckValue::Int(rhs)) => checked_int(
            lhs.checked_add(rhs),
            location,
            format!("整数加法溢出: {lhs} + {rhs}"),
        ),
        (CftSchemaBinOp::Sub, CheckValue::Int(lhs), CheckValue::Int(rhs)) => checked_int(
            lhs.checked_sub(rhs),
            location,
            format!("整数减法溢出: {lhs} - {rhs}"),
        ),
        (CftSchemaBinOp::Mul, CheckValue::Int(lhs), CheckValue::Int(rhs)) => checked_int(
            lhs.checked_mul(rhs),
            location,
            format!("整数乘法溢出: {lhs} * {rhs}"),
        ),
        (CftSchemaBinOp::Div, CheckValue::Int(lhs), CheckValue::Int(rhs)) => checked_int(
            lhs.checked_div(rhs),
            location,
            format!("整数除法失败: {lhs} / {rhs}"),
        ),
        (CftSchemaBinOp::IntDiv, CheckValue::Int(lhs), CheckValue::Int(rhs)) => checked_int(
            lhs.checked_div(rhs),
            location,
            format!("整数整除失败: {lhs} // {rhs}"),
        ),
        (CftSchemaBinOp::Mod, CheckValue::Int(lhs), CheckValue::Int(rhs)) => checked_int(
            lhs.checked_rem(rhs),
            location,
            format!("整数取模失败: {lhs} % {rhs}"),
        ),
        (CftSchemaBinOp::Pow, CheckValue::Int(lhs), CheckValue::Int(rhs)) => {
            let value = rhs.try_into().ok().and_then(|rhs| lhs.checked_pow(rhs));
            checked_int(value, location, format!("整数幂运算失败: {lhs} ** {rhs}"))
        }
        (CftSchemaBinOp::Shl, CheckValue::Int(lhs), CheckValue::Int(rhs)) => checked_shift(
            i64::checked_shl,
            lhs,
            rhs,
            location,
            format!("整数左移失败: {lhs} << {rhs}"),
        ),
        (CftSchemaBinOp::Shr, CheckValue::Int(lhs), CheckValue::Int(rhs)) => checked_shift(
            i64::checked_shr,
            lhs,
            rhs,
            location,
            format!("整数右移失败: {lhs} >> {rhs}"),
        ),
        (CftSchemaBinOp::Add, CheckValue::Float(lhs), CheckValue::Float(rhs)) => Ok(
            LocatedCheckValue::new(CheckValue::Float(lhs + rhs), location),
        ),
        (CftSchemaBinOp::Sub, CheckValue::Float(lhs), CheckValue::Float(rhs)) => Ok(
            LocatedCheckValue::new(CheckValue::Float(lhs - rhs), location),
        ),
        (CftSchemaBinOp::Mul, CheckValue::Float(lhs), CheckValue::Float(rhs)) => Ok(
            LocatedCheckValue::new(CheckValue::Float(lhs * rhs), location),
        ),
        (CftSchemaBinOp::Div, CheckValue::Float(lhs), CheckValue::Float(rhs)) => Ok(
            LocatedCheckValue::new(CheckValue::Float(lhs / rhs), location),
        ),
        (CftSchemaBinOp::Pow, CheckValue::Float(lhs), CheckValue::Float(rhs)) => Ok(
            LocatedCheckValue::new(CheckValue::Float(lhs.powf(rhs)), location),
        ),
        (CftSchemaBinOp::BitOr, CheckValue::Int(lhs), CheckValue::Int(rhs)) => {
            Ok(LocatedCheckValue::new(CheckValue::Int(lhs | rhs), location))
        }
        (CftSchemaBinOp::BitXor, CheckValue::Int(lhs), CheckValue::Int(rhs)) => {
            Ok(LocatedCheckValue::new(CheckValue::Int(lhs ^ rhs), location))
        }
        (CftSchemaBinOp::BitAnd, CheckValue::Int(lhs), CheckValue::Int(rhs)) => {
            Ok(LocatedCheckValue::new(CheckValue::Int(lhs & rhs), location))
        }
        (CftSchemaBinOp::BitOr, CheckValue::Enum(lhs), CheckValue::Enum(rhs))
            if lhs.enum_name == rhs.enum_name =>
        {
            let value = lhs.value | rhs.value;
            Ok(LocatedCheckValue::new(
                CheckValue::Enum(enum_values::enum_with_value(schema, &lhs.enum_name, value)),
                location,
            ))
        }
        (CftSchemaBinOp::BitXor, CheckValue::Enum(lhs), CheckValue::Enum(rhs))
            if lhs.enum_name == rhs.enum_name =>
        {
            let value = lhs.value ^ rhs.value;
            Ok(LocatedCheckValue::new(
                CheckValue::Enum(enum_values::enum_with_value(schema, &lhs.enum_name, value)),
                location,
            ))
        }
        (CftSchemaBinOp::BitAnd, CheckValue::Enum(lhs), CheckValue::Enum(rhs))
            if lhs.enum_name == rhs.enum_name =>
        {
            let value = lhs.value & rhs.value;
            Ok(LocatedCheckValue::new(
                CheckValue::Enum(enum_values::enum_with_value(schema, &lhs.enum_name, value)),
                location,
            ))
        }
        (op, lhs, rhs) => Err(OpsError::eval_type(
            location,
            format!(
                "不支持的二元运算: {} {} {}",
                format_value_for_message(&lhs),
                bin_op_str(op),
                format_value_for_message(&rhs)
            ),
        )),
    }
}
