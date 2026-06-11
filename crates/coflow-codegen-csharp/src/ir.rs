use crate::emit::{build_csharp_database, build_csharp_enum, build_csharp_type};
use crate::model::CsharpProject;
use crate::names::{
    annotation_name_arg, camel_case, csharp_ident_error, csharp_member_ident_error,
    csharp_namespace_error, csharp_type_name, index_param_name, index_var_name,
    multi_index_var_name, pascal_case, pluralize, ref_index_param_name, ref_index_var_name,
    ref_property_name,
};
use crate::schema_view::{FieldMeta, FieldType, SchemaView};
use crate::CsharpCodegenError;
use coflow_cft::CftContainer;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CsharpCodegenOptions {
    pub namespace: String,
    pub database_class: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CsharpDataFormat {
    Json,
    #[serde(rename = "messagepack")]
    MessagePack,
}

impl CsharpCodegenOptions {
    #[must_use]
    pub fn new(namespace: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            database_class: "GameConfig".to_string(),
        }
    }

    #[must_use]
    pub fn with_database_class(mut self, database_class: impl Into<String>) -> Self {
        self.database_class = database_class.into();
        self
    }
}

pub fn build_project(
    schema: &CftContainer,
    options: &CsharpCodegenOptions,
    data_format: CsharpDataFormat,
) -> Result<CsharpProject, CsharpCodegenError> {
    validate_options(options)?;
    validate_schema_names(schema)?;

    let view = SchemaView::new(schema);
    validate_generated_names(&view, options)?;
    validate_ref_id_types(&view)?;

    let enums = schema
        .all_enums()
        .map(build_csharp_enum)
        .collect::<Vec<_>>();

    let types = schema
        .all_types()
        .map(|schema_type| build_csharp_type(schema_type, &view))
        .collect::<Vec<_>>();

    let tables = view.table_names();
    let database = build_csharp_database(&view, &tables, &options.database_class, data_format)?;

    Ok(CsharpProject {
        namespace: options.namespace.clone(),
        database_class: options.database_class.clone(),
        enums,
        types,
        database,
    })
}

fn validate_options(options: &CsharpCodegenOptions) -> Result<(), CsharpCodegenError> {
    if let Some(reason) = csharp_namespace_error(&options.namespace) {
        return Err(CsharpCodegenError::new(format!(
            "invalid C# namespace `{}`: {reason}",
            options.namespace
        )));
    }
    if let Some(reason) = csharp_ident_error(&options.database_class) {
        return Err(CsharpCodegenError::new(format!(
            "invalid C# database class `{}`: {reason}",
            options.database_class
        )));
    }
    if options.database_class == "CftLoadException" {
        return Err(CsharpCodegenError::new(
            "generated C# database file `CftLoadException.cs` collides with reserved load exception file",
        ));
    }
    Ok(())
}

fn validate_schema_names(schema: &CftContainer) -> Result<(), CsharpCodegenError> {
    for schema_enum in schema.all_enums() {
        validate_ident("enum", &schema_enum.name)?;
        for variant in &schema_enum.variants {
            validate_ident("enum variant", &variant.name)?;
        }
    }

    for schema_type in schema.all_types() {
        validate_ident("type", &schema_type.name)?;
        if let Some(parent) = &schema_type.parent {
            validate_ident("parent type", parent)?;
        }
        for field in &schema_type.fields {
            let property_name = pascal_case(&field.name);
            validate_ident("field property", &property_name)?;

            if let Some(target) = annotation_name_arg(&field.annotations, "ref") {
                validate_ident("ref property", &ref_property_name(&field.name, &target))?;
            }
        }
    }

    Ok(())
}

fn validate_generated_names(
    view: &SchemaView,
    options: &CsharpCodegenOptions,
) -> Result<(), CsharpCodegenError> {
    let tables = view.table_names();
    let ref_targets = view.ref_target_names();
    let resolves_refs = !ref_targets.is_empty();

    validate_generated_file_names(view, options)?;
    validate_generated_member_names(view)?;

    for table_name in &tables {
        let list_property = pluralize(table_name);
        validate_member_ident("table list property", &list_property)?;

        let list_var = camel_case(&list_property);
        validate_ident("table list variable", &list_var)?;

        let index_field = index_var_name(table_name);
        validate_member_ident("table index field", &index_field)?;

        let index_param = index_param_name(table_name);
        validate_ident("table index parameter", &index_param)?;

        if resolves_refs {
            let item_var = camel_case(table_name);
            validate_ident("table item variable", &item_var)?;
        }

        let table = view.type_meta(table_name)?;
        for field in table.index_fields() {
            let storage_field = multi_index_var_name(table_name, &field.name);
            validate_member_ident("multi-index storage field", &storage_field)?;

            let parameter_name = storage_field.trim_start_matches('_');
            validate_ident("multi-index parameter", parameter_name)?;
        }
    }

    for target in &ref_targets {
        let ref_index_field = ref_index_var_name(target);
        validate_member_ident("ref index field", &ref_index_field)?;

        let ref_index_arg = ref_index_param_name(target);
        validate_ident("ref index parameter", &ref_index_arg)?;
    }

    for type_name in view.polymorphic_type_names() {
        for case_name in view.concrete_assignable_types(&type_name)? {
            let var_name = camel_case(&case_name);
            validate_ident("polymorphic case variable", &var_name)?;
        }
    }

    Ok(())
}

