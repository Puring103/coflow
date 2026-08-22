mod database;
mod identifiers;
mod types;

use crate::lowering::{CsharpDimensionTable, CsharpLoweringPlan};
use crate::model::{
    CsharpConstructorAssignment, CsharpEnum, CsharpEnumVariant, CsharpEquality, CsharpLoaderField,
    CsharpParameter, CsharpProperty, CsharpType,
};
use coflow_language::CftSchemaDefaultValue;
use crate::CsharpCodegenError;
use coflow_language::{CftEnum, CftField, CftType, CftValueType};
use std::collections::{BTreeSet, HashSet};

pub use database::build_csharp_database;
use identifiers::{csharp_public_member_name, csharp_public_type_name, field_local_name};
use types::{csharp_field_property_type, csharp_type};

pub fn build_csharp_enum(schema_enum: &CftEnum) -> CsharpEnum {
    CsharpEnum {
        name: csharp_public_type_name(&schema_enum.name),
        source_name: schema_enum.name.to_string(),
        is_flags: schema_enum.is_flag,
        summary: csharp_summary(schema_enum.display.as_ref()),
        obsolete: false,
        variants: schema_enum
            .variants
            .iter()
            .map(|variant| CsharpEnumVariant {
                name: csharp_public_member_name(&variant.name),
                source_name: variant.name.to_string(),
                value: variant.value,
                summary: csharp_summary(variant.display.as_ref()),
                obsolete: false,
            })
            .collect(),
    }
}

pub fn build_csharp_type(
    schema_type: &CftType,
    view: &CsharpLoweringPlan<'_>,
) -> Result<CsharpType, CsharpCodegenError> {
    let ty = view.resolve_type(&schema_type.name)?;
    let mut constructor_parameters = Vec::new();
    let mut base_constructor_args = Vec::new();
    let mut assignments = Vec::new();
    let mut properties = Vec::new();

    let is_struct = schema_type.is_struct;
    let is_table = !schema_type.is_abstract && type_is_table(&schema_type.name, view);
    if is_table {
        add_id_constructor_member(
            schema_type,
            view,
            &mut constructor_parameters,
            &mut base_constructor_args,
            &mut properties,
            &mut assignments,
        );
    }

    let all_fields = view.fields(&ty.name)?.collect::<Vec<_>>();
    let own_field_names = schema_type
        .own_fields()
        .map(|field| field.name.clone())
        .collect::<BTreeSet<_>>();

    for field in &all_fields {
        let local_name = field_local_name(&field.name, &mut HashSet::new())?;
        let property_type = csharp_field_property_type(field, view);
        constructor_parameters.push(CsharpParameter {
            ty: property_type.clone(),
            name: local_name.clone(),
        });
        if !is_struct && schema_type.parent.is_some() && !own_field_names.contains(&field.name) {
            base_constructor_args.push(local_name);
            continue;
        }

        add_field_constructor_member(
            field,
            property_type,
            local_name,
            view,
            &mut properties,
            &mut assignments,
        );
    }

    let all_field_props = all_fields
        .iter()
        .map(|f| csharp_public_member_name(&f.name))
        .collect::<Vec<_>>();
    let equality = (!schema_type.is_abstract).then_some({
        CsharpEquality {
            key_property: "Id".to_string(),
            is_struct,
            by_fields: !is_table,
            fields: all_field_props,
        }
    });

    let loader_fields = all_fields
        .iter()
        .map(|field| {
            let reader = loader_reader(&field.value_type, view, "VALUE", "CONTEXT");
            let default = field
                .default
                .as_ref()
                .map(|value| loader_default(value, &field.value_type, view))
                .transpose()?;
            let dimension = field
                .dimension
                .as_ref()
                .map(|binding| {
                    let generated_type = format!(
                        "{}_{}Variants",
                        field.declaring_type, field.name
                    );
                    let variants = view.dimension_variants(binding.dimension.as_str())?;
                    Ok((generated_type, variants))
                })
                .transpose()?;
            let dimension_key = view
                .type_is_singleton(field.declaring_type.as_str())?
                .then(|| format!("\"{}\"", escape_csharp_literal(field.name.as_str())));
            let localized_reader = |base: String, context: &str, key: &str| {
                dimension.as_ref().map_or(base.clone(), |(generated_type, variants)| {
                    let variants = variants
                        .iter()
                        .map(|variant| format!("\"{}\"", escape_csharp_literal(variant)))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!(
                        "ReadLocalized({base}, {context}, $\"{}.{}.{{{key}}}\", \"{}\", {}, new string[] {{ {variants} }}, static (item, context) => {})",
                        field.declaring_type,
                        field.name,
                        escape_csharp_literal(generated_type),
                        dimension_key.as_deref().unwrap_or(key),
                        loader_reader(&field.value_type, view, "item", "context")
                    )
                })
            };
            Ok(CsharpLoaderField {
                source_name: field.name.to_string(),
                property_name: csharp_public_member_name(&field.name),
                reader_expression: localized_reader(reader, "CONTEXT", "RECORD_KEY"),
                default_expression: default
                    .map(|default| localized_reader(default, "context", "key")),
                object_type: match field.value_type.non_nullable() {
                    CftValueType::Object(name) => Some(name.to_string()),
                    _ => None,
                },
                reference_type: match field.value_type.non_nullable() {
                    CftValueType::RecordRef(name) => Some(name.to_string()),
                    _ => None,
                },
            })
        })
        .collect::<Result<Vec<_>, CsharpCodegenError>>()?;
    let loader_variants = if schema_type.is_abstract || view.type_has_descendants(&schema_type.name) {
        view.concrete_assignable_types(&schema_type.name)?
            .iter()
            .filter(|source_name| source_name.as_str() != schema_type.name.as_str())
            .map(|source_name| crate::model::CsharpLoaderVariant {
                source_name: source_name.clone(),
                type_name: view.csharp_type_name(source_name),
            })
            .collect()
    } else {
        Vec::new()
    };
    Ok(CsharpType {
        name: view.csharp_type_name(&schema_type.name),
        source_name: schema_type.name.to_string(),
        declaration: type_declaration(schema_type, view),
        constructor_visibility: if schema_type.is_abstract {
            "protected".to_string()
        } else {
            "public".to_string()
        },
        summary: csharp_summary(schema_type.display.as_ref()),
        obsolete: false,
        properties,
        constructor_parameters,
        base_constructor_call: (!base_constructor_args.is_empty())
            .then(|| format!(" : base({})", base_constructor_args.join(", "))),
        base_constructor_args,
        assignments,
        equality,
        loader_fields,
        loader_id_type: (!schema_type.is_singleton && !schema_type.is_abstract)
            .then(|| csharp_type(&view.key_field_type(&schema_type.name), view)),
        loader_enabled: !schema_type.is_abstract,
        loader_assignable_to: if schema_type.is_abstract {
            Vec::new()
        } else {
            view.assignable_target_names(&schema_type.name)?
        },
        loader_variants,
    })
}

