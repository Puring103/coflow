use crate::schema::CftConstValue;
use crate::{LoadedDictKeyDraft, LoadedFormattedString, LoadedFunction, LoadedValueDraft};

/// 常量复用只复制静态存储值；模板与函数始终保留原始源码。
pub(super) fn materialize(value: &CftConstValue) -> Result<LoadedValueDraft, String> {
    Ok(match value {
        CftConstValue::Int(v) => LoadedValueDraft::Int(*v),
        CftConstValue::Float(v) => LoadedValueDraft::Float(*v),
        CftConstValue::Bool(v) => LoadedValueDraft::Bool(*v),
        CftConstValue::String(v) => LoadedValueDraft::String(v.clone()),
        CftConstValue::FormattedString(source) => {
            LoadedValueDraft::FormattedString(LoadedFormattedString {
                constant_origin: source.constant_origin.clone(),
                source: source.source.clone(),
            })
        }
        CftConstValue::Function(source) => LoadedValueDraft::Function(LoadedFunction {
            constant_origin: source.constant_origin.clone(),
            source: source.source.clone(),
        }),
        CftConstValue::Enum {
            enum_name, value, ..
        } => LoadedValueDraft::enum_value(enum_name.to_string(), *value),
        CftConstValue::OptionNone => LoadedValueDraft::OptionNone,
        CftConstValue::OptionSome(v) => LoadedValueDraft::OptionSome(Box::new(materialize(v)?)),
        CftConstValue::Array(values) => {
            LoadedValueDraft::Array(values.iter().map(materialize).collect::<Result<_, _>>()?)
        }
        CftConstValue::Dictionary(values) => LoadedValueDraft::Dict(
            values
                .iter()
                .map(|(key, value)| {
                    let key = match key {
                        CftConstValue::Int(v) => LoadedDictKeyDraft::Int(*v),
                        CftConstValue::Bool(v) => LoadedDictKeyDraft::Bool(*v),
                        CftConstValue::String(v) => LoadedDictKeyDraft::String(v.clone()),
                        CftConstValue::Enum {
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
        CftConstValue::Object { type_name, fields } => LoadedValueDraft::object(
            type_name.to_string(),
            fields
                .iter()
                .map(|(name, value)| Ok((name.to_string(), materialize(value)?)))
                .collect::<Result<Vec<_>, String>>()?,
        ),
        CftConstValue::RecordReference { type_name, key } => {
            LoadedValueDraft::record_ref(format!("{type_name}::{key}"))
        }
    })
}
