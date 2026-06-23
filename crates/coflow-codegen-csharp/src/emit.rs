use crate::ir::CsharpDataFormat;
use crate::model::{
    CsharpConstructorAssignment, CsharpContextAssignment, CsharpContextField, CsharpContextLookup,
    CsharpContextLookupField, CsharpDatabase, CsharpEnum, CsharpEnumVariant, CsharpEquality,
    CsharpLoadField, CsharpLoader, CsharpParameter, CsharpPolymorphicCase, CsharpProperty,
    CsharpTable, CsharpType,
};
use crate::names::{
    camel_case, csharp_ident_error, display_annotation, escape_csharp_string, format_float,
    has_annotation, pascal_case,
};
use crate::schema_view::{FieldMeta, FieldType, SchemaView};
use crate::CsharpCodegenError;
use coflow_cft::{CftSchemaDefaultValue, CftSchemaEnum, CftSchemaType};
use std::collections::{BTreeMap, BTreeSet, HashSet};

pub fn build_csharp_enum(schema_enum: &CftSchemaEnum) -> CsharpEnum {
    CsharpEnum {
        name: csharp_public_type_name(&schema_enum.name),
        is_flags: has_annotation(&schema_enum.annotations, "flag"),
        summary: display_annotation(&schema_enum.annotations),
        obsolete: has_annotation(&schema_enum.annotations, "deprecated"),
        variants: schema_enum
            .variants
            .iter()
            .map(|variant| CsharpEnumVariant {
                name: csharp_public_member_name(&variant.name),
                value: variant.value,
                summary: display_annotation(&variant.annotations),
                obsolete: has_annotation(&variant.annotations, "deprecated"),
            })
            .collect(),
    }
}

pub fn build_csharp_type(
    schema_type: &CftSchemaType,
    view: &SchemaView,
) -> Result<CsharpType, CsharpCodegenError> {
    let is_struct = has_annotation(&schema_type.annotations, "struct");
    let ty = view.type_meta(&schema_type.name)?;
    let mut constructor_parameters = Vec::new();
    let mut base_constructor_args = Vec::new();
    let mut assignments = Vec::new();
    let mut properties = Vec::new();

    if !schema_type.is_abstract {
        let key_ty = view.key_field_type(&schema_type.name);
        let key_type = csharp_type(&key_ty, view);
        constructor_parameters.push(CsharpParameter {
            ty: key_type,
            name: "id".to_string(),
        });
        if has_concrete_parent(&schema_type.name, view) {
            base_constructor_args.push("id".to_string());
        } else {
            properties.push(CsharpProperty {
                visibility: "public".to_string(),
                name: "Id".to_string(),
                type_name: csharp_type(&key_ty, view),
                summary: None,
                obsolete: false,
            });
            assignments.push(CsharpConstructorAssignment {
                property: "Id".to_string(),
                parameter: "id".to_string(),
            });
        }
    }

    let own_field_names = schema_type
        .fields
        .iter()
        .map(|field| field.name.clone())
        .collect::<BTreeSet<_>>();

    for field in &ty.all_fields {
        let local_name = field_local_name(&field.name, &mut HashSet::new())?;
        let property_type = csharp_property_type(&field.ty, view);
        constructor_parameters.push(CsharpParameter {
            ty: property_type,
            name: local_name.clone(),
        });
        if !is_struct && schema_type.parent.is_some() && !own_field_names.contains(&field.name) {
            base_constructor_args.push(local_name);
            continue;
        }

        let property_name = csharp_public_member_name(&field.name);
        properties.push(CsharpProperty {
            visibility: "public".to_string(),
            name: property_name.clone(),
            type_name: csharp_property_type(&field.ty, view),
            summary: display_annotation(&field.annotations),
            obsolete: has_annotation(&field.annotations, "deprecated"),
        });
        assignments.push(CsharpConstructorAssignment {
            property: property_name,
            parameter: local_name,
        });
    }

    let loader = if schema_type.is_abstract {
        Some(polymorphic_loader(&schema_type.name, view)?)
    } else {
        Some(loader_method(&schema_type.name, view)?)
    };

    let equality = (!schema_type.is_abstract).then(|| CsharpEquality {
        key_property: "Id".to_string(),
        is_struct,
    });

    Ok(CsharpType {
        name: view.csharp_type_name(&schema_type.name),
        declaration: type_declaration(schema_type, view),
        constructor_visibility: if schema_type.is_abstract {
            "protected".to_string()
        } else {
            "private".to_string()
        },
        summary: display_annotation(&schema_type.annotations),
        obsolete: has_annotation(&schema_type.annotations, "deprecated"),
        properties,
        constructor_parameters,
        base_constructor_call: (!base_constructor_args.is_empty())
            .then(|| format!(" : base({})", base_constructor_args.join(", "))),
        base_constructor_args,
        assignments,
        loader,
        equality,
    })
}

