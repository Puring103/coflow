mod identifiers;
pub(crate) mod types;

use crate::lowering::{CsharpDimensionTable, CsharpLoweringPlan};
use crate::model::{
    CsharpAnnotation, CsharpAnnotationArgument, CsharpConstructorAssignment, CsharpEnum, CsharpEnumVariant, CsharpEquality, CsharpFunction,
    CsharpEqualityField, CsharpHostField, CsharpLoaderField, CsharpParameter, CsharpProperty, CsharpType,
};
use coflow_language::{CftAnnotation, CftAnnotationValue, CftSchemaDefaultValue};
use crate::CsharpCodegenError;
use coflow_language::{CftEnum, CftField, CftFunctionParameter, CftType, CftValueType};
use std::collections::{BTreeSet, HashSet};

use identifiers::{
    csharp_public_member_name, csharp_public_type_name, field_local_name,
    function_parameter_name,
};
use types::{csharp_field_property_type, csharp_type};

pub fn build_csharp_enum(schema_enum: &CftEnum, view: &CsharpLoweringPlan<'_>) -> CsharpEnum {
    CsharpEnum {
        name: csharp_public_type_name(&schema_enum.name),
        namespace: view.csharp_namespace(&schema_enum.name),
        qualified_name: view.csharp_enum_ref(&schema_enum.name),
        relative_path: view.csharp_relative_path(&schema_enum.name),
        metadata_name: view.metadata_name(&schema_enum.name),
        source_name: schema_enum.name.to_string(),
        annotations: csharp_annotations(&schema_enum.annotations),
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
                annotations: csharp_annotations(&variant.annotations),
                summary: csharp_summary(variant.display.as_ref()),
                obsolete: false,
            })
            .collect(),
    }
}

