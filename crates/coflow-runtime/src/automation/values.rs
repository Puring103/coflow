use coflow_language::cft::{CftSchema, CftValueType};
use crate::{CfdValue, LoadedValueDraft as D, LoadedDictKeyDraft as K, CfdDictKey, CfdEnumValue};
use super::{Failure, Result};

pub fn expression(schema: &CftSchema, expected: &CftValueType, source: &str) -> Result<CfdValue> {
    let value = crate::cfd_loader::parse_expression(schema,expected,source)?;
    convert(schema,expected,value)
}

// 这里只转换语法值；引用连接由最终候选模型执行，因此同批前向引用和环不受旧模型限制。
fn convert(schema: &CftSchema, expected: &CftValueType, value: D) -> Result<CfdValue> {
    if let CftValueType::Option(inner) = expected {
        return match value { D::OptionNone => Ok(CfdValue::OptionNone), D::OptionSome(v) => Ok(CfdValue::OptionSome(Box::new(convert(schema,inner,*v)?))),
            v => Ok(CfdValue::OptionSome(Box::new(convert(schema,inner,v)?))) };
    }
    Ok(match value {
        D::Bool(v) => CfdValue::Bool(v), D::Int(v) => CfdValue::Int(v), D::Float(v) => CfdValue::Float(v), D::String(v) => CfdValue::String(v),
        D::FormattedString(v) => CfdValue::FormattedString(crate::CfdFormattedString { source: v.source, rendered: String::new() }),
        D::RecordRef(key) => CfdValue::record_ref(key).map_err(|e| Failure::request(e.to_string()))?,
        D::TypedRecordRef {type_name,key} => {
            if let CftValueType::RecordRef(ty) = expected { if !schema.is_assignable(&type_name,ty) { return Err(Failure::request("explicit reference type is not assignable")); } }
            CfdValue::record_ref(key).map_err(|e| Failure::request(e.to_string()))?
        }
        D::EnumVariant {enum_name,variant} => {
            let meta = schema.resolve_enum(&enum_name).and_then(|e| e.variants.iter().find(|v| v.name.as_str() == variant)).ok_or_else(|| Failure::request("unknown enum variant"))?;
            CfdValue::Enum(CfdEnumValue::try_new(enum_name,Some(variant),meta.value).map_err(|e| Failure::request(e.to_string()))?)
        }
        D::EnumValue {enum_name,value} => CfdValue::Enum(CfdEnumValue::try_new(enum_name,None::<String>,value).map_err(|e| Failure::request(e.to_string()))?),
        D::Array(items) => {
            let CftValueType::Array(inner) = expected else { return Err(Failure::request("array type required")); };
            CfdValue::Array(items.into_iter().map(|v| convert(schema,inner,v)).collect::<Result<_>>()?)
        }
        D::Dict(items) => {
            let CftValueType::Dict(_,inner) = expected else { return Err(Failure::request("dictionary type required")); };
            CfdValue::Dict(items.into_iter().map(|(k,v)| Ok((dict_key(schema,k)?,convert(schema,inner,v)?))).collect::<Result<_>>()?)
        }
        D::Object {actual_type, fields} => {
            let CftValueType::Object(expected_type) = expected else { return Err(Failure::request("object type required")); };
            let actual_type = actual_type.unwrap_or_else(|| expected_type.to_string());
            if !schema.is_assignable(&actual_type,expected_type) { return Err(Failure::request("object type is not assignable")); }
            let fields = fields.into_iter().map(|(name,value)| {
                let ty = &schema.field(&actual_type,&name).ok_or_else(|| Failure::request(format!("unknown field `{actual_type}.{name}`")))?.value_type;
                Ok((name,convert(schema,ty,value)?))
            }).collect::<Result<_>>()?;
            CfdValue::Object(Box::new(crate::CfdObject::try_new(actual_type,fields).map_err(|e| Failure::request(e.to_string()))?))
        }
        D::OptionNone | D::OptionSome(_) => return Err(Failure::request("Option value requires an Option field")),
    })
}
fn dict_key(schema: &CftSchema, value: K) -> Result<CfdDictKey> {
    Ok(match value {
        K::Int(v) => CfdDictKey::Int(v), K::String(v) => CfdDictKey::String(v),
        K::EnumVariant {enum_name,variant} => {
            let value = schema.resolve_enum(&enum_name).and_then(|e| e.variants.iter().find(|v| v.name.as_str() == variant)).ok_or_else(|| Failure::request("unknown enum key"))?.value;
            CfdDictKey::Enum(CfdEnumValue::try_new(enum_name,Some(variant),value).map_err(|e| Failure::request(e.to_string()))?)
        }
        K::EnumValue {enum_name,value} => CfdDictKey::Enum(CfdEnumValue::try_new(enum_name,None::<String>,value).map_err(|e| Failure::request(e.to_string()))?),
    })
}