fn has_concrete_parent(type_name: &str, view: &SchemaView) -> bool {
    let mut parent = view
        .type_meta(type_name)
        .ok()
        .and_then(|ty| ty.parent.as_deref());
    while let Some(parent_name) = parent {
        let Ok(parent_ty) = view.type_meta(parent_name) else {
            return false;
        };
        if !parent_ty.is_abstract {
            return true;
        }
        parent = parent_ty.parent.as_deref();
    }
    false
}

pub fn build_csharp_database(
    view: &SchemaView,
    tables: &[String],
    _database_class: &str,
    data_format: CsharpDataFormat,
) -> Result<CsharpDatabase, CsharpCodegenError> {
    let ordered_tables = sort_tables_by_dependencies(view, tables)?;
    let table_models = ordered_tables
        .iter()
        .map(|table_name| build_table_model(view, table_name))
        .collect::<Vec<_>>();
    let load_extension = match data_format {
        CsharpDataFormat::Json => "json",
        CsharpDataFormat::MessagePack => "msgpack",
    };

    let context_fields = table_models
        .iter()
        .map(|table| CsharpContextField {
            source_name: table.source_name.clone(),
            field_name: context_index_field_name(&table.name),
            id_type: table.id_type.clone(),
            type_name: table.name.clone(),
        })
        .collect::<Vec<_>>();
    let context_constructor_parameters = table_models
        .iter()
        .map(|table| CsharpParameter {
            ty: format!("Dictionary<{}, {}>?", table.id_type, table.name),
            name: table.index_var.clone(),
        })
        .collect::<Vec<_>>();
    let context_assignments = table_models
        .iter()
        .map(|table| CsharpContextAssignment {
            field_name: context_index_field_name(&table.name),
            parameter_name: table.index_var.clone(),
        })
        .collect::<Vec<_>>();
    let constructor_parameters = table_models
        .iter()
        .map(|table| CsharpParameter {
            ty: format!("Table<{}, {}>", table.id_type, table.name),
            name: table.accessor_parameter.clone(),
        })
        .collect::<Vec<_>>();
    let constructor_args = table_models
        .iter()
        .map(|table| {
            format!(
                "new Table<{}, {}>({}, {})",
                table.id_type, table.name, table.records_var, table.index_var
            )
        })
        .collect::<Vec<_>>();

    let context_lookups = build_context_lookups(view, tables)?;
    let load_steps = build_load_steps(&table_models, load_extension);

    Ok(CsharpDatabase {
        tables: table_models,
        constructor_parameters,
        load_steps,
        constructor_args,
        context_fields,
        context_lookups,
        context_constructor_parameters,
        context_assignments,
    })
}

fn build_context_lookups(
    view: &SchemaView,
    tables: &[String],
) -> Result<Vec<CsharpContextLookup>, CsharpCodegenError> {
    let mut context_lookups = Vec::new();
    for target in view.ref_target_names() {
        let assignable = view
            .concrete_assignable_types(&target)?
            .into_iter()
            .filter(|type_name| tables.contains(type_name))
            .collect::<Vec<_>>();
        if assignable.is_empty() {
            return Err(CsharpCodegenError::new(format!(
                "reference target `{target}` has no loadable key table"
            )));
        }
        let csharp_target = view.csharp_type_name(&target);
        context_lookups.push(CsharpContextLookup {
            method_name: format!("Get{csharp_target}"),
            id_type: csharp_type(&view.key_field_type(&target), view),
            return_type: csharp_target,
            fields: assignable
                .into_iter()
                .map(|type_name| {
                    let csharp_name = view.csharp_type_name(&type_name);
                    CsharpContextLookupField {
                        field_name: context_index_field_name(&csharp_name),
                        value_name: format!("{}Value", camel_case(&csharp_name)),
                    }
                })
                .collect(),
        });
    }
    Ok(context_lookups)
}