fn function_parameters(
    parameters: &[CftFunctionParameter],
    view: &CsharpLoweringPlan<'_>,
) -> Result<Vec<CsharpParameter>, CsharpCodegenError> {
    let mut used_names = HashSet::new();
    parameters
        .iter()
        .enumerate()
        .map(|(index, parameter)| {
            Ok(CsharpParameter {
                ty: csharp_type(&parameter.value_type, view),
                name: function_parameter_name(
                    parameter.name.as_deref(),
                    index,
                    &mut used_names,
                )?,
            })
        })
        .collect()
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
    let mut functions = Vec::new();
    let mut host_fields = Vec::new();

    let is_struct = schema_type.is_struct;
    if !is_struct {
        constructor_parameters.push(CsharpParameter {
            ty: "CoflowHostState?".to_string(),
            name: "hostSlot".to_string(),
        });
        if schema_type.parent.is_some() {
            base_constructor_args.push("hostSlot".to_string());
        } else {
            assignments.push(CsharpConstructorAssignment {
                property: "HostSlot".to_string(),
                target: "_coflowHost".to_string(),
                parameter: "hostSlot".to_string(),
            });
        }
    }
    let is_table = !schema_type.is_abstract && !is_struct && type_is_table(&schema_type.name, view);
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
        let inherited = !is_struct && schema_type.parent.is_some() && !own_field_names.contains(&field.name);
        if let CftValueType::Function(parameters, result) = &field.value_type {
            constructor_parameters.push(CsharpParameter {
                ty: format!("CoflowFunctionEntry<{}>", csharp_type(&field.value_type, view)),
                name: local_name.clone(),
            });
            let method_name = csharp_public_member_name(&field.name);
            let entry_name = format!("_coflow{}", method_name);
            functions.push(CsharpFunction {
                source_name: field.name.to_string(),
                method_name: method_name.clone(),
                bind_method_name: format!("Bind{method_name}"),
                bind_parameter_name: local_name.clone(),
                entry_name: entry_name.clone(),
                declared_here: !inherited,
                result_type: csharp_type(result, view),
                delegate_type: csharp_type(&field.value_type, view),
                parameters: function_parameters(parameters, view)?,
                returns_void: matches!(result.as_ref(), CftValueType::Unit),
                summary: csharp_summary(field.display.as_ref()),
            });
            if inherited {
                base_constructor_args.push(local_name);
                continue;
            }
            assignments.push(CsharpConstructorAssignment {
                target: entry_name,
                property: method_name,
                parameter: local_name,
            });
            continue;
        }

        let property_type = csharp_field_property_type(field, view);
        if schema_type.is_host {
            let property_name = csharp_public_member_name(&field.name);
            let backing_field = format!("_coflow{property_name}");
            if !inherited {
                properties.push(CsharpProperty {
                    visibility: "public".to_string(),
                    name: property_name.clone(),
                    type_name: property_type.clone(),
                    backing_field: Some(backing_field.clone()),
                    guard_host: true,
                    summary: csharp_summary(field.display.as_ref()),
                    obsolete: false,
                });
            }
            host_fields.push(CsharpHostField {
                target: backing_field,
                parameter: CsharpParameter {
                    ty: property_type,
                    name: local_name,
                },
            });
            if inherited {
                base_constructor_args.push("default!".to_string());
            }
            continue;
        }

        constructor_parameters.push(CsharpParameter {
            ty: property_type.clone(),
            name: local_name.clone(),
        });
        if inherited {
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
            is_struct,
        );
    }

    let all_field_props = all_fields
        .iter()
        .filter(|field| !matches!(field.value_type, CftValueType::Function(_, _)))
        .map(|field| CsharpEqualityField {
            name: csharp_public_member_name(&field.name),
            ty: csharp_field_property_type(field, view),
        })
        .collect::<Vec<_>>();
    let equality = (!schema_type.is_abstract).then_some({
        CsharpEquality {
            key_property: "Id".to_string(),
            is_struct: false,
            by_fields: !is_table,
            fields: all_field_props,
        }
    });

    let loader_fields = all_fields
        .iter()
        .map(|field| {
            let reader = function_loader_reader(field, view, "VALUE", "CONTEXT", !schema_type.is_host)
                .unwrap_or_else(|| loader_reader(&field.value_type, view, "VALUE", "CONTEXT"));
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
                value_type: csharp_type(&field.value_type, view),
                is_function: matches!(field.value_type, CftValueType::Function(_, _)),
                reader_expression: localized_reader(reader, "CONTEXT", "RECORD_KEY"),
                default_expression: default
                    .map(|default| localized_reader(default, "context", "key")),
                object_type: match &field.value_type {
                    CftValueType::Object(name) => Some(name.to_string()),
                    _ => None,
                },
                reference_type: match &field.value_type {
                    CftValueType::RecordRef(name) => Some(name.to_string()),
                    _ => None,
                },
                annotations: csharp_annotations(&field.annotations),
            })
        })
        .collect::<Result<Vec<_>, CsharpCodegenError>>()?;
    let loader_variants = if schema_type.is_abstract || view.type_has_descendants(&schema_type.name) {
        view.concrete_assignable_types(&schema_type.name)?
            .iter()
            .filter(|source_name| source_name.as_str() != schema_type.name.as_str())
            .filter(|source_name| {
                view.resolve_type(source_name)
                    .is_ok_and(|candidate| !candidate.is_host)
            })
            .map(|source_name| crate::model::CsharpLoaderVariant {
                source_name: source_name.clone(),
                type_name: view.metadata_name(source_name),
            })
            .collect()
    } else {
        Vec::new()
    };
    let loader_id_type = is_table.then(|| csharp_type(&view.key_field_type(&schema_type.name), view));
    let table_token_type = loader_id_type.as_ref().map(|key_type| {
        if key_type == "string" {
            format!("CoflowStringTableToken<{}>", view.csharp_type_ref(&schema_type.name))
        } else {
            format!(
                "CoflowEnumTableToken<{}, {key_type}>",
                view.csharp_type_ref(&schema_type.name)
            )
        }
    });
    Ok(CsharpType {
        name: view.csharp_type_name(&schema_type.name),
        namespace: view.csharp_namespace(&schema_type.name),
        qualified_name: view.csharp_type_ref(&schema_type.name),
        relative_path: view.csharp_relative_path(&schema_type.name),
        metadata_name: view.metadata_name(&schema_type.name),
        source_name: schema_type.name.to_string(),
        annotations: csharp_annotations(&schema_type.annotations),
        declaration: type_declaration(schema_type, view),
        constructor_visibility: if schema_type.is_abstract {
            "protected internal".to_string()
        } else {
            "internal".to_string()
        },
        summary: csharp_summary(schema_type.display.as_ref()),
        obsolete: false,
        properties,
        functions,
        host_fields,
        uses_host_slot: !is_struct,
        declares_host_slot: !is_struct && schema_type.parent.is_none(),
        constructor_parameters,
        base_constructor_call: (!base_constructor_args.is_empty())
            .then(|| format!(" : base({})", base_constructor_args.join(", "))),
        base_constructor_args,
        assignments,
        equality,
        loader_fields,
        loader_id_type,
        table_token_type,
        loader_id_reader: (!schema_type.is_singleton && !schema_type.is_abstract)
            .then(|| view.id_as_enum(&schema_type.name).map(|name| view.metadata_name(&name)))
            .flatten(),
        loader_enabled: !schema_type.is_abstract,
        is_host: schema_type.is_host,
        is_abstract: schema_type.is_abstract,
        is_sealed: schema_type.is_sealed,
        is_struct,
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
    let mut constructor_parameters = vec![
        CsharpParameter {
            ty: "CoflowHostState?".to_string(),
            name: "hostSlot".to_string(),
        },
        CsharpParameter {
            ty: "string".to_string(),
            name: "id".to_string(),
        },
    ];
    let mut properties = vec![CsharpProperty {
        visibility: "public".to_string(),
        name: "Id".to_string(),
        type_name: "string".to_string(),
        backing_field: None,
        guard_host: false,
        summary: None,
        obsolete: false,
    }];
    let mut assignments = vec![
        CsharpConstructorAssignment {
            property: "HostSlot".to_string(),
            target: "_coflowHost".to_string(),
            parameter: "hostSlot".to_string(),
        },
        CsharpConstructorAssignment {
            property: "Id".to_string(),
            target: "Id".to_string(),
            parameter: "id".to_string(),
        },
    ];
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
            false,
        );
    }

    let loader_fields = table
        .fields
        .iter()
        .map(|field| CsharpLoaderField {
            source_name: field.name.to_string(),
            property_name: csharp_public_member_name(&field.name),
            value_type: csharp_type(&field.value_type, view),
            is_function: matches!(field.value_type, CftValueType::Function(_, _)),
            reader_expression: function_loader_reader(field, view, "VALUE", "CONTEXT", true)
                .unwrap_or_else(|| loader_reader(&field.value_type, view, "VALUE", "CONTEXT")),
            default_expression: None,
            object_type: match &field.value_type {
                CftValueType::Object(name) => Some(name.to_string()),
                _ => None,
            },
            reference_type: match &field.value_type {
                CftValueType::RecordRef(name) => Some(name.to_string()),
                _ => None,
            },
            annotations: csharp_annotations(&field.annotations),
        })
        .collect();
    Ok(CsharpType {
        name: name.clone(),
        namespace: view.csharp_namespace(&table.source_name),
        qualified_name: view.csharp_type_ref(&table.source_name),
        relative_path: view.csharp_relative_path(&table.source_name),
        metadata_name: view.metadata_name(&table.source_name),
        source_name: table.source_name.clone(),
        annotations: Vec::new(),
        declaration: format!("public sealed partial class {name} : IEquatable<{name}>"),
        constructor_visibility: "public".to_string(),
        summary: None,
        obsolete: false,
        properties,
        functions: Vec::new(),
        host_fields: Vec::new(),
        uses_host_slot: true,
        declares_host_slot: true,
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
        table_token_type: Some(format!(
            "CoflowStringTableToken<{}>",
            view.csharp_type_ref(&table.source_name)
        )),
        loader_id_reader: None,
        loader_enabled: true,
        is_host: false,
        is_abstract: false,
        is_sealed: true,
        is_struct: false,
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
        CftValueType::Int => format!("CfdValueReader.Int64({node})"),
        CftValueType::Float => format!("CfdValueReader.Float64({node})"),
        CftValueType::Bool => format!("CfdValueReader.Boolean({node})"),
        CftValueType::String => format!("CfdValueReader.String({node}, {context})"),
        CftValueType::Enum(name) => format!("ReadEnum{}({node})", view.metadata_name(name)),
        CftValueType::Object(name) => format!("Read{}({node}, {context})", view.metadata_name(name)),
        CftValueType::RecordRef(name) => format!(
            "CfdValueReader.Reference<{}>({node}, {context}, \"{}\")",
            view.csharp_type_ref(name), name
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
        CftValueType::Option(inner) => format!(
            "CfdValueReader.Option({node}, {context}, static (item, context) => {})",
            loader_reader(inner, view, "item", "context")
        ),
        CftValueType::Result(value, error) => format!(
            "CfdValueReader.Result({node}, {context}, static (item, context) => {}, static (item, context) => {})",
            loader_reader(value, view, "item", "context"),
            loader_reader(error, view, "item", "context")
        ),
        CftValueType::Unit => {
            "default!".to_string()
        }
        CftValueType::Function(parameters, result) => {
            let delegate_type = csharp_type(ty, view);
            let parameter_types = parameters
                .iter()
                .map(|parameter| format!("typeof({})", csharp_type(&parameter.value_type, view)))
                .collect::<Vec<_>>();
            format!(
                "{context}.FunctionValueAot<{delegate_type}>({node}, typeof({}), new Type[] {{ {} }}, {})",
                csharp_type(result, view),
                parameter_types.join(", "),
                function_adapter_expression(parameters, result, view),
            )
        }
    }
}

fn function_loader_reader(
    field: &CftField,
    view: &CsharpLoweringPlan<'_>,
    node: &str,
    context: &str,
    required: bool,
) -> Option<String> {
    let CftValueType::Function(parameters, result) = &field.value_type else {
        return None;
    };
    let parameter_types = parameters
        .iter()
        .map(|parameter| {
            format!(", typeof({})", csharp_type(&parameter.value_type, view))
        })
        .collect::<String>();
    let method = if required { "RequiredFunction" } else { "Function" };
    Some(format!(
        "CoflowFunctionEntry<{}>.CreateAot({context}.{method}({node}, \"{}\", typeof({}){parameter_types}), {})",
        csharp_type(&field.value_type, view),
        escape_csharp_literal(&field.name),
        csharp_type(result, view),
        function_adapter_expression(parameters, result, view),
    ))
}

pub(crate) fn function_adapter_expression(
    parameters: &[CftFunctionParameter],
    result: &CftValueType,
    view: &CsharpLoweringPlan<'_>,
) -> String {
    debug_assert!(parameters.len() <= 8, "function arity is validated while building the project");
    let parameter_types = parameters
        .iter()
        .map(|parameter| csharp_type(&parameter.value_type, view))
        .collect::<Vec<_>>();
    let arguments = (0..parameters.len())
        .map(|index| format!("arg{index}"))
        .collect::<Vec<_>>();
    let lambda_parameters = if arguments.is_empty() {
        "()".to_string()
    } else {
        format!("({})", arguments.join(", "))
    };
    let generic_parameters = parameter_types.join(", ");
    let call = if matches!(result, CftValueType::Unit) {
        if arguments.is_empty() {
            "entry.InvokeVoid()".to_string()
        } else {
            format!("entry.InvokeVoid<{generic_parameters}>({})", arguments.join(", "))
        }
    } else {
        let result_type = csharp_type(result, view);
        let type_arguments = if generic_parameters.is_empty() {
            result_type
        } else {
            format!("{generic_parameters}, {result_type}")
        };
        format!("entry.Invoke<{type_arguments}>({})", arguments.join(", "))
    };
    format!("static entry => {lambda_parameters} => {call}")
}

fn loader_default(
    value: &CftSchemaDefaultValue,
    ty: &CftValueType,
    view: &CsharpLoweringPlan<'_>,
) -> Result<String, CsharpCodegenError> {
    loader_default_inner(value, ty, view, &mut Vec::new())
}

fn loader_default_inner(
    value: &CftSchemaDefaultValue,
    ty: &CftValueType,
    view: &CsharpLoweringPlan<'_>,
    object_stack: &mut Vec<coflow_language::TypeName>,
) -> Result<String, CsharpCodegenError> {
    Ok(match value {
        CftSchemaDefaultValue::OptionNone => {
            let CftValueType::Option(inner) = ty else {
                return Err(CsharpCodegenError::new("None default used on a non-Option field"));
            };
            format!("Option<{}>.None", csharp_type(inner, view))
        }
        CftSchemaDefaultValue::OptionSome(value) => {
            let CftValueType::Option(inner) = ty else {
                return Err(CsharpCodegenError::new("Some default used on a non-Option field"));
            };
            format!(
                "Option<{}>.Some({})",
                csharp_type(inner, view),
                loader_default_inner(value, inner, view, object_stack)?
            )
        }
        CftSchemaDefaultValue::ResultOk(value) => {
            let CftValueType::Result(ok, error) = ty else {
                return Err(CsharpCodegenError::new("Ok default used on a non-Result field"));
            };
            format!(
                "Result<{}, {}>.Ok({})",
                csharp_type(ok, view),
                csharp_type(error, view),
                loader_default_inner(value, ok, view, object_stack)?
            )
        }
        CftSchemaDefaultValue::ResultErr(value) => {
            let CftValueType::Result(ok, error) = ty else {
                return Err(CsharpCodegenError::new("Err default used on a non-Result field"));
            };
            format!(
                "Result<{}, {}>.Err({})",
                csharp_type(ok, view),
                csharp_type(error, view),
                loader_default_inner(value, error, view, object_stack)?
            )
        }
        CftSchemaDefaultValue::Int(value) => {
            if *value == i64::MIN {
                "(-9223372036854775807L - 1L)".to_string()
            } else {
                format!("{value}L")
            }
        }
        CftSchemaDefaultValue::Float(value) => {
            format!("{}D", float_literal(*value))
        }
        CftSchemaDefaultValue::Bool(value) => value.to_string(),
        CftSchemaDefaultValue::String(value) => {
            format!("\"{}\"", escape_csharp_literal(value))
        }
        CftSchemaDefaultValue::Enum {
            enum_name, variant, ..
        } => format!(
            "{}.{}",
            view.csharp_enum_ref(enum_name),
            csharp_public_member_name(variant)
        ),
        CftSchemaDefaultValue::EmptyArray => {
            let CftValueType::Array(item) = ty else {
                return Err(CsharpCodegenError::new("empty array default used on a non-array field"));
            };
            format!("Array.Empty<{}>()", csharp_type(item, view))
        }
        CftSchemaDefaultValue::EmptyObject => match ty {
            CftValueType::Dict(key, value) => format!(
                "new Dictionary<{}, {}>()",
                csharp_type(key, view),
                csharp_type(value, view)
            ),
            CftValueType::Object(name) => {
                loader_object_default(name, &[], view, object_stack)?
            }
            _ => {
                return Err(CsharpCodegenError::new(
                    "empty object default used on a non-object field",
                ));
            }
        },
        CftSchemaDefaultValue::Array(values) => {
            let CftValueType::Array(item) = ty else {
                return Err(CsharpCodegenError::new("array default used on a non-array field"));
            };
            let values = values
                .iter()
                .map(|value| loader_default_inner(value, item, view, object_stack))
                .collect::<Result<Vec<_>, _>>()?
                .join(", ");
            format!(
                "CoflowConstantValues.List<{}>({values})",
                csharp_type(item, view)
            )
        }
        CftSchemaDefaultValue::Dictionary(entries) => {
            let CftValueType::Dict(key, item) = ty else {
                return Err(CsharpCodegenError::new(
                    "dictionary default used on a non-dictionary field",
                ));
            };
            let entries = entries
                .iter()
                .map(|(entry_key, value)| {
                    Ok(format!(
                        "new KeyValuePair<{}, {}>({}, {})",
                        csharp_type(key, view),
                        csharp_type(item, view),
                        loader_default_inner(entry_key, key, view, object_stack)?,
                        loader_default_inner(value, item, view, object_stack)?,
                    ))
                })
                .collect::<Result<Vec<_>, CsharpCodegenError>>()?
                .join(", ");
            format!(
                "CoflowConstantValues.Dictionary<{}, {}>({entries})",
                csharp_type(key, view),
                csharp_type(item, view)
            )
        }
        CftSchemaDefaultValue::Object { type_name, fields } => {
            let CftValueType::Object(expected) = ty else {
                return Err(CsharpCodegenError::new("object default used on a non-object field"));
            };
            if type_name != expected {
                return Err(CsharpCodegenError::new(format!(
                    "object default `{type_name}` does not match `{expected}`"
                )));
            }
            loader_object_default(type_name, fields, view, object_stack)?
        }
        CftSchemaDefaultValue::RecordReference { type_name, key } => {
            let CftValueType::RecordRef(expected) = ty else {
                return Err(CsharpCodegenError::new(
                    "record-reference default used on a non-reference field",
                ));
            };
            if type_name != expected {
                return Err(CsharpCodegenError::new(format!(
                    "record-reference default `{type_name}` does not match `{expected}`"
                )));
            }
            format!(
                "context.Resolve<{}>(\"{}\", \"{}\")",
                view.csharp_type_ref(type_name),
                escape_csharp_literal(type_name.as_str()),
                escape_csharp_literal(key),
            )
        }
    })
}

fn loader_object_default(
    type_name: &coflow_language::TypeName,
    fields: &[(coflow_language::FieldName, CftSchemaDefaultValue)],
    view: &CsharpLoweringPlan<'_>,
    object_stack: &mut Vec<coflow_language::TypeName>,
) -> Result<String, CsharpCodegenError> {
    if object_stack.contains(type_name) {
        let mut cycle = object_stack
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        cycle.push(type_name.to_string());
        return Err(CsharpCodegenError::new(format!(
            "schema default dependency cycle: {}",
            cycle.join(" -> ")
        )));
    }
    let schema_type = view.resolve_type(type_name)?;
    if schema_type.is_abstract {
        return Err(CsharpCodegenError::new(format!(
            "object default cannot instantiate abstract CFT type `{type_name}`"
        )));
    }
    object_stack.push(type_name.clone());
    let result = (|| {
        let values = fields
            .iter()
            .map(|(name, value)| (name, value))
            .collect::<std::collections::BTreeMap<_, _>>();
        let mut arguments = Vec::new();
        if !schema_type.is_struct {
            arguments.push("null".to_string());
        }
        if view.is_ref_target_loadable(type_name) {
            arguments.push(match view.key_field_type(type_name) {
                CftValueType::String => "string.Empty".to_string(),
                CftValueType::Enum(name) => format!("default({})", view.csharp_enum_ref(&name)),
                _ => unreachable!("record keys are string or enum"),
            });
        }
        arguments.extend(view
            .fields(type_name.as_str())?
            .map(|field| {
                if matches!(field.value_type, CftValueType::Function(_, _)) {
                    return Err(CsharpCodegenError::new(format!(
                        "object default `{type_name}` cannot contain function field `{}`",
                        field.name
                    )));
                }
                if let Some(value) = values.get(&field.name) {
                    loader_default_inner(value, &field.value_type, view, object_stack)
                } else if let Some(value) = &field.default {
                    loader_default_inner(value, &field.value_type, view, object_stack)
                } else {
                    Err(CsharpCodegenError::new(format!(
                        "object default `{type_name}` is missing field `{}`",
                        field.name
                    )))
                }
            })
            .collect::<Result<Vec<_>, _>>()?);
        Ok(format!(
            "new {}({})",
            view.csharp_type_ref(type_name),
            arguments.join(", ")
        ))
    })();
    object_stack.pop();
    result
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
        guard_host: false,
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
    _view: &CsharpLoweringPlan<'_>,
    properties: &mut Vec<CsharpProperty>,
    assignments: &mut Vec<CsharpConstructorAssignment>,
    is_struct: bool,
) {
    let property_name = csharp_public_member_name(&field.name);
    let backing_field = backing_field_name(&property_name, is_struct);
    properties.push(CsharpProperty {
        visibility: "public".to_string(),
        name: property_name.clone(),
        type_name: property_type,
        backing_field: backing_field.clone(),
        guard_host: !is_struct,
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

pub(crate) fn csharp_annotations(annotations: &[CftAnnotation]) -> Vec<CsharpAnnotation> {
    annotations
        .iter()
        .map(|annotation| CsharpAnnotation {
            name: annotation.name.clone(),
            arguments: annotation
                .arguments
                .iter()
                .map(|argument| match argument {
                    CftAnnotationValue::Name(value) => CsharpAnnotationArgument {
                        kind: "Name",
                        value_expression: format!("\"{}\"", escape_csharp_literal(value)),
                    },
                    CftAnnotationValue::String(value) => CsharpAnnotationArgument {
                        kind: "String",
                        value_expression: format!("\"{}\"", escape_csharp_literal(value)),
                    },
                    CftAnnotationValue::Int(value) => CsharpAnnotationArgument {
                        kind: "Int",
                        value_expression: format!("{value}L"),
                    },
                    CftAnnotationValue::Float(value) => CsharpAnnotationArgument {
                        kind: "Float",
                        value_expression: format!("{value}D"),
                    },
                    CftAnnotationValue::Bool(value) => CsharpAnnotationArgument {
                        kind: "Bool",
                        value_expression: value.to_string(),
                    },
                })
                .collect(),
        })
        .collect()
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
    is_struct: bool,
) -> Option<String> {
    (!is_struct).then(|| format!("_coflow{property_name}"))
}

fn type_declaration(schema_type: &CftType, view: &CsharpLoweringPlan<'_>) -> String {
    let prefix = if schema_type.is_abstract {
        "public abstract partial class"
    } else if schema_type.is_struct {
        "public sealed partial class"
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
        interfaces.push(view.csharp_type_ref(parent));
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
