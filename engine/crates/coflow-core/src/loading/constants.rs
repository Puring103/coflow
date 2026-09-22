use crate::schema::CftStaticValue;
use crate::{LoadedDictKeyDraft, LoadedValueDraft};

/// 常量复用只复制静态存储值；模板与函数始终保留原始源码。
pub(super) fn materialize(value: &CftStaticValue) -> Result<LoadedValueDraft, String> {
    Ok(match value {
        CftStaticValue::Int(v) => LoadedValueDraft::Int(*v),
        CftStaticValue::Float(v) => LoadedValueDraft::Float(*v),
        CftStaticValue::Bool(v) => LoadedValueDraft::Bool(*v),
        CftStaticValue::String(v) => LoadedValueDraft::String(v.clone()),
        CftStaticValue::FormattedString(source) => {
            LoadedValueDraft::FormattedString(source.into())
        }
        CftStaticValue::Function(source) => LoadedValueDraft::Function(source.into()),
        CftStaticValue::Enum {
            enum_name, value, ..
        } => LoadedValueDraft::enum_value(enum_name.to_string(), *value),
        CftStaticValue::OptionNone => LoadedValueDraft::OptionNone,
        CftStaticValue::OptionSome(v) => LoadedValueDraft::OptionSome(Box::new(materialize(v)?)),
        CftStaticValue::Array(values) => {
            LoadedValueDraft::Array(values.iter().map(materialize).collect::<Result<_, _>>()?)
        }
        CftStaticValue::Dictionary(values) => LoadedValueDraft::Dict(
            values
                .iter()
                .map(|(key, value)| {
                    let key = match key {
                        CftStaticValue::Int(v) => LoadedDictKeyDraft::Int(*v),
                        CftStaticValue::Bool(v) => LoadedDictKeyDraft::Bool(*v),
                        CftStaticValue::String(v) => LoadedDictKeyDraft::String(v.clone()),
                        CftStaticValue::Enum {
                            enum_name, variant, ..
                        } => LoadedDictKeyDraft::enum_variant(
                            enum_name.to_string(),
                            variant.to_string(),
                        ),
                        _ => return Err("invalid constant dictionary key".into()),
                    };
                    Ok((key, materialize(value)?))
                })
                .collect::<Result<_, String>>()?,
        ),
        CftStaticValue::Object { type_name, fields } => LoadedValueDraft::object(
            type_name.to_string(),
            fields
                .iter()
                .map(|(name, value)| Ok((name.to_string(), materialize(value)?)))
                .collect::<Result<Vec<_>, String>>()?,
        ),
        CftStaticValue::RecordReference { type_name, key } => {
            LoadedValueDraft::record_ref(format!("{type_name}::{key}"))
        }
    })
}