fn build_load_steps(table_models: &[CsharpTable], load_extension: &str) -> Vec<String> {
    let mut load_steps = Vec::new();
    for (idx, table) in table_models.iter().enumerate() {
        let context_args = table_models
            .iter()
            .take(idx)
            .map(|candidate| candidate.index_var.clone())
            .collect::<Vec<_>>();
        let context_expr = if context_args.is_empty() {
            "LoadContext.Empty".to_string()
        } else {
            format!("new LoadContext({})", context_args.join(", "))
        };
        load_steps.push(format!(
            "var {} = {}.LoadTable(Path.Combine(dataDir, \"{}.{}\"), {});",
            table.records_var, table.name, table.source_name, load_extension, context_expr
        ));
        load_steps.push(format!(
            "var {} = {}.BuildIndex({});",
            table.index_var, table.name, table.records_var
        ));
    }
    load_steps
}

fn build_table_model(view: &SchemaView, table_name: &str) -> CsharpTable {
    let csharp_name = view.csharp_type_name(table_name);
    let id_ty = view.key_field_type(table_name);
    CsharpTable {
        name: csharp_name.clone(),
        source_name: table_name.to_string(),
        accessor_property: format!("Tb{csharp_name}"),
        accessor_parameter: format!("tb{csharp_name}"),
        records_var: plural_records_var(table_name),
        id_type: csharp_type(&id_ty, view),
        id_property: "Id".to_string(),
        id_source_name: "id".to_string(),
        index_var: format!("{}Index", camel_case(&csharp_name)),
    }
}

fn sort_tables_by_dependencies(
    view: &SchemaView,
    tables: &[String],
) -> Result<Vec<String>, CsharpCodegenError> {
    let table_set = tables.iter().cloned().collect::<BTreeSet<_>>();
    let mut deps = BTreeMap::<String, BTreeSet<String>>::new();
    for table in tables {
        let mut table_deps = BTreeSet::new();
        collect_table_dependencies(view, table, &table_set, &mut table_deps)?;
        deps.insert(table.clone(), table_deps);
    }

    let mut ordered = Vec::new();
    let mut temporary = BTreeSet::new();
    let mut permanent = BTreeSet::new();
    let mut stack = Vec::new();

    for table in tables {
        visit_table(
            table,
            &deps,
            &mut temporary,
            &mut permanent,
            &mut stack,
            &mut ordered,
        )?;
    }

    Ok(ordered)
}

fn visit_table(
    table: &str,
    deps: &BTreeMap<String, BTreeSet<String>>,
    temporary: &mut BTreeSet<String>,
    permanent: &mut BTreeSet<String>,
    stack: &mut Vec<String>,
    ordered: &mut Vec<String>,
) -> Result<(), CsharpCodegenError> {
    if permanent.contains(table) {
        return Ok(());
    }
    if temporary.contains(table) {
        let start = stack
            .iter()
            .position(|entry| entry == table)
            .unwrap_or_default();
        let mut cycle = stack[start..].to_vec();
        cycle.push(table.to_string());
        return Err(CsharpCodegenError::new(format!(
            "C# read-only immediate reference loading does not support cyclic table references: {}",
            cycle.join(" -> ")
        )));
    }

    temporary.insert(table.to_string());
    stack.push(table.to_string());
    for dep in deps.get(table).into_iter().flatten() {
        visit_table(dep, deps, temporary, permanent, stack, ordered)?;
    }
    stack.pop();
    temporary.remove(table);
    permanent.insert(table.to_string());
    ordered.push(table.to_string());
    Ok(())
}

fn collect_table_dependencies(
    view: &SchemaView,
    type_name: &str,
    table_set: &BTreeSet<String>,
    out: &mut BTreeSet<String>,
) -> Result<(), CsharpCodegenError> {
    let ty = view.type_meta(type_name)?;
    for field in &ty.all_fields {
        collect_table_dependencies_for_field_type(view, &field.ty, table_set, out)?;
    }
    Ok(())
}

fn collect_table_dependencies_for_field_type(
    view: &SchemaView,
    ty: &FieldType,
    table_set: &BTreeSet<String>,
    out: &mut BTreeSet<String>,
) -> Result<(), CsharpCodegenError> {
    match ty {
        FieldType::Type(name) => {
            for concrete in view.concrete_assignable_types(name)? {
                if table_set.contains(&concrete) {
                    out.insert(concrete.clone());
                }
            }
        }
        FieldType::Array(inner) | FieldType::Nullable(inner) => {
            collect_table_dependencies_for_field_type(view, inner, table_set, out)?;
        }
        FieldType::Dict(_, value) => {
            collect_table_dependencies_for_field_type(view, value, table_set, out)?;
        }
        FieldType::Int
        | FieldType::Float
        | FieldType::Bool
        | FieldType::String
        | FieldType::Enum(_) => {}
    }
    Ok(())
}

