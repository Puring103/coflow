use crate::ir::CsharpDataFormat;
use crate::model::{
    CsharpDatabase, CsharpEnum, CsharpEnumVariant, CsharpLoadAssignment, CsharpLoadField,
    CsharpLoader, CsharpParameter, CsharpPolymorphicCase, CsharpPolymorphicLoader, CsharpProperty,
    CsharpRefIndex, CsharpRefIndexSource, CsharpResolve, CsharpResolveCase, CsharpResolveMethod,
    CsharpResolveTableCall, CsharpTable, CsharpType,
};
use crate::names::{
    camel_case, csharp_ident_error, display_annotation, escape_csharp_string, format_float,
    has_annotation, index_param_name, index_var_name, pascal_case, pluralize, ref_index_param_name,
    ref_index_var_name,
};
use crate::schema_view::{FieldMeta, FieldType, SchemaView, TypeMeta};
use crate::CsharpCodegenError;
use coflow_cft::{CftSchemaDefaultValue, CftSchemaEnum, CftSchemaType};
use std::collections::HashSet;

pub fn build_csharp_enum(schema_enum: &CftSchemaEnum) -> CsharpEnum {
    CsharpEnum {
        name: crate::names::csharp_type_name(&schema_enum.name),
        is_flags: has_annotation(&schema_enum.annotations, "flag"),
        summary: display_annotation(&schema_enum.annotations),
        obsolete: has_annotation(&schema_enum.annotations, "deprecated"),
        variants: schema_enum
            .variants
            .iter()
            .map(|variant| CsharpEnumVariant {
                name: pascal_case(&variant.name),
                value: variant.value,
                summary: display_annotation(&variant.annotations),
                obsolete: has_annotation(&variant.annotations, "deprecated"),
            })
            .collect(),
    }
}

pub fn build_csharp_type(schema_type: &CftSchemaType, view: &SchemaView) -> CsharpType {
    let mut properties = Vec::new();
    let is_struct = has_annotation(&schema_type.annotations, "struct");
    let fields = if is_struct {
        view.types.get(&schema_type.name).map_or_else(
            || {
                schema_type
                    .fields
                    .iter()
                    .map(|field| FieldMeta::from_schema(field, &view.enums))
                    .collect()
            },
            |ty| ty.all_fields.clone(),
        )
    } else {
        schema_type
            .fields
            .iter()
            .map(|field| FieldMeta::from_schema(field, &view.enums))
            .collect()
    };

    if !schema_type.is_abstract {
        let key_ty = view.key_field_type(&schema_type.name);
        properties.push(CsharpProperty {
            visibility: "public".to_string(),
            name: "Id".to_string(),
            type_name: csharp_property_type(&key_ty, view),
            setter: "internal set".to_string(),
            initializer: if is_struct {
                None
            } else {
                default_initializer_for_type(&key_ty, view)
            },
            summary: None,
            obsolete: false,
        });
    }

    if is_struct || schema_type.parent.is_none() {
        properties.push(CsharpProperty {
            visibility: "internal".to_string(),
            name: ref_marker_property().to_string(),
            type_name: "bool".to_string(),
            setter: "set".to_string(),
            initializer: Some("false".to_string()),
            summary: None,
            obsolete: false,
        });
        properties.push(CsharpProperty {
            visibility: "internal".to_string(),
            name: ref_key_property().to_string(),
            type_name: "object?".to_string(),
            setter: "set".to_string(),
            initializer: None,
            summary: None,
            obsolete: false,
        });
    }

    for field in &fields {
        let field_ty = field.ty.clone();
        let csharp_ty = csharp_field_type(field, view);

        properties.push(CsharpProperty {
            visibility: "public".to_string(),
            name: pascal_case(&field.name),
            type_name: csharp_property_type(&csharp_ty, view),
            setter: if field_needs_internal_set(&field_ty, view) {
                "internal set".to_string()
            } else {
                "set".to_string()
            },
            initializer: if is_struct {
                None
            } else {
                default_initializer(field, &csharp_ty, view)
            },
            summary: display_annotation(&field.annotations),
            obsolete: has_annotation(&field.annotations, "deprecated"),
        });
    }

    CsharpType {
        name: view.csharp_type_name(&schema_type.name),
        declaration: type_declaration(schema_type, view),
        summary: display_annotation(&schema_type.annotations),
        obsolete: has_annotation(&schema_type.annotations, "deprecated"),
        properties,
    }
}

