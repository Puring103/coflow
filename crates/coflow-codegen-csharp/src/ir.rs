use crate::emit::{build_csharp_database, build_csharp_enum, build_csharp_type};
use crate::model::{CsharpEnum, CsharpEnumVariant, CsharpProject};
use crate::names::{
    annotation_name_arg, camel_case, csharp_ident_error, csharp_member_ident_error,
    csharp_namespace_error, csharp_type_name, index_param_name, index_var_name,
    multi_index_var_name, pascal_case, pluralize, ref_index_param_name, ref_index_var_name,
    ref_property_name,
};
use crate::schema_view::SchemaView;
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
    key_as_enum_variants: BTreeMap<String, Vec<String>>,
) -> Result<CsharpProject, CsharpCodegenError> {
    validate_options(options)?;
    validate_schema_names(schema)?;
    let key_as_enum_names = key_as_enum_names(schema);
    validate_key_as_enum_variants(&key_as_enum_names, &key_as_enum_variants)?;

    let view = SchemaView::new(schema);
    validate_generated_names(&view, options, &key_as_enum_names)?;

    let mut enums = schema
        .all_enums()
        .map(build_csharp_enum)
        .collect::<Vec<_>>();
    enums.extend(build_key_as_enums(
        &key_as_enum_names,
        key_as_enum_variants,
    )?);

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
    if options
        .database_class
        .eq_ignore_ascii_case("CftLoadException")
    {
        return Err(CsharpCodegenError::new(
            "generated C# database file `CftLoadException.cs` collides with reserved load exception file",
        ));
    }
    Ok(())
}

