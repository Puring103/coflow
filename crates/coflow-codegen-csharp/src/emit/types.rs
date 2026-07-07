use coflow_cft::CftSchemaDefaultValue;

use crate::names::{escape_csharp_string, format_float};
use crate::schema_view::{FieldMeta, FieldType, SchemaView};
use crate::CsharpCodegenError;

use super::identifiers::csharp_public_member_name;

pub(super) fn csharp_type(ty: &FieldType, view: &SchemaView) -> String {
    match ty {
        FieldType::Int => {
            if view.int_32 {
                "int".to_string()
            } else {
                "long".to_string()
            }
        }
        FieldType::Float => {
            if view.float_32 {
                "float".to_string()
            } else {
                "double".to_string()
            }
        }
        FieldType::Bool => "bool".to_string(),
        FieldType::String => "string".to_string(),
        FieldType::Type(name) | FieldType::Ref(name) | FieldType::Enum(name) => {
            view.csharp_named_type(name)
        }
        FieldType::Array(inner) => format!("List<{}>", csharp_type(inner, view)),
        FieldType::Dict(key, value) => {
            format!(
                "Dictionary<{}, {}>",
                csharp_type(key, view),
                csharp_type(value, view)
            )
        }
        FieldType::Nullable(inner) => format!("{}?", csharp_type(inner, view)),
    }
}

/// Property type for a field, with `Localized<T>` wrapping when the field is
/// `@localized`. The wrapping is applied around the same type the field would
/// normally receive (including `IReadOnlyList<T>` / `IReadOnlyDictionary<...>`
/// for collection fields).
pub(super) fn csharp_field_property_type(field: &FieldMeta, view: &SchemaView) -> String {
    let inner = csharp_property_type(&field.ty, view);
    if field.is_dimensional {
        format!("Localized<{inner}>")
    } else {
        inner
    }
}

pub(super) fn csharp_property_type(ty: &FieldType, view: &SchemaView) -> String {
    match ty {
        FieldType::Array(inner) => format!("IReadOnlyList<{}>", csharp_type(inner, view)),
        FieldType::Dict(key, value) => {
            format!(
                "IReadOnlyDictionary<{}, {}>",
                csharp_type(key, view),
                csharp_type(value, view)
            )
        }
        FieldType::Nullable(inner) => format!("{}?", csharp_property_type(inner, view)),
        other => csharp_type(other, view),
    }
}

pub(super) fn default_value_expr(
    default: Option<&CftSchemaDefaultValue>,
    ty: &FieldType,
    view: &SchemaView,
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

fn string_default_expr(value: &str, ty: &FieldType, view: &SchemaView) -> String {
    match ty.non_nullable() {
        FieldType::Enum(name) if view.is_id_as_enum(name) => {
            let enum_name = view.csharp_enum_name(name);
            let value = escape_csharp_string(value);
            format!("({enum_name})Enum.Parse(typeof({enum_name}), \"{value}\")")
        }
        _ => format!("\"{}\"", escape_csharp_string(value)),
    }
}

pub(super) fn collection_default_expr(
    ty: &FieldType,
    view: &SchemaView,
) -> Result<String, CsharpCodegenError> {
    match ty {
        FieldType::Array(inner) => Ok(format!("new List<{}>()", csharp_type(inner, view))),
        FieldType::Dict(key, value) => Ok(format!(
            "new Dictionary<{}, {}>()",
            csharp_type(key, view),
            csharp_type(value, view)
        )),
        _ => Err(CsharpCodegenError::new(
            "collection default requires array or dict type",
        )),
    }
}