pub fn build_csharp_database(
    view: &SchemaView,
    tables: &[String],
    _database_class: &str,
    data_format: CsharpDataFormat,
) -> Result<CsharpDatabase, CsharpCodegenError> {
    let table_models: Vec<CsharpTable> = tables
        .iter()
        .map(|table_name| build_table_model(view, table_name))
        .collect();
    let ref_targets = view.ref_target_names();
    let ref_indexes = build_ref_indexes(view, tables, &ref_targets)?;
    let mut parameters = Vec::<CsharpParameter>::new();
    let mut load_steps = Vec::new();

    let load_extension = match data_format {
        CsharpDataFormat::Json => "json",
        CsharpDataFormat::MessagePack => "msgpack",
    };

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
            "var {} = LoadTable(Path.Combine(dataDir, \"{}.{}\"), \"{}\", Load{}Row);",
            table.list_var, table.source_name, load_extension, table.source_name, table.name
        ));
    }

    for table in &table_models {
        load_steps.push(format!(
            "var {} = BuildUniqueIndex({}, x => x.{}, \"{}\", \"{}\");",
            table.index_var,
            table.list_var,
            table.id_property,
            table.source_name,
            table.id_source_name
        ));
    }

    for ref_index in &ref_indexes {
        parameters.push(CsharpParameter {
            ty: format!(
                "Dictionary<{}, {}>",
                ref_index.target_id_type, ref_index.target_name
            ),
            name: ref_index.parameter_name.clone(),
        });
        load_steps.push(ref_index_load_step(ref_index));
    }

    let resolve = if ref_targets.is_empty() {
        None
    } else {
        load_steps.push(format!(
            "ResolveRefs({});",
            resolve_arguments(view, tables, &ref_targets).join(", ")
        ));
        Some(build_resolve_model(view, tables, &ref_targets)?)
    };

    let constructor_args = parameters
        .iter()
        .map(|parameter| parameter.name.clone())
        .collect::<Vec<_>>();

    Ok(CsharpDatabase {
        tables: table_models,
        ref_indexes,
        constructor_parameters: parameters,
        load_steps,
        constructor_args,
        loaders: loader_methods(view)?,
        polymorphic_loaders: polymorphic_loaders(view)?,
        resolve,
    })
}

fn build_table_model(view: &SchemaView, table_name: &str) -> CsharpTable {
    let csharp_name = view.csharp_type_name(table_name);
    let id_ty = view.key_field_type(table_name);
    CsharpTable {
        name: csharp_name.clone(),
        source_name: table_name.to_string(),
        list_property: pluralize(&csharp_name),
        list_var: camel_case(&pluralize(table_name)),
        item_var: camel_case(table_name),
        id_type: csharp_type(&id_ty, view),
        id_property: "Id".to_string(),
        id_source_name: "id".to_string(),
        index_field: index_var_name(&csharp_name),
        index_var: index_param_name(&csharp_name),
    }
}

fn build_ref_indexes(
    view: &SchemaView,
    tables: &[String],
    ref_targets: &[String],
) -> Result<Vec<CsharpRefIndex>, CsharpCodegenError> {
    let table_set = tables
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let mut out = Vec::new();

    for target in ref_targets {
        let target_id_ty = view.key_field_type(target);
        let csharp_target = view.csharp_type_name(target);
        let assignable_sources = view
            .concrete_assignable_types(target)?
            .into_iter()
            .filter(|type_name| table_set.contains(type_name))
            .map(|type_name| {
                Ok(CsharpRefIndexSource {
                    list_var: camel_case(&pluralize(&type_name)),
                    table_name: type_name.clone(),
                    index_var: index_param_name(&view.csharp_type_name(&type_name)),
                    id_property: "Id".to_string(),
                    id_source_name: "id".to_string(),
                })
            })
            .collect::<Result<Vec<_>, CsharpCodegenError>>()?;

        if assignable_sources.is_empty() {
            return Err(CsharpCodegenError::new(format!(
                "reference target `{target}` has no loadable key table"
            )));
        }

        out.push(CsharpRefIndex {
            target_name: csharp_target.clone(),
            target_source_name: target.clone(),
            target_id_type: csharp_type(&target_id_ty, view),
            index_field: ref_index_var_name(&csharp_target),
            parameter_name: ref_index_param_name(&csharp_target),
            placeholder_name: view
                .type_is_abstract(target)
                .then(|| format!("__Coflow{csharp_target}Ref")),
            assignable_sources,
        });
    }

    Ok(out)
}

