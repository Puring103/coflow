use crate::model::{
    CsharpDatabase, CsharpEnum, CsharpEnumVariant, CsharpIndex, CsharpLoadField, CsharpLoader,
    CsharpParameter, CsharpPolymorphicCase, CsharpPolymorphicLoader, CsharpProperty, CsharpResolve,
    CsharpResolveCase, CsharpResolveMethod, CsharpResolveTableCall, CsharpTable, CsharpType,
};
use crate::names::*;
use crate::schema_view::{FieldMeta, FieldType, SchemaView, TypeMeta};
use crate::CsharpCodegenError;
use coflow_cft::{CftSchemaDefaultValue, CftSchemaEnum, CftSchemaField, CftSchemaType};

pub(crate) fn build_csharp_enum(schema_enum: &CftSchemaEnum) -> CsharpEnum {
    CsharpEnum {
        name: schema_enum.name.clone(),
        is_flags: has_annotation(&schema_enum.annotations, "flag"),
        summary: display_annotation(&schema_enum.annotations),
        obsolete: has_annotation(&schema_enum.annotations, "deprecated"),
        variants: schema_enum
            .variants
            .iter()
            .map(|variant| CsharpEnumVariant {
                name: variant.name.clone(),
                value: variant.value,
                summary: display_annotation(&variant.annotations),
                obsolete: has_annotation(&variant.annotations, "deprecated"),
            })
            .collect(),
    }
}

pub(crate) fn build_csharp_type(
    schema_type: &CftSchemaType,
    view: &SchemaView,
) -> Result<CsharpType, CsharpCodegenError> {
    let mut properties = Vec::new();

    for field in &schema_type.fields {
        let field_ty = FieldType::from_schema(&field.ty_ref, &view.enums);
        let ref_target = annotation_name_arg(&field.annotations, "ref");

        properties.push(CsharpProperty {
            name: pascal_case(&field.name),
            type_name: csharp_type(&field_ty, view),
            setter: "init".to_string(),
            initializer: default_initializer(field, &field_ty)?,
            summary: display_annotation(&field.annotations),
            obsolete: has_annotation(&field.annotations, "deprecated"),
        });

        if let Some(target) = ref_target {
            properties.push(CsharpProperty {
                name: ref_property_name(&field.name, &target),
                type_name: if field_ty.is_nullable() {
                    format!("{target}?")
                } else {
                    target
                },
                setter: "internal set".to_string(),
                initializer: if field_ty.is_nullable() {
                    None
                } else {
                    Some("null!".to_string())
                },
                summary: None,
                obsolete: has_annotation(&field.annotations, "deprecated"),
            });
        }
    }

    Ok(CsharpType {
        name: schema_type.name.clone(),
        declaration: type_declaration(schema_type),
        summary: display_annotation(&schema_type.annotations),
        obsolete: has_annotation(&schema_type.annotations, "deprecated"),
        properties,
    })
}

pub(crate) fn build_csharp_database(
    view: &SchemaView,
    tables: &[String],
    _database_class: &str,
) -> Result<CsharpDatabase, CsharpCodegenError> {
    let table_models = tables
        .iter()
        .map(|table_name| build_table_model(view, table_name))
        .collect::<Result<Vec<_>, _>>()?;
    let indexes = indexed_fields(view, tables)
        .into_iter()
        .map(|indexed| build_index_model(view, indexed))
        .collect::<Vec<_>>();
    let mut parameters = Vec::<CsharpParameter>::new();
    let mut load_steps = Vec::new();

    for table in &table_models {
        parameters.push(CsharpParameter {
            ty: format!("List<{}>", table.name),
            name: table.list_var.clone(),
        });
        parameters.push(CsharpParameter {
            ty: format!("Dictionary<{}, {}>", table.id_type, table.name),
            name: table.index_var.clone(),
        });
        load_steps.push(format!(
            "var {} = LoadTable(Path.Combine(dataDir, \"{}.json\"), \"{}\", Load{});",
            table.list_var, table.name, table.name, table.name
        ));
    }

    for table in &table_models {
        load_steps.push(format!(
            "var {} = BuildUniqueIndex({}, x => x.{}, \"{}\", \"{}\");",
            table.index_var, table.list_var, table.id_property, table.name, table.id_source_name
        ));
    }

    for index in &indexes {
        parameters.push(CsharpParameter {
            ty: format!("Dictionary<{}, List<{}>>", index.key_type, index.table_name),
            name: index.parameter_name.clone(),
        });
        load_steps.push(format!(
            "var {} = BuildMultiIndex({}, x => x.{});",
            index.parameter_name, index.list_var, index.field_property
        ));
    }

    let ref_targets = view.ref_target_names();
    let resolve = if ref_targets.is_empty() {
        None
    } else {
        load_steps.push(format!(
            "ResolveRefs({});",
            resolve_arguments(tables, &ref_targets).join(", ")
        ));
        Some(build_resolve_model(view, tables, &ref_targets)?)
    };

    let constructor_args = parameters
        .iter()
        .map(|parameter| parameter.name.clone())
        .collect::<Vec<_>>();

    Ok(CsharpDatabase {
        tables: table_models,
        indexes,
        constructor_parameters: parameters,
        load_steps,
        constructor_args,
        loaders: loader_methods(view)?,
        polymorphic_loaders: polymorphic_loaders(view)?,
        resolve,
    })
}