fn loader_method(type_name: &str, view: &SchemaView) -> Result<CsharpLoader, CsharpCodegenError> {
    let ty = view.type_meta(type_name)?;
    let mut used_local_names = loader_reserved_local_names(ty);
    let key_ty = view.key_field_type(type_name);
    let key_local_name = field_local_name("id", &mut used_local_names)?;
    Ok(CsharpLoader {
        type_name: view.csharp_type_name(type_name),
        source_name: type_name.to_string(),
        key_type_name: csharp_type(&key_ty, view),
        key_local_name,
        key_property: "Id".to_string(),
        key_read_expr: read_required_expr(
            "id",
            "obj",
            &read_token_expr(&key_ty, "token", "context", view)?,
        ),
        key_messagepack_read_expr: read_messagepack_expr(&key_ty, "reader", "context", view)?,
        fields: ty
            .all_fields
            .iter()
            .map(|field| load_field(field, &mut used_local_names, view))
            .collect::<Result<Vec<_>, _>>()?,
        polymorphic_cases: Vec::new(),
        is_polymorphic: false,
        expected: String::new(),
    })
}

fn polymorphic_loader(
    type_name: &str,
    view: &SchemaView,
) -> Result<CsharpLoader, CsharpCodegenError> {
    let assignable = view.concrete_assignable_types(type_name)?;
    Ok(CsharpLoader {
        type_name: view.csharp_type_name(type_name),
        source_name: type_name.to_string(),
        key_type_name: csharp_type(&view.key_field_type(type_name), view),
        key_local_name: String::new(),
        key_property: "Id".to_string(),
        key_read_expr: String::new(),
        key_messagepack_read_expr: String::new(),
        fields: Vec::new(),
        polymorphic_cases: assignable
            .iter()
            .map(|case| CsharpPolymorphicCase {
                type_name: view.csharp_type_name(case),
                source_name: case.clone(),
            })
            .collect(),
        is_polymorphic: true,
        expected: assignable.join(" | "),
    })
}

fn load_field(
    field: &FieldMeta,
    used_local_names: &mut HashSet<String>,
    view: &SchemaView,
) -> Result<CsharpLoadField, CsharpCodegenError> {
    let local_name = field_local_name(&field.name, used_local_names)?;
    let default_expr = default_value_expr(field.default.as_ref(), &field.ty, view)?;
    let missing_expr = default_expr.as_ref().map_or_else(
        || {
            if field.ty.is_nullable() {
                None
            } else {
                collection_default_expr(field.ty.non_nullable(), view).ok()
            }
        },
        |default| Some(default.clone()),
    );
    Ok(CsharpLoadField {
        property: csharp_public_member_name(&field.name),
        source_name: field.name.clone(),
        local_name,
        type_name: csharp_type(&field.ty, view),
        read_expr: read_field_expr(field, "obj", "context", view, missing_expr.as_deref())?,
        messagepack_read_expr: read_messagepack_field_expr(field, "reader", "context", view)?,
        is_required: missing_expr.is_none(),
        default_expr,
        missing_expr,
        has_name: format!("has{}", csharp_public_member_name(&field.name)),
    })
}

fn loader_reserved_local_names(ty: &crate::schema_view::TypeMeta) -> HashSet<String> {
    let mut out = ty
        .all_fields
        .iter()
        .map(|field| format!("has{}", csharp_public_member_name(&field.name)))
        .collect::<HashSet<_>>();
    out.insert("hasId".to_string());
    out
}

fn field_local_name(
    field_name: &str,
    used_names: &mut HashSet<String>,
) -> Result<String, CsharpCodegenError> {
    let candidate = camel_case(&pascal_case(field_name));
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
        "count"
            | "fieldPath"
            | "i"
            | "index"
            | "item"
            | "key"
            | "keyPath"
            | "obj"
            | "reader"
            | "result"
            | "token"
            | "typeName"
            | "value"
            | "valuePath"
    )
}