fn validate_schema_names(schema: &CftContainer) -> Result<(), CsharpCodegenError> {
    for schema_enum in schema.all_enums() {
        validate_ident("enum", &schema_enum.name)?;
        validate_ident("enum", &csharp_type_name(&schema_enum.name))?;
        let mut variants = BTreeMap::<String, String>::new();
        for variant in &schema_enum.variants {
            validate_ident("enum variant", &variant.name)?;
            let csharp_variant = pascal_case(&variant.name);
            validate_ident("enum variant", &csharp_variant)?;
            insert_generated_enum_variant_name(
                &mut variants,
                &schema_enum.name,
                &csharp_variant,
                &variant.name,
            )?;
        }
    }

    for schema_type in schema.all_types() {
        validate_ident("type", &schema_type.name)?;
        validate_ident("type", &csharp_type_name(&schema_type.name))?;
        if let Some(parent) = &schema_type.parent {
            validate_ident("parent type", parent)?;
            validate_ident("parent type", &csharp_type_name(parent))?;
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
    key_as_enum_names: impl IntoIterator<Item = impl AsRef<str>>,
) -> Result<(), CsharpCodegenError> {
    let tables = view.table_names();
    let ref_targets = view.ref_target_names();
    let resolves_refs = !ref_targets.is_empty();

    validate_generated_file_names(view, options, key_as_enum_names)?;
    validate_generated_member_names(view)?;

    for table_name in &tables {
        let csharp_table = view.csharp_type_name(table_name);
        let list_property = pluralize(&csharp_table);
        validate_member_ident("table list property", &list_property)?;

        let list_var = camel_case(&list_property);
        validate_ident("table list variable", &list_var)?;

        let index_field = index_var_name(&csharp_table);
        validate_member_ident("table index field", &index_field)?;

        let index_param = index_param_name(&csharp_table);
        validate_ident("table index parameter", &index_param)?;

        if resolves_refs {
            let item_var = camel_case(table_name);
            validate_ident("table item variable", &item_var)?;
        }

        let table = view.type_meta(table_name)?;
        for field in table.index_fields() {
            let storage_field = multi_index_var_name(&csharp_table, &field.name);
            validate_member_ident("multi-index storage field", &storage_field)?;

            let parameter_name = storage_field.trim_start_matches('_');
            validate_ident("multi-index parameter", parameter_name)?;
        }
    }

    for target in &ref_targets {
        let csharp_target = view.csharp_type_name(target);
        let ref_index_field = ref_index_var_name(&csharp_target);
        validate_member_ident("ref index field", &ref_index_field)?;

        let ref_index_arg = ref_index_param_name(&csharp_target);
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
    key_as_enum_names: impl IntoIterator<Item = impl AsRef<str>>,
) -> Result<(), CsharpCodegenError> {
    let mut reserved = BTreeSet::new();
    reserved.insert(case_insensitive_file_key("GameConfig.cs"));
    reserved.insert(case_insensitive_file_key(&format!(
        "{}.cs",
        options.database_class
    )));
    reserved.insert(case_insensitive_file_key("CftLoadException.cs"));

    let mut file_sources = BTreeMap::<String, String>::new();
    for enum_name in &view.enums {
        let file_name = format!("{}.cs", view.csharp_enum_name(enum_name));
        insert_generated_file_name(&mut file_sources, &reserved, &file_name, "enum", enum_name)?;
    }
    for enum_name in key_as_enum_names {
        let enum_name = enum_name.as_ref();
        let file_name = format!("{}.cs", csharp_type_name(enum_name));
        insert_generated_file_name(
            &mut file_sources,
            &reserved,
            &file_name,
            "@KeyAsEnum enum",
            enum_name,
        )?;
    }
    for type_name in view.all_type_names() {
        let file_name = format!("{}.cs", view.csharp_type_name(&type_name));
        insert_generated_file_name(&mut file_sources, &reserved, &file_name, "type", &type_name)?;
    }
    Ok(())
}

fn key_as_enum_names(schema: &CftContainer) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for schema_type in schema.all_types() {
        for field in &schema_type.all_fields {
            if let Some(enum_name) =
                crate::names::annotation_string_arg(&field.annotations, "KeyAsEnum")
            {
                out.insert(enum_name);
            }
        }
    }
    out
}

fn validate_key_as_enum_variants(
    declared: &BTreeSet<String>,
    variants: &BTreeMap<String, Vec<String>>,
) -> Result<(), CsharpCodegenError> {
    for enum_name in variants.keys() {
        if !declared.contains(enum_name) {
            return Err(CsharpCodegenError::new(format!(
                "@KeyAsEnum variants provided for undeclared enum `{enum_name}`"
            )));
        }
        validate_ident("@KeyAsEnum enum", enum_name)?;
        for variant in variants.get(enum_name).into_iter().flatten() {
            validate_ident("@KeyAsEnum enum variant", variant)?;
        }
    }
    Ok(())
}

fn build_key_as_enums(
    declared: &BTreeSet<String>,
    mut variants: BTreeMap<String, Vec<String>>,
) -> Result<Vec<CsharpEnum>, CsharpCodegenError> {
    let mut out = Vec::new();
    for name in declared {
        let mut enum_variants = Vec::new();
        for (index, variant) in variants
            .remove(name)
            .unwrap_or_default()
            .into_iter()
            .enumerate()
        {
            enum_variants.push(CsharpEnumVariant {
                name: variant,
                value: i64::try_from(index).map_err(|_| {
                    CsharpCodegenError::new(format!(
                        "@KeyAsEnum enum `{name}` has too many variants"
                    ))
                })?,
                summary: None,
                obsolete: false,
            });
        }
        out.push(CsharpEnum {
            name: csharp_type_name(name),
            is_flags: false,
            summary: None,
            obsolete: false,
            variants: enum_variants,
        });
    }
    Ok(out)
}

fn insert_generated_file_name(
    file_sources: &mut BTreeMap<String, String>,
    reserved: &BTreeSet<String>,
    file_name: &str,
    kind: &str,
    source_name: &str,
) -> Result<(), CsharpCodegenError> {
    let file_key = case_insensitive_file_key(file_name);
    if reserved.contains(&file_key) {
        return Err(CsharpCodegenError::new(format!(
            "generated C# file name `{file_name}` is reserved for {kind} `{source_name}`"
        )));
    }
    if let Some(existing) = file_sources.insert(file_key, source_name.to_string()) {
        return Err(CsharpCodegenError::new(format!(
            "generated C# file name `{file_name}` collides between `{existing}` and `{source_name}`"
        )));
    }
    Ok(())
}

fn case_insensitive_file_key(file_name: &str) -> String {
    file_name.to_ascii_lowercase()
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

fn insert_generated_enum_variant_name(
    variants: &mut BTreeMap<String, String>,
    enum_name: &str,
    variant_name: &str,
    source_name: &str,
) -> Result<(), CsharpCodegenError> {
    if let Some(existing) = variants.insert(variant_name.to_string(), source_name.to_string()) {
        return Err(CsharpCodegenError::new(format!(
            "generated C# enum variant name `{variant_name}` collides in enum `{enum_name}` between variants `{existing}` and `{source_name}`"
        )));
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
