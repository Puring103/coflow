use coflow_cft::{CftSchemaTypeRef, CompiledSchema};
use coflow_data_model::{CfdDataModel, CfdErrorCode, CfdPath, CfdRecord, CfdRecordId};

use super::diagnostics::format_value_for_message;
use super::ops::{OpsError, OpsResult};
use super::value::{CheckRecordRef, CheckValue, LocatedCheckValue};

pub(super) fn field_type_for_record<'a>(
    schema: &'a CompiledSchema,
    model: &CfdDataModel,
    record: &CheckRecordRef,
    name: &str,
) -> Option<&'a CftSchemaTypeRef> {
    record
        .actual_type(model)
        .and_then(|actual_type| schema.field_type(actual_type, name))
}

pub(super) fn current_field(
    schema: &CompiledSchema,
    model: &CfdDataModel,
    root_record: Option<CfdRecordId>,
    root_path: &CfdPath,
    current: &CheckValue,
    name: &str,
) -> Option<LocatedCheckValue> {
    let CheckValue::Record(record) = current else {
        return None;
    };
    if name == "id" {
        return virtual_id(model, root_record, root_path, record, record.path());
    }
    record.field(
        model,
        field_type_for_record(schema, model, record, name),
        name,
    )
}

pub(super) fn field_value(
    schema: &CompiledSchema,
    model: &CfdDataModel,
    root_record: Option<CfdRecordId>,
    root_path: &CfdPath,
    target: LocatedCheckValue,
    name: &str,
) -> OpsResult<LocatedCheckValue> {
    if matches!(target.value, CheckValue::Null) {
        return Err(OpsError::new(
            CfdErrorCode::CheckNullAccess,
            target.path,
            format!("不能访问 null 的字段: 尝试在 null 上读取 `.{name}`"),
        ));
    }
    match target.value {
        CheckValue::Record(record) => {
            if name == "id" {
                return virtual_id(model, root_record, root_path, &record, target.path)
                    .ok_or_else(|| OpsError::eval_type(None, "记录没有虚拟 id"));
            }
            record
                .field(
                    model,
                    field_type_for_record(schema, model, &record, name),
                    name,
                )
                .ok_or_else(|| OpsError::eval_type(target.path, format!("记录没有字段 `{name}`")))
        }
        CheckValue::Entry(entry) => match name {
            "key" => Ok(LocatedCheckValue::new(*entry.key, target.path)),
            "value" => Ok(LocatedCheckValue::new(entry.value, target.path)),
            _ => Err(OpsError::eval_type(
                target.path,
                format!("dict entry 没有字段 `{name}`，只有 `key` 和 `value`"),
            )),
        },
        other => Err(OpsError::eval_type(
            target.path,
            format!(
                "字段访问目标不是对象: 读取 `.{name}` 时实际为 {}",
                format_value_for_message(&other)
            ),
        )),
    }
}

pub(super) fn virtual_id(
    model: &CfdDataModel,
    root_record: Option<CfdRecordId>,
    root_path: &CfdPath,
    record: &CheckRecordRef,
    path: Option<CfdPath>,
) -> Option<LocatedCheckValue> {
    let key = record
        .key(model)
        .filter(|key| !key.is_empty())
        .or_else(|| root_record.and_then(|id| model.record(id).map(CfdRecord::key)))?;
    let key = key.to_string();
    let path = path
        .map(|path| path.field("id"))
        .or_else(|| Some(root_path.clone().field("id")));
    Some(LocatedCheckValue::new(CheckValue::String(key), path))
}