fn read_field_expr(
    field: &FieldMeta,
    obj: &str,
    context: &str,
    view: &SchemaView,
    missing_expr: Option<&str>,
) -> Result<String, CsharpCodegenError> {
    let name = &field.name;
    let reader = read_token_expr(field.ty.non_nullable(), "token", context, view)?;
    if field.ty.is_nullable() {
        return Ok(format!(
            "CoflowJson.ReadNullable({obj}, \"{name}\", (token) => {reader})"
        ));
    }
    if let Some(missing_expr) = missing_expr {
        return Ok(format!(
            "CoflowJson.ReadOptional({obj}, \"{name}\", (token) => {reader}, {missing_expr})"
        ));
    }
    Ok(read_required_expr(name, obj, &reader))
}

fn read_required_expr(name: &str, obj: &str, reader: &str) -> String {
    format!("CoflowJson.ReadRequired({obj}, \"{name}\", (token) => {reader})")
}

fn read_token_expr(
    ty: &FieldType,
    token: &str,
    context: &str,
    view: &SchemaView,
) -> Result<String, CsharpCodegenError> {
    match ty {
        FieldType::Int => Ok(format!("CoflowJson.ReadInt({token})")),
        FieldType::Float => Ok(format!("CoflowJson.ReadFloat({token})")),
        FieldType::Bool => Ok(format!("CoflowJson.ReadBool({token})")),
        FieldType::String => Ok(format!("CoflowJson.ReadString({token})")),
        FieldType::Enum(name) if view.is_key_as_enum(name) => Ok(format!(
            "CoflowJson.ReadStringEnum<{}>({token})",
            view.csharp_enum_name(name)
        )),
        FieldType::Enum(name) if view.enums.contains(name) => Ok(format!(
            "CoflowJson.ReadEnum<{}>({token})",
            view.csharp_enum_name(name)
        )),
        FieldType::Enum(name) => Ok(format!(
            "CoflowJson.ReadStringEnum<{}>({token})",
            view.csharp_enum_name(name)
        )),
        FieldType::Type(name) => {
            let csharp_name = view.csharp_type_name(name);
            let key_reader = read_token_expr(&view.key_field_type(name), token, context, view)?;
            let inline_reader = if view.range_is_polymorphic(name) {
                format!("{csharp_name}.LoadPolymorphic({token}, {context})")
            } else {
                format!("{csharp_name}.LoadInline({token}, {context})")
            };
            Ok(format!(
                "{token}.Type == JTokenType.String ? {context}.Get{csharp_name}({key_reader}) : {inline_reader}"
            ))
        }
        FieldType::Array(inner) => Ok(format!(
            "CoflowJson.ReadArray({token}, (item) => {})",
            read_token_expr(inner, "item", context, view)?
        )),
        FieldType::Dict(key, value) => Ok(format!(
            "CoflowJson.ReadDict({token}, (key) => {}, (value) => {})",
            read_dict_key_expr(key, "key", view)?,
            read_token_expr(value, "value", context, view)?
        )),
        FieldType::Nullable(inner) => Ok(format!(
            "{token}.Type == JTokenType.Null ? null : {}",
            read_token_expr(inner, token, context, view)?
        )),
    }
}