fn ref_index_load_step(ref_index: &CsharpRefIndex) -> String {
    if ref_index.assignable_sources.len() == 1
        && ref_index.assignable_sources[0].table_name == ref_index.target_source_name
    {
        return format!(
            "var {} = {};",
            ref_index.parameter_name, ref_index.assignable_sources[0].index_var
        );
    }

    let source_args = ref_index
        .assignable_sources
        .iter()
        .map(|source| {
            format!(
                "new RefIndexSource<{}, {}>({}, x => x.{}, \"{}\", \"{}\")",
                ref_index.target_id_type,
                ref_index.target_name,
                source.list_var,
                source.id_property,
                source.table_name,
                source.id_source_name
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "var {} = BuildRefIndex({});",
        ref_index.parameter_name, source_args
    )
}

fn loader_methods(view: &SchemaView) -> Result<Vec<CsharpLoader>, CsharpCodegenError> {
    view.non_abstract_type_names()
        .into_iter()
        .map(|type_name| {
            let ty = view.type_meta(&type_name)?;
            let mut used_local_names = loader_reserved_local_names(ty);
            let key_ty = view.key_field_type(&type_name);
            let key_local_name = field_local_name("id", &mut used_local_names)?;
            Ok(CsharpLoader {
                type_name: view.csharp_type_name(&type_name),
                key_type_name: csharp_type(&key_ty, view),
                key_local_name,
                key_read_expr: read_required_expr(
                    "id",
                    "obj",
                    "path",
                    &read_token_expr(&key_ty, "token", "childPath", view)?,
                ),
                key_messagepack_read_expr: read_messagepack_expr(
                    &key_ty,
                    "reader",
                    "fieldPath",
                    view,
                )?,
                fields: ty
                    .all_fields
                    .iter()
                    .map(|field| {
                        let local_name = field_local_name(&field.name, &mut used_local_names)?;
                        let csharp_ty = csharp_field_type(field, view);
                        let default_expr =
                            default_value_expr(field.default.as_ref(), &csharp_ty, view)?;
                        let is_required = default_expr.is_none();
                        Ok(CsharpLoadField {
                            property: pascal_case(&field.name),
                            source_name: field.name.clone(),
                            local_name: local_name.clone(),
                            type_name: csharp_type(&csharp_ty, view),
                            read_expr: read_field_expr(field, "obj", "path", view)?,
                            messagepack_read_expr: read_messagepack_field_expr(
                                field,
                                "reader",
                                "fieldPath",
                                view,
                            )?,
                            default_expr,
                            is_required,
                            assignments: vec![CsharpLoadAssignment {
                                property: pascal_case(&field.name),
                                expr: local_name,
                            }],
                        })
                    })
                    .collect::<Result<Vec<_>, CsharpCodegenError>>()?,
            })
        })
        .collect()
}

fn loader_reserved_local_names(ty: &TypeMeta) -> HashSet<String> {
    let mut out = ty
        .all_fields
        .iter()
        .map(|field| format!("has{}", pascal_case(&field.name)))
        .collect::<HashSet<_>>();
    out.insert("hasId".to_string());
    out
}

fn field_local_name(
    field_name: &str,
    used_names: &mut HashSet<String>,
) -> Result<String, CsharpCodegenError> {
    let candidate = camel_case(field_name);
    let base_name = if csharp_ident_error(&candidate)
        .is_some_and(|reason| reason == "identifier is a C# keyword")
        || is_reserved_loader_local_name(&candidate)
    {
        format!("{candidate}Value")
    } else {
        candidate
    };
    let mut local_name = base_name.clone();
    let mut suffix = 2;
    while used_names.contains(&local_name) {
        local_name = format!("{base_name}{suffix}");
        suffix += 1;
    }

    if let Some(reason) = csharp_ident_error(&local_name) {
        return Err(CsharpCodegenError::new(format!(
            "invalid C# field local variable name `{local_name}`: {reason}"
        )));
    }

    used_names.insert(local_name.clone());
    Ok(local_name)
}

fn is_reserved_loader_local_name(value: &str) -> bool {
    matches!(
        value,
        "bytes"
            | "count"
            | "fieldPath"
            | "i"
            | "index"
            | "item"
            | "itemPath"
            | "itemReader"
            | "key"
            | "keyPath"
            | "list"
            | "path"
            | "rawKey"
            | "reader"
            | "result"
            | "source"
            | "token"
            | "typeKey"
            | "typeName"
            | "value"
            | "valuePath"
            | "valueReader"
    )
}

fn polymorphic_loaders(
    view: &SchemaView,
) -> Result<Vec<CsharpPolymorphicLoader>, CsharpCodegenError> {
    view.polymorphic_type_names()
        .into_iter()
        .map(|type_name| {
            let assignable = view.concrete_assignable_types(&type_name)?;
            Ok(CsharpPolymorphicLoader {
                type_name: view.csharp_type_name(&type_name),
                expected: assignable.join(" | "),
                cases: assignable
                    .into_iter()
                    .map(|type_name| CsharpPolymorphicCase {
                        type_name: view.csharp_type_name(&type_name),
                        source_name: type_name,
                    })
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
    let parameters = resolve_parameters(view, tables, ref_targets);
    let mut table_calls = Vec::new();
    for table_name in tables {
        let table = view.type_meta(table_name)?;
        let csharp_table = view.csharp_type_name(table_name);
        table_calls.push(CsharpResolveTableCall {
            table_name: csharp_table,
            source_name: table_name.clone(),
            list_var: camel_case(&pluralize(table_name)),
            item_var: camel_case(table_name),
            id_property: "Id".to_string(),
            index_args: resolve_index_argument_list(view, ref_targets),
            path_expr: format!("$\"{table_name}[{{{}.Id}}]\"", camel_case(table_name)),
            returns_value: table.is_struct,
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
        type_name: view.csharp_type_name(&ty.name),
        returns_value: ty.is_struct,
        is_polymorphic: false,
        parameters: resolve_index_parameter_models(view, ref_targets),
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
        type_name: view.csharp_type_name(&ty.name),
        returns_value: false,
        is_polymorphic: true,
        parameters: resolve_index_parameter_models(view, ref_targets),
        statements: Vec::new(),
        cases: view
            .concrete_assignable_types(&ty.name)?
            .into_iter()
            .map(|type_name| CsharpResolveCase {
                var_name: camel_case(&type_name),
                type_name: view.csharp_type_name(&type_name),
                index_args: resolve_index_argument_list(view, ref_targets),
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

    if !value_needs_resolve(&field.ty) {
        return Ok(());
    }

    let mut context = ResolveContext::new(view, ref_targets);
    push_resolve_nested_value(
        out,
        &mut context,
        &field.ty,
        &format!("value.{property}"),
        &field.name,
    )
}

fn push_resolve_nested_value(
    out: &mut Vec<String>,
    context: &mut ResolveContext<'_>,
    ty: &FieldType,
    access: &str,
    path_suffix: &str,
) -> Result<(), CsharpCodegenError> {
    match ty {
        FieldType::Type(type_name) => {
            push_resolve_type_value(out, context, type_name, access, path_suffix);
        }
        FieldType::Array(inner) => {
            push_resolve_array_value(out, context, inner, access, path_suffix)?;
        }
        FieldType::Dict(key, value) => {
            push_resolve_dict_value(out, context, key, value, access, path_suffix)?;
        }
        FieldType::Nullable(inner) => {
            push_resolve_nullable_value(out, context, inner, access, path_suffix)?;
        }
        FieldType::Int
        | FieldType::Float
        | FieldType::Bool
        | FieldType::String
        | FieldType::Enum(_) => {}
    }
    Ok(())
}

struct ResolveContext<'a> {
    view: &'a SchemaView,
    ref_targets: &'a [String],
    locals: ResolveLocalNames,
}

impl<'a> ResolveContext<'a> {
    const fn new(view: &'a SchemaView, ref_targets: &'a [String]) -> Self {
        Self {
            view,
            ref_targets,
            locals: ResolveLocalNames {
                lists: 0,
                indexes: 0,
                dictionaries: 0,
                keys: 0,
                nullables: 0,
            },
        }
    }
}

#[derive(Default)]
struct ResolveLocalNames {
    lists: usize,
    indexes: usize,
    dictionaries: usize,
    keys: usize,
    nullables: usize,
}

impl ResolveLocalNames {
    fn list(&mut self) -> String {
        self.lists += 1;
        format!("list{}", self.lists)
    }

    fn index(&mut self) -> String {
        self.indexes += 1;
        format!("i{}", self.indexes)
    }

    fn dictionary(&mut self) -> String {
        self.dictionaries += 1;
        format!("dictionary{}", self.dictionaries)
    }

    fn key(&mut self) -> String {
        self.keys += 1;
        format!("key{}", self.keys)
    }

    fn nullable_value(&mut self) -> String {
        self.nullables += 1;
        format!("nullableValue{}", self.nullables)
    }
}

fn push_resolve_type_value(
    out: &mut Vec<String>,
    context: &ResolveContext<'_>,
    type_name: &str,
    access: &str,
    path_suffix: &str,
) {
    let args = resolve_index_argument_list(context.view, context.ref_targets);
    let csharp_name = context.view.csharp_type_name(type_name);
    let target_index = ref_index_param_name(&csharp_name);
    let target_id_type = csharp_type(&context.view.key_field_type(type_name), context.view);
    out.push(format!("if ({access}.{})", ref_marker_property()));
    out.push("{".to_string());
    out.push(format!(
        "    {access} = ResolveRef({target_index}, ({target_id_type}){access}.{}!, $\"{{path}}.{path_suffix}\", \"{type_name}\");",
        ref_key_property()
    ));
    out.push("}".to_string());
    out.push("else".to_string());
    out.push("{".to_string());
    if type_has_nested_resolvable_fields(type_name, context.view) {
        if context.view.type_is_struct(type_name) {
            out.push(format!(
                "    {access} = Resolve{csharp_name}Refs({access}, {args}, $\"{{path}}.{path_suffix}\");"
            ));
        } else {
            out.push(format!(
                "    Resolve{csharp_name}Refs({access}, {args}, $\"{{path}}.{path_suffix}\");"
            ));
        }
    }
    out.push("}".to_string());
}

fn push_resolve_array_value(
    out: &mut Vec<String>,
    context: &mut ResolveContext<'_>,
    inner: &FieldType,
    access: &str,
    path_suffix: &str,
) -> Result<(), CsharpCodegenError> {
    if !value_needs_resolve(inner) {
        return Ok(());
    }
    let list_name = context.locals.list();
    let index_name = context.locals.index();
    out.push("{".to_string());
    out.push(format!(
        "    var {list_name} = (List<{}>){access};",
        csharp_type(inner, context.view)
    ));
    out.push(format!(
        "    for (var {index_name} = 0; {index_name} < {list_name}.Count; {index_name}++)"
    ));
    out.push("    {".to_string());
    let item_access = format!("{list_name}[{index_name}]");
    push_indented_resolve_nested_value(
        out,
        context,
        inner,
        &item_access,
        &format!("{path_suffix}[{{{index_name}}}]"),
        "        ",
    )?;
    out.push("    }".to_string());
    out.push("}".to_string());
    Ok(())
}

fn push_resolve_dict_value(
    out: &mut Vec<String>,
    context: &mut ResolveContext<'_>,
    key: &FieldType,
    value: &FieldType,
    access: &str,
    path_suffix: &str,
) -> Result<(), CsharpCodegenError> {
    if !value_needs_resolve(value) {
        return Ok(());
    }
    let dictionary_name = context.locals.dictionary();
    let key_name = context.locals.key();
    out.push("{".to_string());
    out.push(format!(
        "    var {dictionary_name} = (Dictionary<{}, {}>){access};",
        csharp_type(key, context.view),
        csharp_type(value, context.view)
    ));
    out.push(format!(
        "    foreach (var {key_name} in new List<{}>({dictionary_name}.Keys))",
        csharp_type(key, context.view),
    ));
    out.push("    {".to_string());
    let value_access = format!("{dictionary_name}[{key_name}]");
    push_indented_resolve_nested_value(
        out,
        context,
        value,
        &value_access,
        &format!("{path_suffix}[{{{key_name}}}]"),
        "        ",
    )?;
    out.push("    }".to_string());
    out.push("}".to_string());
    Ok(())
}

fn push_resolve_nullable_value(
    out: &mut Vec<String>,
    context: &mut ResolveContext<'_>,
    inner: &FieldType,
    access: &str,
    path_suffix: &str,
) -> Result<(), CsharpCodegenError> {
    if !value_needs_resolve(inner) {
        return Ok(());
    }
    out.push(format!("if ({access} != null)"));
    out.push("{".to_string());
    let needs_value_copy = nullable_value_needs_copy(inner, context.view);
    let nullable_value = if needs_value_copy {
        Some(context.locals.nullable_value())
    } else {
        None
    };
    if needs_value_copy {
        let local = nullable_value.as_deref().unwrap_or("");
        out.push(format!("    var {local} = {access}.Value;"));
    }
    let nested_access = nullable_value.as_deref().unwrap_or(access);
    push_indented_resolve_nested_value(out, context, inner, nested_access, path_suffix, "    ")?;
    if let Some(local) = nullable_value {
        out.push(format!("    {access} = {local};"));
    }
    out.push("}".to_string());
    Ok(())
}

fn nullable_value_needs_copy(ty: &FieldType, view: &SchemaView) -> bool {
    matches!(
        ty.non_nullable(),
        FieldType::Type(name) if view.type_is_struct(name) && view.range_contains_ref(name)
    )
}

fn push_indented_resolve_nested_value(
    out: &mut Vec<String>,
    context: &mut ResolveContext<'_>,
    ty: &FieldType,
    access: &str,
    path_suffix: &str,
    indent: &str,
) -> Result<(), CsharpCodegenError> {
    let mut inner_statements = Vec::new();
    push_resolve_nested_value(&mut inner_statements, context, ty, access, path_suffix)?;
    out.extend(
        inner_statements
            .into_iter()
            .map(|line| format!("{indent}{line}")),
    );
    Ok(())
}

fn value_needs_resolve(ty: &FieldType) -> bool {
    match ty {
        FieldType::Type(_) => true,
        FieldType::Array(inner) | FieldType::Nullable(inner) => value_needs_resolve(inner),
        FieldType::Dict(_, value) => value_needs_resolve(value),
        FieldType::Int
        | FieldType::Float
        | FieldType::Bool
        | FieldType::String
        | FieldType::Enum(_) => false,
    }
}

fn field_needs_internal_set(ty: &FieldType, view: &SchemaView) -> bool {
    matches!(ty.non_nullable(), FieldType::Type(_)) || value_needs_resolve_writeback(ty, view)
}

fn value_needs_resolve_writeback(ty: &FieldType, view: &SchemaView) -> bool {
    match ty {
        FieldType::Type(name) => {
            view.type_is_struct(name) && type_has_nested_resolvable_fields(name, view)
        }
        FieldType::Array(inner) | FieldType::Nullable(inner) => {
            value_needs_resolve_writeback(inner, view)
        }
        FieldType::Dict(_, value) => value_needs_resolve_writeback(value, view),
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
) -> Vec<CsharpParameter> {
    let mut out = Vec::new();

    for table_name in tables {
        let csharp_table = view.csharp_type_name(table_name);
        out.push(CsharpParameter {
            ty: format!("List<{csharp_table}>"),
            name: camel_case(&pluralize(table_name)),
        });
    }

    for target in ref_targets {
        let csharp_target = view.csharp_type_name(target);
        let id_type = csharp_type(&view.key_field_type(target), view);
        out.push(CsharpParameter {
            ty: format!("Dictionary<{id_type}, {csharp_target}>"),
            name: ref_index_param_name(&csharp_target),
        });
    }

    out
}

fn resolve_index_parameter_models(
    view: &SchemaView,
    ref_targets: &[String],
) -> Vec<CsharpParameter> {
    let mut out = Vec::new();
    for target in ref_targets {
        let csharp_target = view.csharp_type_name(target);
        let id_type = csharp_type(&view.key_field_type(target), view);
        out.push(CsharpParameter {
            ty: format!("Dictionary<{id_type}, {csharp_target}>"),
            name: ref_index_param_name(&csharp_target),
        });
    }
    out
}

fn resolve_arguments(view: &SchemaView, tables: &[String], ref_targets: &[String]) -> Vec<String> {
    tables
        .iter()
        .map(|table| camel_case(&pluralize(table)))
        .chain(
            ref_targets
                .iter()
                .map(|target| ref_index_param_name(&view.csharp_type_name(target))),
        )
        .collect()
}

fn resolve_index_argument_list(view: &SchemaView, ref_targets: &[String]) -> String {
    ref_targets
        .iter()
        .map(|target| ref_index_param_name(&view.csharp_type_name(target)))
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
    let csharp_ty = csharp_field_type(field, view);
    let reader = read_token_expr(csharp_ty.non_nullable(), "token", "childPath", view)?;

    if let Some(default) = default_value_expr(field.default.as_ref(), &csharp_ty, view)? {
        if field.ty.is_nullable() {
            return Ok(format!(
                "ReadNullableWithDefault({obj}, \"{name}\", {path}, {default}, (token, childPath) => {reader})"
            ));
        }
        return Ok(read_with_default_expr(name, obj, path, &default, &reader));
    }

    if field.ty.is_nullable() {
        return Ok(format!(
            "ReadRequiredNullable({obj}, \"{name}\", {path}, (token, childPath) => {reader})"
        ));
    }

    Ok(read_required_expr(name, obj, path, &reader))
}

fn read_required_expr(name: &str, obj: &str, path: &str, reader: &str) -> String {
    format!("ReadRequired({obj}, \"{name}\", {path}, (token, childPath) => {reader})")
}

fn read_with_default_expr(
    name: &str,
    obj: &str,
    path: &str,
    default: &str,
    reader: &str,
) -> String {
    format!("ReadWithDefault({obj}, \"{name}\", {path}, {default}, (token, childPath) => {reader})")
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
        FieldType::Enum(name) if view.enums.contains(name) => Ok(format!(
            "ReadEnum<{}>({token}, {path})",
            view.csharp_enum_name(name)
        )),
        FieldType::Enum(name) => Ok(format!(
            "ReadStringEnum<{}>({token}, {path})",
            view.csharp_enum_name(name)
        )),
        FieldType::Type(name) => {
            let csharp_name = view.csharp_type_name(name);
            let key_ty = csharp_type(&view.key_field_type(name), view);
            let key_reader = read_token_expr(&view.key_field_type(name), token, path, view)?;
            let inline_reader = if view.range_is_polymorphic(name) {
                format!("Load{csharp_name}Polymorphic({token}, {path})")
            } else {
                format!("Load{csharp_name}({token}, {path})")
            };
            Ok(format!(
                "{token}.Type == JTokenType.String ? New{csharp_name}Ref(({key_ty})({key_reader})) : {inline_reader}"
            ))
        }
        FieldType::Array(inner) => Ok(format!(
            "ReadArray({token}, {path}, (item, itemPath) => {})",
            read_token_expr(inner, "item", "itemPath", view)?
        )),
        FieldType::Dict(key, value) => Ok(format!(
            "ReadDict({token}, {path}, (key, keyPath) => {}, (value, valuePath) => {})",
            read_dict_key_expr(key, "key", "keyPath", view)?,
            read_token_expr(value, "value", "valuePath", view)?
        )),
        FieldType::Nullable(inner) => Ok(format!(
            "{token}.Type == JTokenType.Null ? null : {}",
            read_token_expr(inner, token, path, view)?
        )),
    }
}

fn read_dict_key_expr(
    ty: &FieldType,
    key: &str,
    path: &str,
    view: &SchemaView,
) -> Result<String, CsharpCodegenError> {
    match ty.non_nullable() {
        FieldType::String => Ok(key.to_string()),
        FieldType::Int => Ok(format!("ReadIntKey({key}, {path})")),
        FieldType::Enum(name) => Ok(format!(
            "ReadEnumKey<{}>({key}, {path})",
            view.csharp_enum_name(name)
        )),
        _ => Err(CsharpCodegenError::new(
            "dictionary key type must be string, int, or enum",
        )),
    }
}

fn read_messagepack_field_expr(
    field: &FieldMeta,
    reader: &str,
    path: &str,
    view: &SchemaView,
) -> Result<String, CsharpCodegenError> {
    read_messagepack_expr(&csharp_field_type(field, view), reader, path, view)
}

fn read_messagepack_expr(
    ty: &FieldType,
    reader: &str,
    path: &str,
    view: &SchemaView,
) -> Result<String, CsharpCodegenError> {
    match ty {
        FieldType::Int => Ok(format!("ReadInt(ref {reader}, {path})")),
        FieldType::Float => Ok(format!("ReadFloat(ref {reader}, {path})")),
        FieldType::Bool => Ok(format!("ReadBool(ref {reader}, {path})")),
        FieldType::String => Ok(format!("ReadString(ref {reader}, {path})")),
        FieldType::Enum(name) if view.enums.contains(name) => Ok(format!(
            "ReadEnum<{}>(ref {reader}, {path})",
            view.csharp_enum_name(name)
        )),
        FieldType::Enum(name) => Ok(format!(
            "ReadStringEnum<{}>(ref {reader}, {path})",
            view.csharp_enum_name(name)
        )),
        FieldType::Type(name) => {
            let csharp_name = view.csharp_type_name(name);
            let key_ty = csharp_type(&view.key_field_type(name), view);
            let key_reader = read_messagepack_expr(&view.key_field_type(name), reader, path, view)?;
            let inline_reader = if view.range_is_polymorphic(name) {
                format!("Load{csharp_name}Polymorphic(ref {reader}, {path})")
            } else {
                format!("Load{csharp_name}(ref {reader}, {path})")
            };
            Ok(format!(
                "NextIsString(ref {reader}) ? New{csharp_name}Ref(({key_ty})({key_reader})) : {inline_reader}"
            ))
        }
        FieldType::Array(inner) => Ok(format!(
            "ReadArray(ref {reader}, {path}, static (ref MessagePackReader itemReader, string itemPath) => {})",
            read_messagepack_expr(inner, "itemReader", "itemPath", view)?
        )),
        FieldType::Dict(key, value) => Ok(format!(
            "ReadDict(ref {reader}, {path}, static (key, keyPath) => {}, static (ref MessagePackReader valueReader, string valuePath) => {})",
            read_messagepack_dict_key_expr(key, "key", "keyPath", view)?,
            read_messagepack_expr(value, "valueReader", "valuePath", view)?
        )),
        FieldType::Nullable(inner) => Ok(format!(
            "ReadNil(ref {reader}, {path}) ? null : {}",
            read_messagepack_expr(inner, reader, path, view)?
        )),
    }
}

fn read_messagepack_dict_key_expr(
    ty: &FieldType,
    key: &str,
    path: &str,
    view: &SchemaView,
) -> Result<String, CsharpCodegenError> {
    match ty.non_nullable() {
        FieldType::String => Ok(key.to_string()),
        FieldType::Int => Ok(format!("ReadIntKey({key}, {path})")),
        FieldType::Enum(name) => Ok(format!(
            "ReadEnumKey<{}>({key}, {path})",
            view.csharp_enum_name(name)
        )),
        _ => Err(CsharpCodegenError::new(
            "dictionary key type must be string, int, or enum",
        )),
    }
}

fn csharp_type(ty: &FieldType, view: &SchemaView) -> String {
    match ty {
        FieldType::Int => "long".to_string(),
        FieldType::Float => "double".to_string(),
        FieldType::Bool => "bool".to_string(),
        FieldType::String => "string".to_string(),
        FieldType::Type(name) | FieldType::Enum(name) => view.csharp_named_type(name),
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

fn csharp_field_type(field: &FieldMeta, _view: &SchemaView) -> FieldType {
    field.ty.clone()
}

fn csharp_property_type(ty: &FieldType, view: &SchemaView) -> String {
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

fn type_declaration(schema_type: &CftSchemaType, view: &SchemaView) -> String {
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
        .map(|parent| format!(" : {}", view.csharp_type_name(parent)))
        .unwrap_or_default();

    format!(
        "{prefix} {}{parent}",
        view.csharp_type_name(&schema_type.name)
    )
}

fn default_value_expr(
    default: Option<&CftSchemaDefaultValue>,
    ty: &FieldType,
    view: &SchemaView,
) -> Result<Option<String>, CsharpCodegenError> {
    let Some(default) = default else {
        return Ok(None);
    };
    Ok(Some(match default {
        CftSchemaDefaultValue::Null if ty.is_nullable() && is_csharp_value_type(ty, view) => {
            format!("({})null", csharp_type(ty, view))
        }
        CftSchemaDefaultValue::Null => "null".to_string(),
        CftSchemaDefaultValue::Int(value) => value.to_string(),
        CftSchemaDefaultValue::Float(value) => format_float(*value),
        CftSchemaDefaultValue::Bool(value) => value.to_string(),
        CftSchemaDefaultValue::String(value) => string_default_expr(value, ty, view),
        CftSchemaDefaultValue::Enum {
            enum_name, variant, ..
        } => format!(
            "{}.{}",
            view.csharp_enum_name(enum_name),
            pascal_case(variant)
        ),
        CftSchemaDefaultValue::EmptyArray | CftSchemaDefaultValue::EmptyObject => {
            collection_default_expr(ty.non_nullable(), view)?
        }
    }))
}

fn string_default_expr(value: &str, ty: &FieldType, view: &SchemaView) -> String {
    match ty.non_nullable() {
        FieldType::Enum(name) if !view.enums.contains(name) => {
            let enum_name = view.csharp_enum_name(name);
            let value = escape_csharp_string(value);
            format!("({enum_name})Enum.Parse(typeof({enum_name}), \"{value}\")")
        }
        _ => format!("\"{}\"", escape_csharp_string(value)),
    }
}

fn default_initializer_for_type(ty: &FieldType, view: &SchemaView) -> Option<String> {
    match ty.non_nullable() {
        FieldType::String => Some("\"\"".to_string()),
        FieldType::Type(name) if !view.type_is_struct(name) => Some("null!".to_string()),
        FieldType::Array(_) | FieldType::Dict(_, _) => collection_default_expr(ty, view).ok(),
        FieldType::Int
        | FieldType::Float
        | FieldType::Bool
        | FieldType::Type(_)
        | FieldType::Enum(_)
        | FieldType::Nullable(_) => None,
    }
}

fn type_has_nested_resolvable_fields(type_name: &str, view: &SchemaView) -> bool {
    view.type_meta(type_name).is_ok_and(|ty| {
        ty.all_fields
            .iter()
            .any(|field| value_needs_resolve(&field.ty))
    })
}

fn default_initializer(field: &FieldMeta, ty: &FieldType, view: &SchemaView) -> Option<String> {
    if let Some(default) = &field.default {
        return default_value_expr(Some(default), ty, view).ok().flatten();
    }

    if field.has_default || ty.is_nullable() {
        return None;
    }

    match ty.non_nullable() {
        FieldType::String => Some("\"\"".to_string()),
        FieldType::Type(name) if !view.type_is_struct(name) => Some("null!".to_string()),
        FieldType::Array(_) | FieldType::Dict(_, _) => collection_default_expr(ty, view).ok(),
        FieldType::Int
        | FieldType::Float
        | FieldType::Bool
        | FieldType::Type(_)
        | FieldType::Enum(_)
        | FieldType::Nullable(_) => None,
    }
}

const fn ref_marker_property() -> &'static str {
    "__CoflowIsRef"
}

const fn ref_key_property() -> &'static str {
    "__CoflowRefKey"
}

fn is_csharp_value_type(ty: &FieldType, view: &SchemaView) -> bool {
    match ty.non_nullable() {
        FieldType::Int | FieldType::Float | FieldType::Bool | FieldType::Enum(_) => true,
        FieldType::Type(name) => view.type_is_struct(name),
        FieldType::String
        | FieldType::Array(_)
        | FieldType::Dict(_, _)
        | FieldType::Nullable(_) => false,
    }
}

fn collection_default_expr(
    ty: &FieldType,
    view: &SchemaView,
) -> Result<String, CsharpCodegenError> {
    match ty.non_nullable() {
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
