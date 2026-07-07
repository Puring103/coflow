use std::cmp::Ordering;

use coflow_cft::{CftSchemaBinOp, CftSchemaCmpOp, CftSchemaView};
use coflow_data_model::{CfdErrorCode, CfdPath};

use super::diagnostics::{bin_op_str, format_value_for_message};
use super::enum_values;
use super::value::{values_equal, CheckValue, LocatedCheckValue};

pub(super) struct OpsError {
    code: CfdErrorCode,
    path: Option<CfdPath>,
    message: String,
}

impl OpsError {
    pub(super) fn new(
        code: CfdErrorCode,
        path: Option<CfdPath>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            path,
            message: message.into(),
        }
    }

    pub(super) fn eval_type(path: Option<CfdPath>, message: impl Into<String>) -> Self {
        Self::new(CfdErrorCode::CheckEvalTypeError, path, message)
    }

    pub(super) fn into_parts(self) -> (CfdErrorCode, Option<CfdPath>, String) {
        (self.code, self.path, self.message)
    }
}

pub(super) type OpsResult<T> = Result<T, OpsError>;

pub(super) fn checked_int(
    value: Option<i64>,
    path: Option<CfdPath>,
    message: impl Into<String>,
) -> OpsResult<LocatedCheckValue> {
    value
        .map(|value| LocatedCheckValue::new(CheckValue::Int(value), path.clone()))
        .ok_or_else(|| OpsError::new(CfdErrorCode::CheckEvalTypeError, path, message))
}

pub(super) fn checked_shift(
    op: fn(i64, u32) -> Option<i64>,
    lhs: i64,
    rhs: i64,
    path: Option<CfdPath>,
    message: impl Into<String>,
) -> OpsResult<LocatedCheckValue> {
    let Some(rhs) = rhs.try_into().ok() else {
        return Err(OpsError::new(
            CfdErrorCode::CheckEvalTypeError,
            path,
            message,
        ));
    };
    checked_int(op(lhs, rhs), path, message)
}

pub(super) fn compare(
    op: CftSchemaCmpOp,
    lhs: &CheckValue,
    rhs: &CheckValue,
    path: Option<CfdPath>,
) -> OpsResult<bool> {
    Ok(match op {
        CftSchemaCmpOp::Eq => values_equal(lhs, rhs),
        CftSchemaCmpOp::Ne => !values_equal(lhs, rhs),
        CftSchemaCmpOp::Lt => compare_order(lhs, rhs, path)?.is_lt(),
        CftSchemaCmpOp::Le => !compare_order(lhs, rhs, path)?.is_gt(),
        CftSchemaCmpOp::Gt => compare_order(lhs, rhs, path)?.is_gt(),
        CftSchemaCmpOp::Ge => !compare_order(lhs, rhs, path)?.is_lt(),
    })
}

pub(super) fn compare_order(
    lhs: &CheckValue,
    rhs: &CheckValue,
    path: Option<CfdPath>,
) -> OpsResult<Ordering> {
    if matches!(lhs, CheckValue::Null) || matches!(rhs, CheckValue::Null) {
        return Err(OpsError::new(
            CfdErrorCode::CheckNullAccess,
            path,
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
                path,
                format!("float 比较失败: {lhs} cmp {rhs}"),
            )
        }),
        (CheckValue::Enum(lhs), CheckValue::Enum(rhs)) if lhs.enum_name == rhs.enum_name => {
            Ok(lhs.value.cmp(&rhs.value))
        }
        _ => Err(OpsError::new(
            CfdErrorCode::CheckEvalTypeError,
            path,
            format!(
                "值不可做有序比较: {} cmp {}",
                format_value_for_message(lhs),
                format_value_for_message(rhs)
            ),
        )),
    }
}