fn read_dict_key_expr(
    ty: &FieldType,
    key: &str,
    view: &SchemaView,
) -> Result<String, CsharpCodegenError> {
    match ty.non_nullable() {
        FieldType::String => Ok(key.to_string()),
        FieldType::Int => Ok(format!("CoflowJson.ReadIntKey({key})")),
        FieldType::Enum(name) => Ok(format!(
            "CoflowJson.ReadEnumKey<{}>({key})",
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
    context: &str,
    view: &SchemaView,
) -> Result<String, CsharpCodegenError> {
    read_messagepack_expr(&field.ty, reader, context, view)
}

fn read_messagepack_expr(
    ty: &FieldType,
    reader: &str,
    context: &str,
    view: &SchemaView,
) -> Result<String, CsharpCodegenError> {
    match ty {
        FieldType::Int => Ok(format!("CoflowMessagePack.ReadInt(ref {reader})")),
        FieldType::Float => Ok(format!("CoflowMessagePack.ReadFloat(ref {reader})")),
        FieldType::Bool => Ok(format!("CoflowMessagePack.ReadBool(ref {reader})")),
        FieldType::String => Ok(format!("CoflowMessagePack.ReadString(ref {reader})")),
        FieldType::Enum(name) if view.is_key_as_enum(name) => Ok(format!(
            "CoflowMessagePack.ReadStringEnum<{}>(ref {reader})",
            view.csharp_enum_name(name)
        )),
        FieldType::Enum(name) if view.enums.contains(name) => Ok(format!(
            "CoflowMessagePack.ReadEnum<{}>(ref {reader})",
            view.csharp_enum_name(name)
        )),
        FieldType::Enum(name) => Ok(format!(
            "CoflowMessagePack.ReadStringEnum<{}>(ref {reader})",
            view.csharp_enum_name(name)
        )),
        FieldType::Type(name) => {
            let csharp_name = view.csharp_type_name(name);
            let key_reader = read_messagepack_expr(&view.key_field_type(name), reader, context, view)?;
            let inline_reader = if view.range_is_polymorphic(name) {
                format!("{csharp_name}.LoadPolymorphic(ref {reader}, {context})")
            } else {
                format!("{csharp_name}.LoadInline(ref {reader}, {context})")
            };
            Ok(format!(
                "CoflowMessagePack.NextIsString(ref {reader}) ? {context}.Get{csharp_name}({key_reader}) : {inline_reader}"
            ))
        }
        FieldType::Array(inner) => Ok(format!(
            "CoflowMessagePack.ReadArray(ref {reader}, {context}, static (ref MessagePackReader itemReader, CoflowTables.LoadContext context) => {})",
            read_messagepack_expr(inner, "itemReader", "context", view)?
        )),
        FieldType::Dict(key, value) => Ok(format!(
            "CoflowMessagePack.ReadDict(ref {reader}, {context}, static (key) => {}, static (ref MessagePackReader valueReader, CoflowTables.LoadContext context) => {})",
            read_messagepack_dict_key_expr(key, "key", view)?,
            read_messagepack_expr(value, "valueReader", "context", view)?
        )),
        FieldType::Nullable(inner) => Ok(format!(
            "CoflowMessagePack.ReadNil(ref {reader}) ? null : {}",
            read_messagepack_expr(inner, reader, context, view)?
        )),
    }
}

fn read_messagepack_dict_key_expr(
    ty: &FieldType,
    key: &str,
    view: &SchemaView,
) -> Result<String, CsharpCodegenError> {
    match ty.non_nullable() {
        FieldType::String => Ok(key.to_string()),
        FieldType::Int => Ok(format!("CoflowMessagePack.ReadIntKey({key})")),
        FieldType::Enum(name) => Ok(format!(
            "CoflowMessagePack.ReadEnumKey<{}>({key})",
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
    } else if schema_type.is_sealed || !view.type_has_descendants(&schema_type.name) {
        "public sealed partial class"
    } else {
        "public partial class"
    };

    let mut interfaces = Vec::new();
    if let Some(parent) = schema_type
        .parent
        .as_ref()
        .filter(|_| !has_annotation(&schema_type.annotations, "struct"))
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

fn default_value_expr(
    default: Option<&CftSchemaDefaultValue>,
    ty: &FieldType,
    view: &SchemaView,
) -> Result<Option<String>, CsharpCodegenError> {
    let Some(default) = default else {
        return Ok(None);
    };
    Ok(Some(match default {
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
            csharp_public_member_name(variant)
        ),
        CftSchemaDefaultValue::EmptyArray | CftSchemaDefaultValue::EmptyObject => {
            collection_default_expr(ty.non_nullable(), view)?
        }
    }))
}

fn string_default_expr(value: &str, ty: &FieldType, view: &SchemaView) -> String {
    match ty.non_nullable() {
        FieldType::Enum(name) if view.is_key_as_enum(name) => {
            let enum_name = view.csharp_enum_name(name);
            let value = escape_csharp_string(value);
            format!("({enum_name})Enum.Parse(typeof({enum_name}), \"{value}\")")
        }
        _ => format!("\"{}\"", escape_csharp_string(value)),
    }
}

fn collection_default_expr(
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

fn csharp_public_type_name(name: &str) -> String {
    pascal_case(name)
}

fn csharp_public_member_name(name: &str) -> String {
    pascal_case(name)
}

fn plural_records_var(table_name: &str) -> String {
    let base = camel_case(&pascal_case(table_name));
    if base.ends_with('s') {
        format!("{base}Rows")
    } else {
        format!("{base}s")
    }
}

fn context_index_field_name(type_name: &str) -> String {
    format!("{type_name}Index")
}