pub fn build_csharp_dimension_type(
    table: &CsharpDimensionTable,
    view: &CsharpLoweringPlan<'_>,
) -> Result<CsharpType, CsharpCodegenError> {
    let name = view.csharp_type_name(&table.source_name);
    let mut constructor_parameters = vec![CsharpParameter {
        ty: "string".to_string(),
        name: "id".to_string(),
    }];
    let mut properties = vec![CsharpProperty {
        visibility: "public".to_string(),
        name: "Id".to_string(),
        type_name: "string".to_string(),
        backing_field: None,
        summary: None,
        obsolete: false,
    }];
    let mut assignments = vec![CsharpConstructorAssignment {
        property: "Id".to_string(),
        target: "Id".to_string(),
        parameter: "id".to_string(),
    }];
    let mut used_names = HashSet::new();
    for field in &table.fields {
        let local_name = field_local_name(&field.name, &mut used_names)?;
        let property_type = csharp_field_property_type(field, view);
        constructor_parameters.push(CsharpParameter {
            ty: property_type.clone(),
            name: local_name.clone(),
        });
        add_field_constructor_member(
            field,
            property_type,
            local_name,
            view,
            &mut properties,
            &mut assignments,
        );
    }

    let loader_fields = table
        .fields
        .iter()
        .map(|field| CsharpLoaderField {
            source_name: field.name.to_string(),
            property_name: csharp_public_member_name(&field.name),
            reader_expression: loader_reader(&field.value_type, view, "VALUE", "CONTEXT"),
            default_expression: None,
            object_type: match field.value_type.non_nullable() {
                CftValueType::Object(name) => Some(name.to_string()),
                _ => None,
            },
            reference_type: match field.value_type.non_nullable() {
                CftValueType::RecordRef(name) => Some(name.to_string()),
                _ => None,
            },
        })
        .collect();
    Ok(CsharpType {
        name: name.clone(),
        source_name: table.source_name.clone(),
        declaration: format!("public sealed partial class {name} : IEquatable<{name}>"),
        constructor_visibility: "public".to_string(),
        summary: None,
        obsolete: false,
        properties,
        constructor_parameters,
        base_constructor_args: Vec::new(),
        base_constructor_call: None,
        assignments,
        equality: Some(CsharpEquality {
            key_property: "Id".to_string(),
            is_struct: false,
            by_fields: false,
            fields: Vec::new(),
        }),
        loader_fields,
        loader_id_type: Some("string".to_string()),
        loader_enabled: true,
        loader_assignable_to: vec![table.source_name.clone()],
        loader_variants: Vec::new(),
    })
}

