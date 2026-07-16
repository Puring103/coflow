use coflow_cft::{CftSchemaDefaultValue, CftValueType};

use crate::lowering::CsharpLoweringPlan;
use crate::names::{escape_csharp_string, format_float};
use crate::CsharpCodegenError;
use coflow_cft::CftField;

use super::identifiers::csharp_public_member_name;

pub(super) fn csharp_type(ty: &CftValueType, view: &CsharpLoweringPlan<'_>) -> String {
    match ty {
        CftValueType::Int => {
            if view.int_32 {
                "int".to_string()
            } else {
                "long".to_string()
            }
        }
        CftValueType::Float => {
            if view.float_32 {
                "float".to_string()
            } else {
                "double".to_string()
            }
        }
        CftValueType::Bool => "bool".to_string(),
        CftValueType::String => "string".to_string(),
        CftValueType::Object(name) | CftValueType::RecordRef(name) => view.csharp_type_name(name),
        CftValueType::Enum(name) => view.csharp_enum_name(name),
        CftValueType::Array(inner) => format!("List<{}>", csharp_type(inner, view)),
        CftValueType::Dict(key, value) => {
            format!(
                "Dictionary<{}, {}>",
                csharp_type(key, view),
                csharp_type(value, view)
            )
        }
        CftValueType::Nullable(inner) => format!("{}?", csharp_type(inner, view)),
    }
}

/// Property type for a field, with `Localized<T>` wrapping when the field is
/// `@localized`. The wrapping is applied around the same type the field would
/// normally receive (including `IReadOnlyList<T>` / `IReadOnlyDictionary<...>`
/// for collection fields).
pub(super) fn csharp_field_property_type(
    field: &CftField,
    view: &CsharpLoweringPlan<'_>,
) -> String {
    let inner = csharp_property_type(&field.value_type, view);
    if field.dimension.is_some() {
        format!("Localized<{inner}>")
    } else {
        inner
    }
}

pub(super) fn csharp_property_type(ty: &CftValueType, view: &CsharpLoweringPlan<'_>) -> String {
    match ty {
        CftValueType::Array(inner) => format!("IReadOnlyList<{}>", csharp_type(inner, view)),
        CftValueType::Dict(key, value) => {
            format!(
                "IReadOnlyDictionary<{}, {}>",
                csharp_type(key, view),
                csharp_type(value, view)
            )
        }
        CftValueType::Nullable(inner) => format!("{}?", csharp_property_type(inner, view)),
        other => csharp_type(other, view),
    }
}

pub(super) fn default_value_expr(
    default: Option<&CftSchemaDefaultValue>,
    ty: &CftValueType,
    view: &CsharpLoweringPlan<'_>,
) -> Result<Option<String>, CsharpCodegenError> {
    let Some(default) = default else {
        return Ok(None);
    };
    Ok(Some(match default {
        CftSchemaDefaultValue::Null => "null".to_string(),
        CftSchemaDefaultValue::Int(value) => {
            if view.int_32 {
                value.to_string()
            } else {
                format!("{value}L")
            }
        }
        CftSchemaDefaultValue::Float(value) => {
            let mut text = format_float(*value);
            if view.float_32 {
                text.push('f');
            }
            text
        }
        CftSchemaDefaultValue::Bool(value) => value.to_string(),
        CftSchemaDefaultValue::String(value) => string_default_expr(value, ty, view),
        CftSchemaDefaultValue::Enum {
            enum_name, variant, ..
        } => format!(
            "{}.{}",
            view.csharp_enum_name(enum_name),
            csharp_public_member_name(variant)
        ),
        CftSchemaDefaultValue::EmptyArray | CftSchemaDefaultValue::EmptyObject => {
            collection_default_expr(ty.non_nullable(), view)?
        }
    }))
}

fn string_default_expr(value: &str, ty: &CftValueType, view: &CsharpLoweringPlan<'_>) -> String {
    match ty.non_nullable() {
        CftValueType::Enum(name) if view.is_id_as_enum(name) => {
            let enum_name = view.csharp_enum_name(name);
            let value = escape_csharp_string(value);
            format!("({enum_name})Enum.Parse(typeof({enum_name}), \"{value}\")")
        }
        _ => format!("\"{}\"", escape_csharp_string(value)),
    }
}

pub(super) fn collection_default_expr(
    ty: &CftValueType,
    view: &CsharpLoweringPlan<'_>,
) -> Result<String, CsharpCodegenError> {
    match ty {
        CftValueType::Array(inner) => Ok(format!("new List<{}>()", csharp_type(inner, view))),
        CftValueType::Dict(key, value) => Ok(format!(
            "new Dictionary<{}, {}>()",
            csharp_type(key, view),
            csharp_type(value, view)
        )),
        _ => Err(CsharpCodegenError::new(
            "collection default requires array or dict type",
        )),
    }
}
