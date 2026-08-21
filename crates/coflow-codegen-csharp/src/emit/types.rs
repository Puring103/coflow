use coflow_language::CftValueType;

use crate::lowering::CsharpLoweringPlan;
use coflow_language::CftField;

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