fn loader_reader(
    ty: &CftValueType,
    view: &CsharpLoweringPlan<'_>,
    node: &str,
    context: &str,
) -> String {
    match ty {
        CftValueType::Int => format!("CfdValueReader.{}({node})", if view.int_32 { "Int32" } else { "Int64" }),
        CftValueType::Float => format!("CfdValueReader.{}({node})", if view.float_32 { "Float32" } else { "Float64" }),
        CftValueType::Bool => format!("CfdValueReader.Boolean({node})"),
        CftValueType::String => format!("CfdValueReader.String({node}, {context})"),
        CftValueType::Enum(name) => format!("ReadEnum{}({node})", view.csharp_enum_name(name)),
        CftValueType::Object(name) => format!("Read{}({node}, {context})", view.csharp_type_name(name)),
        CftValueType::RecordRef(name) => format!(
            "CfdValueReader.Reference<{}>({node}, {context}, \"{}\")",
            view.csharp_type_name(name), name
        ),
        CftValueType::Array(inner) => format!(
            "CfdValueReader.Array({node}, {context}, static (item, context) => {})",
            loader_reader(inner, view, "item", "context")
        ),
        CftValueType::Dict(key, value) => format!(
            "CfdValueReader.Dictionary({node}, {context}, static (item, context) => {}, static (item, context) => {})",
            loader_reader(key, view, "item", "context"),
            loader_reader(value, view, "item", "context")
        ),
        CftValueType::Nullable(inner) => {
            let target = csharp_type(ty, view);
            format!("({target})({node} is CfdNullValue ? default : {inner})", inner = loader_reader(inner, view, node, context))
        }
    }
}

fn loader_default(
    value: &CftSchemaDefaultValue,
    ty: &CftValueType,
    view: &CsharpLoweringPlan<'_>,
) -> Result<String, CsharpCodegenError> {
    let non_nullable = ty.non_nullable();
    Ok(match value {
        CftSchemaDefaultValue::Null => "default".to_string(),
        CftSchemaDefaultValue::Int(value) => {
            if view.int_32 {
                let value = i32::try_from(*value).map_err(|_| {
                    CsharpCodegenError::new(format!(
                        "integer default `{value}` does not fit the configured C# int type"
                    ))
                })?;
                value.to_string()
            } else if *value == i64::MIN {
                "(-9223372036854775807L - 1L)".to_string()
            } else {
                format!("{value}L")
            }
        }
        CftSchemaDefaultValue::Float(value) => {
            if view.float_32 {
                let value = *value as f32;
                if !value.is_finite() {
                    return Err(CsharpCodegenError::new(
                        "float default does not fit the configured C# float type",
                    ));
                }
                format!("{}F", float_literal(f64::from(value)))
            } else {
                format!("{}D", float_literal(*value))
            }
        }
        CftSchemaDefaultValue::Bool(value) => value.to_string(),
        CftSchemaDefaultValue::String(value) => {
            format!("\"{}\"", escape_csharp_literal(value))
        }
        CftSchemaDefaultValue::Enum {
            enum_name, variant, ..
        } => format!(
            "{}.{}",
            view.csharp_enum_name(enum_name),
            csharp_public_member_name(variant)
        ),
        CftSchemaDefaultValue::EmptyArray => {
            let CftValueType::Array(item) = non_nullable else {
                return Err(CsharpCodegenError::new("empty array default used on a non-array field"));
            };
            format!("Array.Empty<{}>()", csharp_type(item, view))
        }
        CftSchemaDefaultValue::EmptyObject => match non_nullable {
            CftValueType::Dict(key, value) => format!(
                "new Dictionary<{}, {}>()",
                csharp_type(key, view),
                csharp_type(value, view)
            ),
            CftValueType::Object(name) => format!(
                "Read{}Fields(Array.Empty<CfdFieldNode>(), string.Empty, context)",
                if view.resolve_type(name)?.is_abstract {
                    return Err(CsharpCodegenError::new(format!(
                        "empty object default cannot instantiate abstract CFT type `{name}`"
                    )));
                } else {
                    view.csharp_type_name(name)
                }
            ),
            _ => {
                return Err(CsharpCodegenError::new(
                    "empty object default used on a non-object field",
                ));
            }
        },
    })
}

fn float_literal(value: f64) -> String {
    let literal = format!("{value:?}");
    if literal.contains(['.', 'e', 'E']) {
        literal
    } else {
        format!("{literal}.0")
    }
}

