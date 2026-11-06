//! Declared schema defaults lowered to the shared model input representation.

use crate::{LoadedDictKeyDraft, LoadedValueDraft};
use coflow_language::cft::{CftSchema, CftSchemaDefaultValue, CftValueType};

/// Lowers an already compiled default, preserving explicit object fields and reference types.
///
/// Missing object fields are completed by model validation or declared-default completion.
///
/// # Errors
///
/// Returns an error when the default and compiled type do not match.
#[allow(clippy::too_many_lines)]
pub fn lower_schema_default(
    schema: &CftSchema,
    ty: &CftValueType,
    default: &CftSchemaDefaultValue,
) -> Result<LoadedValueDraft, String> {
    use CftSchemaDefaultValue as D;
    use CftValueType as T;
    use LoadedValueDraft as V;

    Ok(match (ty, default) {
        (T::Option(_), D::OptionNone) => V::OptionNone,
        (T::Option(inner), D::OptionSome(value)) => {
            V::OptionSome(Box::new(lower_schema_default(schema, inner, value)?))
        }
        (T::Int | T::Long, D::Int(value)) => V::Int(*value),
        (T::Float | T::Double, D::Float(value)) => V::Float(*value),
        (T::Bool, D::Bool(value)) => V::Bool(*value),
        (T::String, D::String(value)) => V::String(value.clone()),
        (T::String, D::FormattedString(value)) => V::FormattedString(value.as_ref().into()),
        (
            T::Enum(expected),
            D::Enum {
                enum_name,
                variant,
                value,
            },
        ) if expected == enum_name => {
            if schema
                .resolve_enum(enum_name)
                .is_some_and(|enm| enm.is_flag)
            {
                V::enum_value(enum_name.as_str(), *value)
            } else {
                V::enum_variant(enum_name.as_str(), variant.as_str())
            }
        }
        (T::Array(_), D::EmptyArray) => V::Array(Vec::new()),
        (T::Dict(_, _), D::EmptyObject) => V::Dict(Vec::new()),
        (T::Object(name), D::EmptyObject) => {
            V::object(name.as_str(), std::iter::empty::<(String, V)>())
        }
        (T::Array(inner), D::Array(values)) => V::Array(
            values
                .iter()
                .map(|value| lower_schema_default(schema, inner, value))
                .collect::<Result<_, _>>()?,
        ),
        (T::Dict(key_type, value_type), D::Dictionary(entries)) => V::Dict(
            entries
                .iter()
                .map(|(key, value)| {
                    Ok((
                        lower_default_key(schema, key_type, key)?,
                        lower_schema_default(schema, value_type, value)?,
                    ))
                })
                .collect::<Result<_, String>>()?,
        ),
        (T::Object(expected), D::Object { type_name, fields })
            if schema.is_assignable(type_name, expected) =>
        {
            // 先保留显式字段，缺省字段在消费端补齐；被 None 等值覆盖的递归默认值不展开。
            let fields = fields
                .iter()
                .map(|(name, value)| {
                    let field = schema
                        .field(type_name, name)
                        .ok_or_else(|| format!("unknown default field `{type_name}.{name}`"))?;
                    Ok((
                        name.to_string(),
                        lower_schema_default(schema, &field.value_type, value)?,
                    ))
                })
                .collect::<Result<Vec<_>, String>>()?;
            V::object(type_name.as_str(), fields)
        }
        (T::RecordRef(expected), D::RecordReference { type_name, id })
            if schema.is_assignable(type_name, expected) =>
        {
            V::TypedRecordRef {
                type_name: type_name.to_string(),
                id: id.clone(),
            }
        }
        _ => return Err(format!("schema default does not match field type `{ty}`")),
    })
}

fn lower_default_key(
    schema: &CftSchema,
    ty: &CftValueType,
    default: &CftSchemaDefaultValue,
) -> Result<LoadedDictKeyDraft, String> {
    // 旗标默认值可能带有编译期合成的 mask_N 名称；键和值统一保留数值掩码。
    match lower_schema_default(schema, ty, default)? {
        LoadedValueDraft::Int(value) => Ok(LoadedDictKeyDraft::Int(value)),
        LoadedValueDraft::String(value) => Ok(LoadedDictKeyDraft::String(value)),
        LoadedValueDraft::EnumVariant { enum_name, variant } => {
            Ok(LoadedDictKeyDraft::EnumVariant { enum_name, variant })
        }
        LoadedValueDraft::EnumValue { enum_name, value } => {
            Ok(LoadedDictKeyDraft::EnumValue { enum_name, value })
        }
        _ => {
            Err("schema default dictionary key must be an integer, string, or enum variant".into())
        }
    }
}