#[derive(Debug, Clone)]
struct IndexedField {
    table: String,
    field: FieldMeta,
}

fn build_table_model(
    view: &SchemaView,
    table_name: &str,
) -> Result<CsharpTable, CsharpCodegenError> {
    let table = view.type_meta(table_name)?;
    let id_field = table.id_field()?;
    Ok(CsharpTable {
        name: table_name.to_string(),
        list_property: pluralize(table_name),
        list_var: camel_case(&pluralize(table_name)),
        item_var: camel_case(table_name),
        id_type: csharp_type(&id_field.ty, view),
        id_property: pascal_case(&id_field.name),
        id_source_name: id_field.name.clone(),
        index_field: index_var_name(table_name),
        index_var: index_param_name(table_name),
    })
}

fn build_index_model(view: &SchemaView, indexed: IndexedField) -> CsharpIndex {
    let storage_field = multi_index_var_name(&indexed.table, &indexed.field.name);
    CsharpIndex {
        table_name: indexed.table.clone(),
        list_property: pluralize(&indexed.table),
        list_var: camel_case(&pluralize(&indexed.table)),
        field_property: pascal_case(&indexed.field.name),
        key_type: csharp_type(&indexed.field.ty, view),
        parameter_name: storage_field.trim_start_matches('_').to_string(),
        storage_field,
    }
}

fn indexed_fields(view: &SchemaView, tables: &[String]) -> Vec<IndexedField> {
    let mut out = Vec::new();
    for table_name in tables {
        if let Some(table) = view.types.get(table_name) {
            for field in table.index_fields() {
                out.push(IndexedField {
                    table: table_name.clone(),
                    field: field.clone(),
                });
            }
        }
    }
    out
}

fn loader_methods(view: &SchemaView) -> Result<Vec<CsharpLoader>, CsharpCodegenError> {
    view.non_abstract_type_names()
        .into_iter()
        .map(|type_name| {
            let ty = view.type_meta(&type_name)?;
            Ok(CsharpLoader {
                type_name,
                fields: ty
                    .all_fields
                    .iter()
                    .map(|field| {
                        Ok(CsharpLoadField {
                            property: pascal_case(&field.name),
                            read_expr: read_field_expr(field, "obj", "path", view)?,
                        })
                    })
                    .collect::<Result<Vec<_>, CsharpCodegenError>>()?,
            })
        })
        .collect()
}

fn polymorphic_loaders(
    view: &SchemaView,
) -> Result<Vec<CsharpPolymorphicLoader>, CsharpCodegenError> {
    view.polymorphic_type_names()
        .into_iter()
        .map(|type_name| {
            let assignable = view.concrete_assignable_types(&type_name)?;
            Ok(CsharpPolymorphicLoader {
                type_name,
                expected: assignable.join(" | "),
                cases: assignable
                    .into_iter()
                    .map(|type_name| CsharpPolymorphicCase { type_name })
                    .collect(),
            })
        })
        .collect()
}

