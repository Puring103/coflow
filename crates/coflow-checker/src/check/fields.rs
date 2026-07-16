use coflow_cft::{CftSchema, CftValueType};
use coflow_data_model::{CfdDataModel, CfdErrorCode};
use coflow_structure::StructuralBudget;

use super::diagnostics::format_value_for_message;
use super::ops::{OpsError, OpsResult};
use super::value::{
    CheckRecordRef, CheckValue, LocatedBudgetExceeded, LocatedCheckValue, ValueLocation,
};

pub(super) fn field_type_for_record<'a>(
    schema: &'a CftSchema,
    model: &CfdDataModel,
    record: &CheckRecordRef,
    name: &str,
) -> Option<&'a CftValueType> {
    record
        .actual_type(model)
        .and_then(|actual_type| schema.field(actual_type, name))
        .map(|field| &field.value_type)
}

pub(super) fn current_field(
    schema: &CftSchema,
    model: &CfdDataModel,
    current: &CheckValue,
    name: &str,
    budget: &mut StructuralBudget,
) -> OpsResult<Option<LocatedCheckValue>> {
    let CheckValue::Record(record) = current else {
        return Ok(None);
    };
    if name == "id" {
        return Ok(virtual_id(model, record, record.location()));
    }
    record
        .field(
            model,
            field_type_for_record(schema, model, record, name),
            name,
            budget,
        )
        .map_err(budget_error)
}

pub(super) fn field_value(
    schema: &CftSchema,
    model: &CfdDataModel,
    target: LocatedCheckValue,
    name: &str,
    budget: &mut StructuralBudget,
) -> OpsResult<LocatedCheckValue> {
    if matches!(target.value, CheckValue::Null) {
        return Err(OpsError::new(
            CfdErrorCode::CheckNullAccess,
            target.location,
            format!("不能访问 null 的字段: 尝试在 null 上读取 `.{name}`"),
        ));
    }
    match target.value {
        CheckValue::Record(record) => {
            if name == "id" {
                return virtual_id(model, &record, record.location())
                    .ok_or_else(|| OpsError::eval_type(None, "记录没有虚拟 id"));
            }
            record
                .field(
                    model,
                    field_type_for_record(schema, model, &record, name),
                    name,
                    budget,
                )
                .map_err(budget_error)?
                .ok_or_else(|| {
                    OpsError::eval_type(target.location, format!("记录没有字段 `{name}`"))
                })
        }
        CheckValue::Entry(entry) => match name {
            "key" => Ok(LocatedCheckValue::new(*entry.key, target.location)),
            "value" => Ok(LocatedCheckValue::new(entry.value, target.location)),
            _ => Err(OpsError::eval_type(
                target.location,
                format!("dict entry 没有字段 `{name}`，只有 `key` 和 `value`"),
            )),
        },
        other => Err(OpsError::eval_type(
            target.location,
            format!(
                "字段访问目标不是对象: 读取 `.{name}` 时实际为 {}",
                format_value_for_message(&other)
            ),
        )),
    }
}

pub(super) fn virtual_id(
    model: &CfdDataModel,
    record: &CheckRecordRef,
    location: Option<ValueLocation>,
) -> Option<LocatedCheckValue> {
    let key = record.key(model).filter(|key| !key.is_empty())?;
    let key = key.to_string();
    let location = location.map(|location| location.field("id"));
    Some(LocatedCheckValue::new(CheckValue::String(key), location))
}

fn budget_error(exceeded: LocatedBudgetExceeded) -> OpsError {
    OpsError::new(
        CfdErrorCode::CheckBudgetExceeded,
        *exceeded.location,
        exceeded.error.to_string(),
    )
}
