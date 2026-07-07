use std::cmp::Ordering;

use coflow_cft::CftSchemaCmpOp;
use coflow_data_model::{CfdErrorCode, CfdPath};

use super::diagnostics::format_value_for_message;
use super::value::{values_equal, CheckValue, LocatedCheckValue};

pub(super) struct OpsError {
    code: CfdErrorCode,
    path: Option<CfdPath>,
    message: String,
}

impl OpsError {
    fn new(code: CfdErrorCode, path: Option<CfdPath>, message: impl Into<String>) -> Self {
        Self {
            code,
            path,
            message: message.into(),
        }
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