fn build_resolve_model(
    view: &SchemaView,
    tables: &[String],
    ref_targets: &[String],
) -> Result<CsharpResolve, CsharpCodegenError> {
    let parameters = resolve_parameters(view, tables, ref_targets)?;
    let mut table_calls = Vec::new();
    for table_name in tables {
        let table = view.type_meta(table_name)?;
        let id_field = table.id_field()?;
        table_calls.push(CsharpResolveTableCall {
            table_name: table_name.clone(),
            list_var: camel_case(&pluralize(table_name)),
            item_var: camel_case(table_name),
            id_property: pascal_case(&id_field.name),
            index_args: resolve_index_argument_list(ref_targets),
            path_expr: format!(
                "$\"{table_name}[{{{}.{}}}]\"",
                camel_case(table_name),
                pascal_case(&id_field.name)
            ),
        });
    }

    let mut methods = Vec::new();
    for type_name in view.all_type_names() {
        let ty = view.type_meta(&type_name)?;
        methods.push(if ty.is_abstract {
            build_polymorphic_resolver(view, ty, ref_targets)?
        } else {
            build_type_resolver(view, ty, ref_targets)?
        });
    }

    Ok(CsharpResolve {
        parameters,
        table_calls,
        methods,
    })
}

fn build_type_resolver(
    view: &SchemaView,
    ty: &TypeMeta,
    ref_targets: &[String],
) -> Result<CsharpResolveMethod, CsharpCodegenError> {
    let mut statements = Vec::new();
    for field in &ty.all_fields {
        push_resolve_field(&mut statements, view, field, ref_targets)?;
    }
    Ok(CsharpResolveMethod {
        type_name: ty.name.clone(),
        is_polymorphic: false,
        parameters: resolve_index_parameter_models(view, ref_targets)?,
        statements,
        cases: Vec::new(),
    })
}

fn build_polymorphic_resolver(
    view: &SchemaView,
    ty: &TypeMeta,
    ref_targets: &[String],
) -> Result<CsharpResolveMethod, CsharpCodegenError> {
    Ok(CsharpResolveMethod {
        type_name: ty.name.clone(),
        is_polymorphic: true,
        parameters: resolve_index_parameter_models(view, ref_targets)?,
        statements: Vec::new(),
        cases: view
            .concrete_assignable_types(&ty.name)?
            .into_iter()
            .map(|type_name| CsharpResolveCase {
                var_name: camel_case(&type_name),
                type_name,
                index_args: resolve_index_argument_list(ref_targets),
            })
            .collect(),
    })
}

fn push_resolve_field(
    out: &mut Vec<String>,
    view: &SchemaView,
    field: &FieldMeta,
    ref_targets: &[String],
) -> Result<(), CsharpCodegenError> {
    let property = pascal_case(&field.name);

    if let Some(target) = annotation_name_arg(&field.annotations, "ref") {
        let ref_property = ref_property_name(&field.name, &target);
        let target_index = index_var_name(&target);
        if field.ty.is_nullable() {
            let id_access = nullable_ref_id_access(field, &property);
            out.push(format!("if (value.{property} != null)"));
            out.push("{".to_string());
            out.push(format!(
                "    value.{ref_property} = ResolveRef({target_index}, {id_access}, $\"{{path}}.{}\", \"{target}\");",
                field.name
            ));
            out.push("}".to_string());
        } else {
            out.push(format!(
                "value.{ref_property} = ResolveRef({target_index}, value.{property}, $\"{{path}}.{}\", \"{target}\");",
                field.name
            ));
        }
        return Ok(());
    }

    push_resolve_nested_value(
        out,
        view,
        &field.ty,
        &format!("value.{property}"),
        &field.name,
        ref_targets,
    )
}

fn nullable_ref_id_access(field: &FieldMeta, property: &str) -> String {
    match field.ty.non_nullable() {
        FieldType::Int => format!("value.{property}.Value"),
        _ => format!("value.{property}"),
    }
}

