use std::collections::{BTreeMap, BTreeSet};

use crate::ir::CsharpDataFormat;
use crate::model::{
    CsharpContextAssignment, CsharpContextField, CsharpContextLookup, CsharpContextLookupField,
    CsharpDatabase, CsharpParameter, CsharpTable,
};
use crate::names::camel_case;
use crate::schema_view::{FieldType, SchemaView};
use crate::CsharpCodegenError;

use super::identifiers::{context_index_field_name, plural_records_var};
use super::types::csharp_type;

pub fn build_csharp_database(
    view: &SchemaView,
    tables: &[String],
    _database_class: &str,
    data_format: CsharpDataFormat,
) -> Result<CsharpDatabase, CsharpCodegenError> {
    let ordered_tables = match data_format {
        CsharpDataFormat::Json => tables.to_vec(),
        CsharpDataFormat::MessagePack => sort_tables_by_dependencies(view, tables)?,
    };
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
    let load_steps = match data_format {
        CsharpDataFormat::Json => build_json_load_steps(&table_models, load_extension),
        CsharpDataFormat::MessagePack => {
            build_messagepack_load_steps(&table_models, load_extension)
        }
    };

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
            continue;
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

fn build_json_load_steps(table_models: &[CsharpTable], load_extension: &str) -> Vec<String> {
    let mut load_steps = Vec::new();
    for table in table_models {
        load_steps.push(format!(
            "var ({}, {}) = {}.LoadRawTable(Path.Combine(dataDir, \"{}.{}\"));",
            table.records_var, table.raw_rows_var, table.name, table.source_name, load_extension
        ));
    }
    for table in table_models {
        load_steps.push(format!(
            "var {} = {}.BuildIndex({});",
            table.index_var, table.name, table.records_var
        ));
    }
    let context_args = table_models
        .iter()
        .map(|table| table.index_var.clone())
        .collect::<Vec<_>>();
    let context_expr = if context_args.is_empty() {
        "LoadContext.Empty".to_string()
    } else {
        format!("new LoadContext({})", context_args.join(", "))
    };
    load_steps.push(format!("var context = {context_expr};"));
    for table in table_models {
        load_steps.push(format!(
            "{}.HydrateAll({}, {}, context);",
            table.name, table.records_var, table.raw_rows_var
        ));
    }
    load_steps
}

fn build_messagepack_load_steps(table_models: &[CsharpTable], load_extension: &str) -> Vec<String> {
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
        raw_rows_var: format!("{}RawRows", camel_case(&csharp_name)),
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
        FieldType::Ref(name) => {
            let mut hit_table = false;
            for concrete in view.concrete_assignable_types(name)? {
                if table_set.contains(&concrete) {
                    out.insert(concrete.clone());
                    hit_table = true;
                }
            }
            if !hit_table {
                if let Ok(meta) = view.type_meta(name) {
                    for field in &meta.all_fields {
                        collect_table_dependencies_for_field_type(view, &field.ty, table_set, out)?;
                    }
                }
            }
        }
        FieldType::Type(name) => {
            if let Ok(meta) = view.type_meta(name) {
                for field in &meta.all_fields {
                    collect_table_dependencies_for_field_type(view, &field.ty, table_set, out)?;
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