pub(super) fn eager_bin_op(
    schema: &CftSchemaView,
    op: CftSchemaBinOp,
    lhs: CheckValue,
    rhs: CheckValue,
    path: Option<CfdPath>,
) -> OpsResult<LocatedCheckValue> {
    if matches!(lhs, CheckValue::Null) || matches!(rhs, CheckValue::Null) {
        return Err(OpsError::new(
            CfdErrorCode::CheckNullAccess,
            path,
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
            path,
            format!("整数加法溢出: {lhs} + {rhs}"),
        ),
        (CftSchemaBinOp::Sub, CheckValue::Int(lhs), CheckValue::Int(rhs)) => checked_int(
            lhs.checked_sub(rhs),
            path,
            format!("整数减法溢出: {lhs} - {rhs}"),
        ),
        (CftSchemaBinOp::Mul, CheckValue::Int(lhs), CheckValue::Int(rhs)) => checked_int(
            lhs.checked_mul(rhs),
            path,
            format!("整数乘法溢出: {lhs} * {rhs}"),
        ),
        (CftSchemaBinOp::Div, CheckValue::Int(lhs), CheckValue::Int(rhs)) => checked_int(
            lhs.checked_div(rhs),
            path,
            format!("整数除法失败: {lhs} / {rhs}"),
        ),
        (CftSchemaBinOp::IntDiv, CheckValue::Int(lhs), CheckValue::Int(rhs)) => checked_int(
            lhs.checked_div(rhs),
            path,
            format!("整数整除失败: {lhs} // {rhs}"),
        ),
        (CftSchemaBinOp::Mod, CheckValue::Int(lhs), CheckValue::Int(rhs)) => checked_int(
            lhs.checked_rem(rhs),
            path,
            format!("整数取模失败: {lhs} % {rhs}"),
        ),
        (CftSchemaBinOp::Pow, CheckValue::Int(lhs), CheckValue::Int(rhs)) => {
            let value = rhs.try_into().ok().and_then(|rhs| lhs.checked_pow(rhs));
            checked_int(value, path, format!("整数幂运算失败: {lhs} ** {rhs}"))
        }
        (CftSchemaBinOp::Shl, CheckValue::Int(lhs), CheckValue::Int(rhs)) => checked_shift(
            i64::checked_shl,
            lhs,
            rhs,
            path,
            format!("整数左移失败: {lhs} << {rhs}"),
        ),
        (CftSchemaBinOp::Shr, CheckValue::Int(lhs), CheckValue::Int(rhs)) => checked_shift(
            i64::checked_shr,
            lhs,
            rhs,
            path,
            format!("整数右移失败: {lhs} >> {rhs}"),
        ),
        (CftSchemaBinOp::Add, CheckValue::Float(lhs), CheckValue::Float(rhs)) => {
            Ok(LocatedCheckValue::new(CheckValue::Float(lhs + rhs), path))
        }
        (CftSchemaBinOp::Sub, CheckValue::Float(lhs), CheckValue::Float(rhs)) => {
            Ok(LocatedCheckValue::new(CheckValue::Float(lhs - rhs), path))
        }
        (CftSchemaBinOp::Mul, CheckValue::Float(lhs), CheckValue::Float(rhs)) => {
            Ok(LocatedCheckValue::new(CheckValue::Float(lhs * rhs), path))
        }
        (CftSchemaBinOp::Div, CheckValue::Float(lhs), CheckValue::Float(rhs)) => {
            Ok(LocatedCheckValue::new(CheckValue::Float(lhs / rhs), path))
        }
        (CftSchemaBinOp::Pow, CheckValue::Float(lhs), CheckValue::Float(rhs)) => Ok(
            LocatedCheckValue::new(CheckValue::Float(lhs.powf(rhs)), path),
        ),
        (CftSchemaBinOp::BitOr, CheckValue::Int(lhs), CheckValue::Int(rhs)) => {
            Ok(LocatedCheckValue::new(CheckValue::Int(lhs | rhs), path))
        }
        (CftSchemaBinOp::BitXor, CheckValue::Int(lhs), CheckValue::Int(rhs)) => {
            Ok(LocatedCheckValue::new(CheckValue::Int(lhs ^ rhs), path))
        }
        (CftSchemaBinOp::BitAnd, CheckValue::Int(lhs), CheckValue::Int(rhs)) => {
            Ok(LocatedCheckValue::new(CheckValue::Int(lhs & rhs), path))
        }
        (CftSchemaBinOp::BitOr, CheckValue::Enum(lhs), CheckValue::Enum(rhs))
            if lhs.enum_name == rhs.enum_name =>
        {
            let value = lhs.value | rhs.value;
            Ok(LocatedCheckValue::new(
                CheckValue::Enum(enum_values::enum_with_value(
                    schema,
                    &lhs.enum_name,
                    value,
                )),
                path,
            ))
        }
        (CftSchemaBinOp::BitXor, CheckValue::Enum(lhs), CheckValue::Enum(rhs))
            if lhs.enum_name == rhs.enum_name =>
        {
            let value = lhs.value ^ rhs.value;
            Ok(LocatedCheckValue::new(
                CheckValue::Enum(enum_values::enum_with_value(
                    schema,
                    &lhs.enum_name,
                    value,
                )),
                path,
            ))
        }
        (CftSchemaBinOp::BitAnd, CheckValue::Enum(lhs), CheckValue::Enum(rhs))
            if lhs.enum_name == rhs.enum_name =>
        {
            let value = lhs.value & rhs.value;
            Ok(LocatedCheckValue::new(
                CheckValue::Enum(enum_values::enum_with_value(
                    schema,
                    &lhs.enum_name,
                    value,
                )),
                path,
            ))
        }
        (op, lhs, rhs) => Err(OpsError::eval_type(
            path,
            format!(
                "不支持的二元运算: {} {} {}",
                format_value_for_message(&lhs),
                bin_op_str(op),
                format_value_for_message(&rhs)
            ),
        )),
    }
}