fn push_resolve_nested_value(
    out: &mut Vec<String>,
    view: &SchemaView,
    ty: &FieldType,
    access: &str,
    path_suffix: &str,
    ref_targets: &[String],
) -> Result<(), CsharpCodegenError> {
    match ty {
        FieldType::Type(type_name) => {
            out.push(format!(
                "Resolve{type_name}Refs({access}, {}, $\"{{path}}.{path_suffix}\");",
                resolve_index_argument_list(ref_targets)
            ));
        }
        FieldType::Array(inner) => {
            if value_needs_resolve(inner, view) {
                out.push(format!("for (var i = 0; i < {access}.Count; i++)"));
                out.push("{".to_string());
                let mut inner_statements = Vec::new();
                push_resolve_nested_value(
                    &mut inner_statements,
                    view,
                    inner,
                    &format!("{access}[i]"),
                    &format!("{path_suffix}[{{i}}]"),
                    ref_targets,
                )?;
                out.extend(
                    inner_statements
                        .into_iter()
                        .map(|line| format!("    {line}")),
                );
                out.push("}".to_string());
            }
        }
        FieldType::Dict(_, value) => {
            if value_needs_resolve(value, view) {
                out.push(format!("foreach (var pair in {access})"));
                out.push("{".to_string());
                let mut inner_statements = Vec::new();
                push_resolve_nested_value(
                    &mut inner_statements,
                    view,
                    value,
                    "pair.Value",
                    &format!("{path_suffix}[{{pair.Key}}]"),
                    ref_targets,
                )?;
                out.extend(
                    inner_statements
                        .into_iter()
                        .map(|line| format!("    {line}")),
                );
                out.push("}".to_string());
            }
        }
        FieldType::Nullable(inner) => {
            if value_needs_resolve(inner, view) {
                out.push(format!("if ({access} != null)"));
                out.push("{".to_string());
                let mut inner_statements = Vec::new();
                push_resolve_nested_value(
                    &mut inner_statements,
                    view,
                    inner,
                    access,
                    path_suffix,
                    ref_targets,
                )?;
                out.extend(
                    inner_statements
                        .into_iter()
                        .map(|line| format!("    {line}")),
                );
                out.push("}".to_string());
            }
        }
        FieldType::Int
        | FieldType::Float
        | FieldType::Bool
        | FieldType::String
        | FieldType::Enum(_) => {}
    }
    Ok(())
}

fn value_needs_resolve(ty: &FieldType, view: &SchemaView) -> bool {
    match ty {
        FieldType::Type(name) => view.range_contains_ref(name),
        FieldType::Array(inner) | FieldType::Nullable(inner) => value_needs_resolve(inner, view),
        FieldType::Dict(_, value) => value_needs_resolve(value, view),
        FieldType::Int
        | FieldType::Float
        | FieldType::Bool
        | FieldType::String
        | FieldType::Enum(_) => false,
    }
}

fn resolve_parameters(
    view: &SchemaView,
    tables: &[String],
    ref_targets: &[String],
) -> Result<Vec<CsharpParameter>, CsharpCodegenError> {
    let mut out = Vec::new();

    for table_name in tables {
        out.push(CsharpParameter {
            ty: format!("List<{table_name}>"),
            name: camel_case(&pluralize(table_name)),
        });
    }

    for target in ref_targets {
        let target_meta = view.type_meta(target)?;
        let id_type = csharp_type(&target_meta.id_field()?.ty, view);
        out.push(CsharpParameter {
            ty: format!("Dictionary<{id_type}, {target}>"),
            name: index_var_name(target),
        });
    }

    Ok(out)
}

fn resolve_index_parameter_models(
    view: &SchemaView,
    ref_targets: &[String],
) -> Result<Vec<CsharpParameter>, CsharpCodegenError> {
    let mut out = Vec::new();
    for target in ref_targets {
        let target_meta = view.type_meta(target)?;
        let id_type = csharp_type(&target_meta.id_field()?.ty, view);
        out.push(CsharpParameter {
            ty: format!("Dictionary<{id_type}, {target}>"),
            name: index_var_name(target),
        });
    }
    Ok(out)
}

fn resolve_arguments(tables: &[String], ref_targets: &[String]) -> Vec<String> {
    tables
        .iter()
        .map(|table| camel_case(&pluralize(table)))
        .chain(ref_targets.iter().map(|target| index_param_name(target)))
        .collect()
}

fn resolve_index_argument_list(ref_targets: &[String]) -> String {
    ref_targets
        .iter()
        .map(|target| index_var_name(target))
        .collect::<Vec<_>>()
        .join(", ")
}

fn read_field_expr(
    field: &FieldMeta,
    obj: &str,
    path: &str,
    view: &SchemaView,
) -> Result<String, CsharpCodegenError> {
    let name = &field.name;

    if annotation_name_arg(&field.annotations, "ref").is_some() {
        return read_value_expr(&field.ty, obj, name, path, view);
    }

    read_value_expr(&field.ty, obj, name, path, view)
}

fn read_value_expr(
    ty: &FieldType,
    obj: &str,
    name: &str,
    path: &str,
    view: &SchemaView,
) -> Result<String, CsharpCodegenError> {
    if let FieldType::Nullable(inner) = ty {
        return Ok(format!(
            "ReadNullable({obj}, \"{name}\", {path}, (token, childPath) => {})",
            read_token_expr(inner, "token", "childPath", view)?
        ));
    }

    Ok(format!(
        "ReadRequired({obj}, \"{name}\", {path}, (token, childPath) => {})",
        read_token_expr(ty, "token", "childPath", view)?
    ))
}

