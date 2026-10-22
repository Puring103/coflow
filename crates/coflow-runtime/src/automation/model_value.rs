use std::collections::BTreeMap;

use coflow_language::cft::{CftSchema, CftValueType};
use serde::{Deserialize, Serialize};

use super::{Failure, Result};
use crate::{CfdDictKey, CfdValue};

/// CLI 的可写值协议独立于模型存储格式，渲染结果和来源信息不进入写入值。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModelValue {
    Bool {
        value: bool,
    },
    Int {
        #[schemars(regex(pattern = "^-?[0-9]+$"))]
        value: String,
    },
    Float {
        value: f64,
    },
    String {
        value: String,
    },
    None {},
    Some {
        value: Box<ModelValue>,
    },
    Enum {
        #[serde(rename = "type")]
        type_name: String,
        #[serde(default)]
        #[schemars(regex(pattern = "^-?[0-9]+$"))]
        value: Option<String>,
        variant: Option<String>,
    },
    Ref {
        #[serde(rename = "type")]
        type_name: String,
        id: String,
    },
    Object {
        #[serde(rename = "type")]
        type_name: String,
        fields: BTreeMap<String, ModelValue>,
    },
    Array {
        items: Vec<ModelValue>,
    },
    Dict {
        entries: Vec<DictionaryEntry>,
    },
    Fstring {
        source: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DictionaryEntry {
    pub key: ModelValue,
    pub value: ModelValue,
}

impl ModelValue {
    pub(crate) fn resolve(&self, schema: &CftSchema, expected: &CftValueType) -> Result<CfdValue> {
        use CftValueType as T;
        // 类型身份与 Option 层级在转换前检查，不能靠源码打印隐式抹掉错误的身份。
        Ok(match (self, expected) {
            (Self::Bool { value }, T::Bool) => CfdValue::Bool(*value),
            (Self::Int { value }, T::Int) => CfdValue::Int(parse_integer(value)?),
            (Self::Float { value }, T::Float) if value.is_finite() => CfdValue::Float(*value),
            (Self::String { value }, T::String) => CfdValue::String(value.clone()),
            (Self::None {}, T::Option(_)) => CfdValue::OptionNone,
            (Self::Some { value }, T::Option(inner)) => {
                CfdValue::OptionSome(Box::new(value.resolve(schema, inner)?))
            }
            (
                Self::Enum {
                    type_name,
                    value,
                    variant,
                },
                T::Enum(name),
            ) if type_name == name.as_str() => {
                let enumeration = schema
                    .resolve_enum(name)
                    .ok_or_else(|| Failure::request("unknown enum type"))?;
                let member = variant
                    .as_ref()
                    .map(|variant| {
                        enumeration
                            .variants
                            .iter()
                            .find(|member| member.name.as_str() == variant)
                            .ok_or_else(|| {
                                Failure::request(format!(
                                    "unknown enum member `{type_name}::{variant}`"
                                ))
                            })
                    })
                    .transpose()?;
                // 命名输入可由 Schema 解出数值；查询总是携带数值，组合值不伪造成员名称。
                let number = match (value, member) {
                    (Some(value), member) => {
                        let number = parse_integer(value)?;
                        if member.is_some_and(|member| member.value != number) {
                            return Err(Failure::request("enum member and numeric value disagree"));
                        }
                        number
                    }
                    (None, Some(member)) => member.value,
                    (None, None) => {
                        return Err(Failure::request(
                            "enum value requires a numeric value or member",
                        ))
                    }
                };
                CfdValue::Enum(
                    crate::CfdEnumValue::try_new(type_name.clone(), variant.clone(), number)
                        .map_err(|e| Failure::request(e.to_string()))?,
                )
            }
            (Self::Ref { type_name, id: key }, T::RecordRef(name))
                if schema.is_assignable(type_name, name) =>
            {
                CfdValue::record_ref(key.clone()).map_err(|e| Failure::request(e.to_string()))?
            }
            (Self::Object { type_name, fields }, T::Object(name))
                if schema.is_assignable(type_name, name) =>
            {
                let meta = schema
                    .resolve_type(type_name)
                    .ok_or_else(|| Failure::request("unknown object type"))?;
                if meta.is_abstract {
                    return Err(Failure::request("object type must be concrete"));
                }
                let fields = fields
                    .iter()
                    .map(|(name, value)| {
                        let field = schema.field(type_name, name).ok_or_else(|| {
                            Failure::request(format!("unknown field `{type_name}.{name}`"))
                        })?;
                        Ok((name.clone(), value.resolve(schema, &field.value_type)?))
                    })
                    .collect::<Result<_>>()?;
                CfdValue::Object(Box::new(
                    crate::CfdObject::try_new(type_name.clone(), fields)
                        .map_err(|e| Failure::request(e.to_string()))?,
                ))
            }
            (Self::Array { items }, T::Array(inner)) => CfdValue::Array(
                items
                    .iter()
                    .map(|v| v.resolve(schema, inner))
                    .collect::<Result<_>>()?,
            ),
            (Self::Dict { entries }, T::Dict(key_type, value_type)) => {
                let mut items = Vec::new();
                for entry in entries {
                    let key = dict_key(entry.key.resolve(schema, key_type)?)?;
                    if items.iter().any(|(existing, _)| existing == &key) {
                        return Err(Failure::request("duplicate dictionary key"));
                    }
                    items.push((key, entry.value.resolve(schema, value_type)?));
                }
                CfdValue::Dict(items)
            }
            (Self::Fstring { source }, T::String) => {
                super::values::expression(schema, expected, source)?
            }
            _ => {
                return Err(Failure::request(format!(
                    "model value does not match expected type `{expected}`"
                )))
            }
        })
    }

    pub(crate) fn from_model(
        schema: &CftSchema,
        expected: &CftValueType,
        value: &CfdValue,
    ) -> Result<Self> {
        Ok(match value {
            CfdValue::Bool(value) => Self::Bool { value: *value },
            CfdValue::Int(value) => Self::Int {
                value: value.to_string(),
            },
            CfdValue::Float(value) if value.is_finite() => Self::Float { value: *value },
            CfdValue::Float(_) => return Err(Failure::request("non-finite model float")),
            CfdValue::String(value) => Self::String {
                value: value.clone(),
            },
            CfdValue::OptionNone => Self::None {},
            CfdValue::OptionSome(value) => {
                let CftValueType::Option(inner) = expected else {
                    return Err(Failure::request("Option schema required"));
                };
                Self::Some {
                    value: Box::new(Self::from_model(schema, inner, value)?),
                }
            }
            CfdValue::Enum(value) => Self::Enum {
                type_name: value.enum_name.to_string(),
                value: Some(value.value.to_string()),
                variant: value.variant.as_ref().map(ToString::to_string),
            },
            CfdValue::Ref(key) => {
                let CftValueType::RecordRef(name) = expected else {
                    return Err(Failure::request("reference schema required"));
                };
                Self::Ref {
                    type_name: name.to_string(),
                    id: key.to_string(),
                }
            }
            CfdValue::Object(object) => Self::Object {
                type_name: object.actual_type.to_string(),
                fields: object
                    .fields
                    .iter()
                    .map(|(name, value)| {
                        let field = schema
                            .field(&object.actual_type, name)
                            .ok_or_else(|| Failure::request("object field schema required"))?;
                        Ok((
                            name.to_string(),
                            Self::from_model(schema, &field.value_type, value)?,
                        ))
                    })
                    .collect::<Result<_>>()?,
            },
            CfdValue::Array(items) => {
                let CftValueType::Array(inner) = expected else {
                    return Err(Failure::request("array schema required"));
                };
                Self::Array {
                    items: items
                        .iter()
                        .map(|v| Self::from_model(schema, inner, v))
                        .collect::<Result<_>>()?,
                }
            }
            CfdValue::Dict(items) => {
                let CftValueType::Dict(_, inner) = expected else {
                    return Err(Failure::request("dictionary schema required"));
                };
                Self::Dict {
                    entries: items
                        .iter()
                        .map(|(key, value)| {
                            Ok(DictionaryEntry {
                                key: Self::from_key(key)?,
                                value: Self::from_model(schema, inner, value)?,
                            })
                        })
                        .collect::<Result<_>>()?,
                }
            }
            CfdValue::FormattedString(value) => Self::Fstring {
                source: value.source.clone(),
            },
        })
    }

    pub(crate) fn from_key(key: &CfdDictKey) -> Result<Self> {
        Ok(match key {
            CfdDictKey::Int(value) => Self::Int {
                value: value.to_string(),
            },
            CfdDictKey::String(value) => Self::String {
                value: value.clone(),
            },
            CfdDictKey::Enum(value) => Self::Enum {
                type_name: value.enum_name.to_string(),
                value: Some(value.value.to_string()),
                variant: value.variant.as_ref().map(ToString::to_string),
            },
        })
    }
}

fn parse_integer(value: &str) -> Result<i64> {
    let digits = value.strip_prefix('-').unwrap_or(value);
    if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return Err(Failure::request("int value must be a decimal string"));
    }
    value
        .parse()
        .map_err(|_| Failure::request("int value exceeds signed 64-bit range"))
}

