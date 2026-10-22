use crate::{CfdDictKey, CfdEnumValue, CfdValue, LoadedDictKeyDraft as K, LoadedValueDraft as D};
use coflow_language::cft::{CftSchema, CftValueType};

// 这里只转换语法值；引用连接由最终候选模型执行，因此同批前向引用和环不受旧模型限制。
pub(crate) fn convert(
    schema: &CftSchema,
    expected: &CftValueType,
    value: D,
) -> Result<CfdValue, String> {
    if let CftValueType::Option(inner) = expected {
        return match value {
            D::OptionNone => Ok(CfdValue::OptionNone),
            D::OptionSome(v) => Ok(CfdValue::OptionSome(Box::new(convert(schema, inner, *v)?))),
            v => Ok(CfdValue::OptionSome(Box::new(convert(schema, inner, v)?))),
        };
    }
    Ok(match value {
        D::Bool(v) => CfdValue::Bool(v),
        D::Int(v) => CfdValue::Int(v),
        D::Float(v) => CfdValue::Float(v),
        D::String(v) => CfdValue::String(v),
        D::FormattedString(v) => CfdValue::FormattedString(crate::CfdFormattedString {
            source: v.source,
            rendered: String::new(),
        }),
        D::RecordRef(key) => CfdValue::record_ref(key).map_err(|e| e.to_string())?,
        D::TypedRecordRef { type_name, id: key } => {
            if let CftValueType::RecordRef(ty) = expected {
                if !schema.is_assignable(&type_name, ty) {
                    return Err("explicit reference type is not assignable".to_string());
                }
            }
            CfdValue::record_ref(key).map_err(|e| e.to_string())?
        }
        D::EnumVariant { enum_name, variant } => {
            let meta = schema
                .resolve_enum(&enum_name)
                .and_then(|e| e.variants.iter().find(|v| v.name.as_str() == variant))
                .ok_or_else(|| "unknown enum variant".to_string())?;
            CfdValue::Enum(
                CfdEnumValue::try_new(enum_name, Some(variant), meta.value)
                    .map_err(|e| e.to_string())?,
            )
        }
        D::EnumValue { enum_name, value } => CfdValue::Enum(
            CfdEnumValue::try_new(enum_name, None::<String>, value).map_err(|e| e.to_string())?,
        ),
        D::Array(items) => {
            let CftValueType::Array(inner) = expected else {
                return Err("array type required".to_string());
            };
            CfdValue::Array(
                items
                    .into_iter()
                    .map(|v| convert(schema, inner, v))
                    .collect::<Result<_, String>>()?,
            )
        }
        D::Dict(items) => {
            let CftValueType::Dict(_, inner) = expected else {
                return Err("dictionary type required".to_string());
            };
            CfdValue::Dict(
                items
                    .into_iter()
                    .map(|(k, v)| Ok((dict_key(schema, k)?, convert(schema, inner, v)?)))
                    .collect::<Result<_, String>>()?,
            )
        }
        D::Object {
            actual_type,
            fields,
        } => {
            let CftValueType::Object(expected_type) = expected else {
                return Err("object type required".to_string());
            };
            let actual_type = actual_type.unwrap_or_else(|| expected_type.to_string());
            if !schema.is_assignable(&actual_type, expected_type) {
                return Err("object type is not assignable".to_string());
            }
            let fields = fields
                .into_iter()
                .map(|(name, value)| {
                    let ty = &schema
                        .field(&actual_type, &name)
                        .ok_or_else(|| format!("unknown field `{actual_type}.{name}`"))?
                        .value_type;
                    Ok((name, convert(schema, ty, value)?))
                })
                .collect::<Result<_, String>>()?;
            CfdValue::Object(Box::new(
                crate::CfdObject::try_new(actual_type, fields).map_err(|e| e.to_string())?,
            ))
        }
        D::OptionNone | D::OptionSome(_) => {
            return Err("Option value requires an Option field".to_string())
        }
    })
}
pub(crate) fn dict_key(schema: &CftSchema, value: K) -> Result<CfdDictKey, String> {
    Ok(match value {
        K::Int(v) => CfdDictKey::Int(v),
        K::String(v) => CfdDictKey::String(v),
        K::EnumVariant { enum_name, variant } => {
            let value = schema
                .resolve_enum(&enum_name)
                .and_then(|e| e.variants.iter().find(|v| v.name.as_str() == variant))
                .ok_or_else(|| "unknown enum key".to_string())?
                .value;
            CfdDictKey::Enum(
                CfdEnumValue::try_new(enum_name, Some(variant), value)
                    .map_err(|e| e.to_string())?,
            )
        }
        K::EnumValue { enum_name, value } => CfdDictKey::Enum(
            CfdEnumValue::try_new(enum_name, None::<String>, value).map_err(|e| e.to_string())?,
        ),
    })
}