fn read_token_expr(
    ty: &FieldType,
    token: &str,
    path: &str,
    view: &SchemaView,
) -> Result<String, CsharpCodegenError> {
    match ty {
        FieldType::Int => Ok(format!("ReadInt({token}, {path})")),
        FieldType::Float => Ok(format!("ReadFloat({token}, {path})")),
        FieldType::Bool => Ok(format!("ReadBool({token}, {path})")),
        FieldType::String => Ok(format!("ReadString({token}, {path})")),
        FieldType::Enum(name) => Ok(format!("ReadEnum<{name}>({token}, {path})")),
        FieldType::Type(name) => {
            if view.range_is_polymorphic(name) {
                Ok(format!("Load{name}Polymorphic({token}, {path})"))
            } else {
                Ok(format!("Load{name}({token}, {path})"))
            }
        }
        FieldType::Array(inner) => Ok(format!(
            "ReadArray({token}, {path}, (item, itemPath) => {})",
            read_token_expr(inner, "item", "itemPath", view)?
        )),
        FieldType::Dict(key, value) => Ok(format!(
            "ReadDict({token}, {path}, (key, keyPath) => {}, (value, valuePath) => {})",
            read_dict_key_expr(key, "key", "keyPath")?,
            read_token_expr(value, "value", "valuePath", view)?
        )),
        FieldType::Nullable(inner) => Ok(format!(
            "{token}.Type == JTokenType.Null ? null : {}",
            read_token_expr(inner, token, path, view)?
        )),
    }
}

fn read_dict_key_expr(ty: &FieldType, key: &str, path: &str) -> Result<String, CsharpCodegenError> {
    match ty.non_nullable() {
        FieldType::String => Ok(key.to_string()),
        FieldType::Int => Ok(format!("ReadIntKey({key}, {path})")),
        FieldType::Enum(name) => Ok(format!("ReadEnumKey<{name}>({key}, {path})")),
        _ => Err(CsharpCodegenError::new(
            "dictionary key type must be string, int, or enum",
        )),
    }
}

fn csharp_type(ty: &FieldType, view: &SchemaView) -> String {
    let _ = view;
    match ty {
        FieldType::Int => "long".to_string(),
        FieldType::Float => "float".to_string(),
        FieldType::Bool => "bool".to_string(),
        FieldType::String => "string".to_string(),
        FieldType::Type(name) | FieldType::Enum(name) => name.clone(),
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

fn type_declaration(schema_type: &CftSchemaType) -> String {
    let prefix = if schema_type.is_abstract {
        "public abstract partial class"
    } else if has_annotation(&schema_type.annotations, "struct") {
        "public partial struct"
    } else if schema_type.is_sealed {
        "public sealed partial class"
    } else {
        "public partial class"
    };

    let parent = schema_type
        .parent
        .as_ref()
        .filter(|_| !has_annotation(&schema_type.annotations, "struct"))
        .map(|parent| format!(" : {parent}"))
        .unwrap_or_default();

    format!("{prefix} {}{parent}", schema_type.name)
}

fn default_initializer(
    field: &CftSchemaField,
    ty: &FieldType,
) -> Result<Option<String>, CsharpCodegenError> {
    if let Some(default) = &field.default {
        return Ok(Some(match default {
            CftSchemaDefaultValue::Null => "null".to_string(),
            CftSchemaDefaultValue::Int(value) => value.to_string(),
            CftSchemaDefaultValue::Float(value) => format_float(*value),
            CftSchemaDefaultValue::Bool(value) => value.to_string(),
            CftSchemaDefaultValue::String(value) => format!("\"{}\"", escape_csharp_string(value)),
            CftSchemaDefaultValue::Enum {
                enum_name, variant, ..
            } => format!("{enum_name}.{variant}"),
            CftSchemaDefaultValue::EmptyArray | CftSchemaDefaultValue::EmptyObject => {
                "new()".to_string()
            }
        }));
    }

    if field.has_default || ty.is_nullable() {
        return Ok(None);
    }

    Ok(match ty.non_nullable() {
        FieldType::String => Some("\"\"".to_string()),
        FieldType::Type(_) => Some("null!".to_string()),
        FieldType::Array(_) | FieldType::Dict(_, _) => Some("new()".to_string()),
        FieldType::Int
        | FieldType::Float
        | FieldType::Bool
        | FieldType::Enum(_)
        | FieldType::Nullable(_) => None,
    })
}