pub(crate) fn encode(
    schema: &CftSchema,
    expected: &CftValueType,
    value: Option<&CfdValue>,
) -> Result<serde_json::Value> {
    super::json(
        value
            .map(|v| ModelValue::from_model(schema, expected, v))
            .transpose()?,
    )
}

/// 渲染信息与可写值使用相同的树形位置，但只在包含 fstring 的分支输出。
pub(crate) fn rendered(value: &CfdValue) -> Option<serde_json::Value> {
    use serde_json::{json, Value};
    match value {
        CfdValue::FormattedString(value) => Some(json!(value.rendered)),
        CfdValue::OptionSome(value) => rendered(value),
        CfdValue::Object(value) => {
            let fields: serde_json::Map<_, _> = value
                .fields
                .iter()
                .filter_map(|(name, value)| rendered(value).map(|v| (name.to_string(), v)))
                .collect();
            (!fields.is_empty()).then_some(Value::Object(fields))
        }
        CfdValue::Array(items) => {
            let values: Vec<_> = items
                .iter()
                .map(|v| rendered(v).unwrap_or(Value::Null))
                .collect();
            values
                .iter()
                .any(|v| !v.is_null())
                .then_some(Value::Array(values))
        }
        CfdValue::Dict(items) => {
            let values: Vec<_> = items
                .iter()
                .map(|(_, v)| rendered(v).unwrap_or(Value::Null))
                .collect();
            values
                .iter()
                .any(|v| !v.is_null())
                .then_some(Value::Array(values))
        }
        _ => None,
    }
}

pub(crate) fn dict_key(value: CfdValue) -> Result<CfdDictKey> {
    Ok(match value {
        CfdValue::Int(v) => CfdDictKey::Int(v),
        CfdValue::String(v) => CfdDictKey::String(v),
        CfdValue::Enum(v) => CfdDictKey::Enum(v),
        _ => return Err(Failure::request("invalid dictionary key type")),
    })
}
