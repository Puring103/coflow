use crate::emit::{build_csharp_database, build_csharp_enum, build_csharp_type};
use crate::model::CsharpProject;
use crate::names::{
    annotation_name_arg, camel_case, csharp_ident_error, csharp_member_ident_error,
    csharp_namespace_error, index_param_name, index_var_name, multi_index_var_name, pascal_case,
    pluralize, ref_index_param_name, ref_index_var_name, ref_property_name,
};
use crate::schema_view::SchemaView;
use crate::CsharpCodegenError;
use coflow_cft::CftContainer;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsharpCodegenOptions {
    pub namespace: String,
    pub database_class: String,
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
) -> Result<CsharpProject, CsharpCodegenError> {
    validate_options(options)?;
    validate_schema_names(schema)?;

    let view = SchemaView::new(schema);
    validate_generated_names(&view)?;

    let enums = schema
        .all_enums()
        .map(build_csharp_enum)
        .collect::<Vec<_>>();

    let types = schema
        .all_types()
        .map(|schema_type| build_csharp_type(schema_type, &view))
        .collect::<Vec<_>>();

    let tables = view.table_names();
    let database = build_csharp_database(&view, &tables, &options.database_class)?;

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

fn validate_generated_names(view: &SchemaView) -> Result<(), CsharpCodegenError> {
    let tables = view.table_names();
    let ref_targets = view.ref_target_names();
    let resolves_refs = !ref_targets.is_empty();

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
