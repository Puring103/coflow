use coflow_language::CftValueType;

use crate::lowering::CsharpLoweringPlan;
use crate::names::csharp_type_name;
use coflow_language::CftField;

pub(crate) fn csharp_type(ty: &CftValueType, view: &CsharpLoweringPlan<'_>) -> String {
    match ty {
        CftValueType::Int => "long".to_string(),
        CftValueType::Float => "double".to_string(),
        CftValueType::Bool => "bool".to_string(),
        CftValueType::String => "string".to_string(),
        CftValueType::Object(name) | CftValueType::RecordRef(name) => view.csharp_type_ref(name),
        CftValueType::Enum(name) => view.csharp_enum_ref(name),
        CftValueType::Array(inner) => {
            format!("IReadOnlyList<{}>", csharp_type(inner, view))
        }
        CftValueType::Dict(key, value) => {
            format!(
                "IReadOnlyDictionary<{}, {}>",
                csharp_type(key, view),
                csharp_type(value, view)
            )
        }
        CftValueType::Option(inner) => format!("Option<{}>", csharp_type(inner, view)),
        CftValueType::Result(value, error) => format!(
            "Result<{}, {}>",
            csharp_type(value, view),
            csharp_type(error, view)
        ),
        CftValueType::Function(parameters, result) => {
            let mut arguments = parameters
                .iter()
                .map(|parameter| csharp_type(&parameter.value_type, view))
                .collect::<Vec<_>>();
            if matches!(result.as_ref(), CftValueType::Unit) {
                if arguments.is_empty() {
                    "Action".to_string()
                } else {
                    format!("Action<{}>", arguments.join(", "))
                }
            } else {
                arguments.push(csharp_type(result, view));
                format!("Func<{}>", arguments.join(", "))
            }
        }
        CftValueType::Unit => "Unit".to_string(),
    }
}

/// Property type for a dimensional field, with its generated dimension type
/// wrapping the normal field type, including collection types.
pub(super) fn csharp_field_property_type(
    field: &CftField,
    view: &CsharpLoweringPlan<'_>,
) -> String {
    let inner = csharp_property_type(&field.value_type, view);
    if let Some(binding) = &field.dimension {
        format!("{}<{inner}>", csharp_type_name(binding.dimension.as_str()))
    } else {
        inner
    }
}

pub(super) fn csharp_property_type(ty: &CftValueType, view: &CsharpLoweringPlan<'_>) -> String {
    match ty {
        CftValueType::Array(_) | CftValueType::Dict(_, _) => csharp_type(ty, view),
        CftValueType::Option(inner) => format!("Option<{}>", csharp_property_type(inner, view)),
        CftValueType::Result(value, error) => format!(
            "Result<{}, {}>",
            csharp_property_type(value, view),
            csharp_property_type(error, view)
        ),
        other => csharp_type(other, view),
    }
}