fn escape_csharp_literal(value: &str) -> String {
    value
        .chars()
        .flat_map(|character| match character {
            '\\' => "\\\\".chars().collect::<Vec<_>>(),
            '"' => "\\\"".chars().collect(),
            '\n' => "\\n".chars().collect(),
            '\r' => "\\r".chars().collect(),
            '\t' => "\\t".chars().collect(),
            character => vec![character],
        })
        .collect()
}

fn add_id_constructor_member(
    schema_type: &CftType,
    view: &CsharpLoweringPlan<'_>,
    constructor_parameters: &mut Vec<CsharpParameter>,
    base_constructor_args: &mut Vec<String>,
    properties: &mut Vec<CsharpProperty>,
    assignments: &mut Vec<CsharpConstructorAssignment>,
) {
    let key_ty = view.key_field_type(&schema_type.name);
    constructor_parameters.push(CsharpParameter {
        ty: csharp_type(&key_ty, view),
        name: "id".to_string(),
    });
    if has_concrete_parent(&schema_type.name, view) {
        base_constructor_args.push("id".to_string());
        return;
    }
    properties.push(CsharpProperty {
        visibility: "public".to_string(),
        name: "Id".to_string(),
        type_name: csharp_type(&key_ty, view),
        backing_field: None,
        summary: None,
        obsolete: false,
    });
    assignments.push(CsharpConstructorAssignment {
        property: "Id".to_string(),
        target: "Id".to_string(),
        parameter: "id".to_string(),
    });
}

fn add_field_constructor_member(
    field: &CftField,
    property_type: String,
    local_name: String,
    view: &CsharpLoweringPlan<'_>,
    properties: &mut Vec<CsharpProperty>,
    assignments: &mut Vec<CsharpConstructorAssignment>,
) {
    let property_name = csharp_public_member_name(&field.name);
    let backing_field = backing_field_name(&property_name, &field.value_type, view);
    properties.push(CsharpProperty {
        visibility: "public".to_string(),
        name: property_name.clone(),
        type_name: property_type,
        backing_field: backing_field.clone(),
        summary: csharp_summary(field.display.as_ref()),
        obsolete: false,
    });
    assignments.push(CsharpConstructorAssignment {
        target: backing_field.unwrap_or_else(|| property_name.clone()),
        property: property_name,
        parameter: local_name,
    });
}

fn csharp_summary(display: Option<&coflow_language::CftDisplayMetadata>) -> Option<String> {
    display
        .and_then(|display| match (&display.label, &display.description) {
            (Some(label), Some(description)) => Some(format!("{label}: {description}")),
            (Some(label), None) => Some(label.clone()),
            (None, Some(description)) => Some(description.clone()),
            (None, None) => None,
        })
        .map(|text| {
            text.replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
        })
}

fn type_is_table(type_name: &str, view: &CsharpLoweringPlan<'_>) -> bool {
    view.is_ref_target_loadable(type_name)
}

fn has_concrete_parent(type_name: &str, view: &CsharpLoweringPlan<'_>) -> bool {
    let mut parent = view
        .resolve_type(type_name)
        .ok()
        .and_then(|ty| ty.parent.as_deref());
    while let Some(parent_name) = parent {
        let Ok(parent_ty) = view.resolve_type(parent_name) else {
            return false;
        };
        if !parent_ty.is_abstract {
            return true;
        }
        parent = parent_ty.parent.as_deref();
    }
    false
}

pub(super) fn backing_field_name(
    property_name: &str,
    ty: &CftValueType,
    view: &CsharpLoweringPlan<'_>,
) -> Option<String> {
    let _ = (property_name, ty, view);
    None
}

fn type_declaration(schema_type: &CftType, view: &CsharpLoweringPlan<'_>) -> String {
    let prefix = if schema_type.is_abstract {
        "public abstract partial class"
    } else if schema_type.is_struct {
        "public partial struct"
    } else if schema_type.is_sealed || !view.type_has_descendants(&schema_type.name) {
        "public sealed partial class"
    } else {
        "public partial class"
    };

    let mut interfaces = Vec::new();
    if let Some(parent) = schema_type
        .parent
        .as_ref()
        .filter(|_| !schema_type.is_struct)
    {
        interfaces.push(view.csharp_type_name(parent));
    }
    if !schema_type.is_abstract {
        interfaces.push(format!(
            "IEquatable<{}>",
            view.csharp_type_name(&schema_type.name)
        ));
    }
    let suffix = if interfaces.is_empty() {
        String::new()
    } else {
        format!(" : {}", interfaces.join(", "))
    };

    format!(
        "{prefix} {}{suffix}",
        view.csharp_type_name(&schema_type.name)
    )
}