fn validate_generated_file_names(
    view: &SchemaView,
    options: &CsharpCodegenOptions,
) -> Result<(), CsharpCodegenError> {
    let mut reserved = BTreeSet::new();
    reserved.insert("GameConfig.cs".to_string());
    reserved.insert(format!("{}.cs", options.database_class));
    reserved.insert("CftLoadException.cs".to_string());

    let mut file_sources = BTreeMap::<String, String>::new();
    for enum_name in &view.enums {
        let file_name = format!("{}.cs", csharp_type_name(enum_name));
        insert_generated_file_name(&mut file_sources, &reserved, &file_name, "enum", enum_name)?;
    }
    for type_name in view.all_type_names() {
        let file_name = format!("{}.cs", csharp_type_name(&type_name));
        insert_generated_file_name(&mut file_sources, &reserved, &file_name, "type", &type_name)?;
    }
    Ok(())
}

fn insert_generated_file_name(
    file_sources: &mut BTreeMap<String, String>,
    reserved: &BTreeSet<String>,
    file_name: &str,
    kind: &str,
    source_name: &str,
) -> Result<(), CsharpCodegenError> {
    if reserved.contains(file_name) {
        return Err(CsharpCodegenError::new(format!(
            "generated C# file name `{file_name}` is reserved for {kind} `{source_name}`"
        )));
    }
    if let Some(existing) = file_sources.insert(file_name.to_string(), source_name.to_string()) {
        return Err(CsharpCodegenError::new(format!(
            "generated C# file name `{file_name}` collides between `{existing}` and `{source_name}`"
        )));
    }
    Ok(())
}

fn validate_generated_member_names(view: &SchemaView) -> Result<(), CsharpCodegenError> {
    for ty in view.types.values() {
        let mut members = BTreeMap::<String, String>::new();
        for field in &ty.all_fields {
            let property_name = pascal_case(&field.name);
            insert_generated_member_name(&mut members, &ty.name, &property_name, &field.name)?;
            if let Some(target) = annotation_name_arg(&field.annotations, "ref") {
                let ref_name = ref_property_name(&field.name, &target);
                insert_generated_member_name(
                    &mut members,
                    &ty.name,
                    &ref_name,
                    &format!("{} @ref({target})", field.name),
                )?;
            }
        }
    }
    Ok(())
}

fn insert_generated_member_name(
    members: &mut BTreeMap<String, String>,
    type_name: &str,
    member_name: &str,
    source_name: &str,
) -> Result<(), CsharpCodegenError> {
    if let Some(existing) = members.insert(member_name.to_string(), source_name.to_string()) {
        return Err(CsharpCodegenError::new(format!(
            "generated C# member name `{member_name}` collides in type `{type_name}` between fields `{existing}` and `{source_name}`"
        )));
    }
    Ok(())
}

fn validate_ref_id_types(view: &SchemaView) -> Result<(), CsharpCodegenError> {
    for ty in view.types.values() {
        for field in &ty.all_fields {
            let Some(target) = annotation_name_arg(&field.annotations, "ref") else {
                continue;
            };
            let target_id_type = target_range_id_type(view, &target)?;
            let field_id_type = field.ty.non_nullable();
            if field_id_type != &target_id_type {
                return Err(CsharpCodegenError::new(format!(
                    "@ref({target}) field `{}` id type `{}` does not match target @id type `{}`",
                    field.name,
                    field_type_display(&field.ty),
                    field_type_display(&target_id_type)
                )));
            }
        }
    }
    Ok(())
}

fn target_range_id_type(view: &SchemaView, target: &str) -> Result<FieldType, CsharpCodegenError> {
    let assignable = view.concrete_assignable_types(target)?;
    let type_names = if assignable.is_empty() {
        vec![target.to_string()]
    } else {
        assignable
    };
    let mut id_type = None::<FieldType>;
    for type_name in type_names {
        let ty = view.type_meta(&type_name)?;
        let source_id = inherited_id_field(view, ty)?.ty.non_nullable().clone();
        if let Some(existing) = &id_type {
            if existing != &source_id {
                return Err(CsharpCodegenError::new(format!(
                    "@ref({target}) target range has inconsistent @id type `{}` for `{}` and `{}` for `{}`",
                    field_type_display(existing),
                    target,
                    field_type_display(&source_id),
                    type_name
                )));
            }
        } else {
            id_type = Some(source_id);
        }
    }
    id_type.ok_or_else(|| CsharpCodegenError::new(format!("type `{target}` has no @id field")))
}

fn inherited_id_field<'a>(
    view: &'a SchemaView,
    ty: &'a crate::schema_view::TypeMeta,
) -> Result<&'a FieldMeta, CsharpCodegenError> {
    if let Ok(field) = ty.id_field() {
        return Ok(field);
    }
    view.type_meta(&ty.name)?.id_field()
}

fn field_type_display(ty: &FieldType) -> String {
    match ty {
        FieldType::Int => "int".to_string(),
        FieldType::Float => "float".to_string(),
        FieldType::Bool => "bool".to_string(),
        FieldType::String => "string".to_string(),
        FieldType::Type(name) | FieldType::Enum(name) => name.clone(),
        FieldType::Array(inner) => format!("[{}]", field_type_display(inner)),
        FieldType::Dict(key, value) => {
            format!(
                "{{{}: {}}}",
                field_type_display(key),
                field_type_display(value)
            )
        }
        FieldType::Nullable(inner) => format!("{}?", field_type_display(inner)),
    }
}

fn validate_ident(kind: &str, value: &str) -> Result<(), CsharpCodegenError> {
    if let Some(reason) = csharp_ident_error(value) {
        return Err(CsharpCodegenError::new(format!(
            "invalid C# {kind} name `{value}`: {reason}"
        )));
    }
    Ok(())
}

fn validate_member_ident(kind: &str, value: &str) -> Result<(), CsharpCodegenError> {
    if let Some(reason) = csharp_member_ident_error(value) {
        return Err(CsharpCodegenError::new(format!(
            "invalid C# {kind} name `{value}`: {reason}"
        )));
    }
    Ok(())
}
