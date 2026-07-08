use crate::emit::{build_csharp_database, build_csharp_enum, build_csharp_type};
use crate::model::{CsharpEnum, CsharpEnumVariant, CsharpProject};
use crate::names::{
    camel_case, csharp_ident_error, csharp_member_ident_error, csharp_namespace_error,
    csharp_type_name, has_annotation, index_param_name, pluralize,
};
use crate::schema_context::CsharpSchemaContext;
use crate::CsharpCodegenError;
use coflow_cft::CftContainer;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CsharpCodegenOptions {
    pub namespace: String,
    pub database_class: String,
    pub int_32: bool,
    pub float_32: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CsharpDataFormat {
    Json,
    #[serde(rename = "messagepack")]
    MessagePack,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CsharpIdAsEnumVariant {
    pub name: String,
    pub value: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsharpCodegenDiagnostic {
    pub code: String,
    pub stage: String,
    pub message: String,
}

impl CsharpCodegenOptions {
    #[must_use]
    pub fn new(namespace: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            database_class: "CoflowTables".to_string(),
            int_32: false,
            float_32: false,
        }
    }

    #[must_use]
    pub fn with_database_class(mut self, database_class: impl Into<String>) -> Self {
        self.database_class = database_class.into();
        self
    }

    #[must_use]
    pub const fn with_int_32(mut self, value: bool) -> Self {
        self.int_32 = value;
        self
    }

    #[must_use]
    pub const fn with_float_32(mut self, value: bool) -> Self {
        self.float_32 = value;
        self
    }
}

pub fn build_project(
    schema: &CftContainer,
    options: &CsharpCodegenOptions,
    data_format: CsharpDataFormat,
    id_as_enum_variants: BTreeMap<String, Vec<CsharpIdAsEnumVariant>>,
    non_empty_tables: Option<&BTreeSet<String>>,
) -> Result<CsharpProject, CsharpCodegenError> {
    let view = CsharpSchemaContext::new(schema)
        .with_int_32(options.int_32)
        .with_float_32(options.float_32);
    let diagnostics = preflight_csharp_codegen_with_view(&view, options, &id_as_enum_variants);
    if !diagnostics.is_empty() {
        return Err(CsharpCodegenError::new(
            diagnostics
                .into_iter()
                .map(|diagnostic| diagnostic.message)
                .collect::<Vec<_>>()
                .join("\n"),
        ));
    }
    let id_as_enum_names = view.id_as_enum_names();
    let tables: Vec<String> = view
        .table_names()
        .into_iter()
        .filter(|name| non_empty_tables.is_none_or(|set| set.contains(name)))
        .collect();
    let loadable: BTreeSet<String> = tables.iter().cloned().collect();
    let view = CsharpSchemaContext::new(schema)
        .with_int_32(options.int_32)
        .with_float_32(options.float_32)
        .with_loadable_tables(loadable);

    let mut id_as_enum_variants = build_id_as_enums(&view, &id_as_enum_names, id_as_enum_variants);
    let enums = view
        .cft
        .enums
        .values()
        .map(|schema_enum| {
            id_as_enum_variants
                .remove(&schema_enum.name)
                .unwrap_or_else(|| build_csharp_enum(schema_enum))
        })
        .collect::<Vec<_>>();

    let types = view
        .cft
        .types
        .values()
        .map(|schema_type| build_csharp_type(schema_type, &view))
        .collect::<Result<Vec<_>, _>>()?;

    let database = build_csharp_database(&view, &tables, &options.database_class, data_format)?;
    let singletons = build_csharp_singletons(&view);

    Ok(CsharpProject {
        namespace: options.namespace.clone(),
        database_class: options.database_class.clone(),
        data_format: match data_format {
            CsharpDataFormat::Json => "json".to_string(),
            CsharpDataFormat::MessagePack => "messagepack".to_string(),
        },
        uses_json: data_format == CsharpDataFormat::Json,
        uses_messagepack: data_format == CsharpDataFormat::MessagePack,
        uses_localization: view.uses_localization(),
        int_type: if options.int_32 { "int" } else { "long" },
        float_type: if options.float_32 { "float" } else { "double" },
        enums,
        types,
        database,
        singletons,
    })
}

fn build_csharp_singletons(view: &CsharpSchemaContext) -> Vec<crate::model::CsharpSingleton> {
    view.singleton_type_names()
        .into_iter()
        .map(|name| {
            let csharp_name = view.csharp_type_name(&name);
            crate::model::CsharpSingleton {
                accessor_property: name.clone(),
                source_name: name,
                records_var: format!("{}Singleton", camel_case(&csharp_name)),
                type_name: csharp_name,
            }
        })
        .collect()
}

#[must_use]
pub fn preflight_csharp_codegen(
    schema: &CftContainer,
    options: &CsharpCodegenOptions,
    id_as_enum_variants: &BTreeMap<String, Vec<CsharpIdAsEnumVariant>>,
) -> Vec<CsharpCodegenDiagnostic> {
    let view = CsharpSchemaContext::new(schema)
        .with_int_32(options.int_32)
        .with_float_32(options.float_32);
    preflight_csharp_codegen_with_view(&view, options, id_as_enum_variants)
}

fn preflight_csharp_codegen_with_view(
    view: &CsharpSchemaContext,
    options: &CsharpCodegenOptions,
    id_as_enum_variants: &BTreeMap<String, Vec<CsharpIdAsEnumVariant>>,
) -> Vec<CsharpCodegenDiagnostic> {
    let mut diagnostics = Vec::new();
    validate_options(options, &mut diagnostics);
    validate_schema_names(view, &mut diagnostics);
    let id_as_enum_names = view.id_as_enum_names();
    validate_id_as_enum_variants(&id_as_enum_names, id_as_enum_variants, &mut diagnostics);
    validate_generated_names(view, options, &mut diagnostics);
    diagnostics
}

fn codegen_diagnostic(message: impl Into<String>) -> CsharpCodegenDiagnostic {
    CsharpCodegenDiagnostic {
        code: "CODEGEN-CSHARP-001".to_string(),
        stage: "CODEGEN".to_string(),
        message: message.into(),
    }
}

fn push_codegen_diagnostic(
    diagnostics: &mut Vec<CsharpCodegenDiagnostic>,
    message: impl Into<String>,
) {
    diagnostics.push(codegen_diagnostic(message));
}

fn validate_options(
    options: &CsharpCodegenOptions,
    diagnostics: &mut Vec<CsharpCodegenDiagnostic>,
) {
    if let Some(reason) = csharp_namespace_error(&options.namespace) {
        push_codegen_diagnostic(
            diagnostics,
            format!("invalid C# namespace `{}`: {reason}", options.namespace),
        );
    }
    if let Some(reason) = csharp_ident_error(&options.database_class) {
        push_codegen_diagnostic(
            diagnostics,
            format!(
                "invalid C# database class `{}`: {reason}",
                options.database_class
            ),
        );
    }
}

fn validate_schema_names(
    view: &CsharpSchemaContext,
    diagnostics: &mut Vec<CsharpCodegenDiagnostic>,
) {
    for schema_enum in view.cft_enum_metas() {
        validate_ident("enum", &schema_enum.name, diagnostics);
        validate_ident("enum", &csharp_type_name(&schema_enum.name), diagnostics);
        let mut variants = BTreeMap::<String, String>::new();
        for variant in &schema_enum.all_variants {
            let csharp_variant = csharp_type_name(&variant.name);
            validate_ident("enum variant", &csharp_variant, diagnostics);
            insert_generated_enum_variant_name(
                &mut variants,
                &schema_enum.name,
                &csharp_variant,
                &variant.name,
                diagnostics,
            );
        }
    }

    for schema_type in view.type_metas() {
        validate_ident("type", &schema_type.name, diagnostics);
        validate_ident("type", &csharp_type_name(&schema_type.name), diagnostics);
        if let Some(parent) = &schema_type.parent {
            validate_ident("parent type", parent, diagnostics);
            validate_ident("parent type", &csharp_type_name(parent), diagnostics);
        }
        for field in &schema_type.own_fields {
            let property_name = csharp_type_name(&field.name);
            validate_ident("field property", &property_name, diagnostics);
        }
    }
}

fn validate_generated_names(
    view: &CsharpSchemaContext,
    options: &CsharpCodegenOptions,
    diagnostics: &mut Vec<CsharpCodegenDiagnostic>,
) {
    let tables = view.table_names();
    let ref_targets = view.ref_target_names();

    validate_generated_file_names(view, options, diagnostics);
    validate_generated_member_names(view, diagnostics);

    for table_name in &tables {
        let csharp_table = view.csharp_type_name(table_name);
        let list_property = format!("Tb{csharp_table}");
        validate_member_ident("table accessor property", &list_property, diagnostics);

        let list_var = camel_case(&pluralize(table_name));
        validate_ident("table list variable", &list_var, diagnostics);

        let index_param = index_param_name(&csharp_table);
        validate_ident("table index parameter", &index_param, diagnostics);
    }

    for target in &ref_targets {
        let csharp_target = view.csharp_type_name(target);
        let lookup_method = format!("Get{csharp_target}");
        validate_member_ident("context lookup method", &lookup_method, diagnostics);
    }

    for type_name in view.polymorphic_type_names() {
        if let Ok(case_names) = view.concrete_assignable_types(&type_name) {
            for case_name in case_names {
                let var_name = camel_case(&view.csharp_type_name(&case_name));
                validate_ident("polymorphic case variable", &var_name, diagnostics);
            }
        }
    }
}

fn validate_generated_file_names(
    view: &CsharpSchemaContext,
    options: &CsharpCodegenOptions,
    diagnostics: &mut Vec<CsharpCodegenDiagnostic>,
) {
    let mut reserved = BTreeSet::new();
    reserved.insert(case_insensitive_file_key(&format!(
        "{}.cs",
        options.database_class
    )));

    let mut file_sources = BTreeMap::<String, String>::new();
    for enum_name in view.enum_names() {
        let file_name = format!("{}.cs", view.csharp_enum_name(enum_name));
        insert_generated_file_name(
            &mut file_sources,
            &reserved,
            &file_name,
            "enum",
            enum_name,
            diagnostics,
        );
    }
    for type_name in view.all_type_names() {
        let file_name = format!("{}.cs", view.csharp_type_name(&type_name));
        insert_generated_file_name(
            &mut file_sources,
            &reserved,
            &file_name,
            "type",
            &type_name,
            diagnostics,
        );
    }
}

fn validate_id_as_enum_variants(
    declared: &BTreeSet<String>,
    variants: &BTreeMap<String, Vec<CsharpIdAsEnumVariant>>,
    diagnostics: &mut Vec<CsharpCodegenDiagnostic>,
) {
    for enum_name in variants.keys() {
        if !declared.contains(enum_name) {
            push_codegen_diagnostic(
                diagnostics,
                format!("@idAsEnum variants provided for undeclared enum `{enum_name}`"),
            );
        }
        validate_ident("@idAsEnum enum", enum_name, diagnostics);
        let mut values = BTreeMap::<i64, String>::new();
        for variant in variants.get(enum_name).into_iter().flatten() {
            validate_ident("@idAsEnum enum variant", &variant.name, diagnostics);
            if let Some(existing) = values.insert(variant.value, variant.name.clone()) {
                push_codegen_diagnostic(
                    diagnostics,
                    format!(
                    "@idAsEnum enum `{enum_name}` value `{}` is used by both `{existing}` and `{}`",
                    variant.value, variant.name
                ),
                );
            }
        }
    }
}

fn build_id_as_enums(
    view: &CsharpSchemaContext,
    declared: &BTreeSet<String>,
    mut variants: BTreeMap<String, Vec<CsharpIdAsEnumVariant>>,
) -> BTreeMap<String, CsharpEnum> {
    let mut out = BTreeMap::new();
    for name in declared {
        let is_flags = view
            .cft_enum_meta(name)
            .is_some_and(|schema_enum| has_annotation(&schema_enum.annotations, "flag"));
        let mut enum_variants = Vec::new();
        if is_flags {
            enum_variants.push(CsharpEnumVariant {
                name: "None".to_string(),
                value: 0,
                summary: None,
                obsolete: false,
            });
        }
        for variant in variants.remove(name).unwrap_or_default() {
            enum_variants.push(CsharpEnumVariant {
                name: variant.name,
                value: variant.value,
                summary: None,
                obsolete: false,
            });
        }
        out.insert(
            name.clone(),
            CsharpEnum {
                name: name.clone(),
                is_flags,
                summary: None,
                obsolete: false,
                variants: enum_variants,
            },
        );
    }
    out
}

fn insert_generated_file_name(
    file_sources: &mut BTreeMap<String, String>,
    reserved: &BTreeSet<String>,
    file_name: &str,
    kind: &str,
    source_name: &str,
    diagnostics: &mut Vec<CsharpCodegenDiagnostic>,
) {
    let file_key = case_insensitive_file_key(file_name);
    if reserved.contains(&file_key) {
        push_codegen_diagnostic(
            diagnostics,
            format!("generated C# file name `{file_name}` is reserved for {kind} `{source_name}`"),
        );
        return;
    }
    if let Some(existing) = file_sources.insert(file_key, source_name.to_string()) {
        push_codegen_diagnostic(
            diagnostics,
            format!(
            "generated C# file name `{file_name}` collides between `{existing}` and `{source_name}`"
        ),
        );
    }
}

fn case_insensitive_file_key(file_name: &str) -> String {
    file_name.to_ascii_lowercase()
}

fn validate_generated_member_names(
    view: &CsharpSchemaContext,
    diagnostics: &mut Vec<CsharpCodegenDiagnostic>,
) {
    for ty in view.type_metas() {
        let mut members = BTreeMap::<String, String>::new();
        let Ok(fields) = view.fields(&ty.name) else {
            continue;
        };
        for field in fields {
            let property_name = csharp_type_name(&field.name);
            insert_generated_member_name(
                &mut members,
                &ty.name,
                &property_name,
                &field.name,
                diagnostics,
            );
        }
    }
}

fn insert_generated_member_name(
    members: &mut BTreeMap<String, String>,
    type_name: &str,
    member_name: &str,
    source_name: &str,
    diagnostics: &mut Vec<CsharpCodegenDiagnostic>,
) {
    if let Some(existing) = members.insert(member_name.to_string(), source_name.to_string()) {
        push_codegen_diagnostic(diagnostics, format!(
            "generated C# member name `{member_name}` collides in type `{type_name}` between fields `{existing}` and `{source_name}`"
        ));
    }
}

fn insert_generated_enum_variant_name(
    variants: &mut BTreeMap<String, String>,
    enum_name: &str,
    variant_name: &str,
    source_name: &str,
    diagnostics: &mut Vec<CsharpCodegenDiagnostic>,
) {
    if let Some(existing) = variants.insert(variant_name.to_string(), source_name.to_string()) {
        push_codegen_diagnostic(diagnostics, format!(
            "generated C# enum variant name `{variant_name}` collides in enum `{enum_name}` between variants `{existing}` and `{source_name}`"
        ));
    }
}

fn validate_ident(kind: &str, value: &str, diagnostics: &mut Vec<CsharpCodegenDiagnostic>) {
    if let Some(reason) = csharp_ident_error(value) {
        push_codegen_diagnostic(
            diagnostics,
            format!("invalid C# {kind} name `{value}`: {reason}"),
        );
    }
}

fn validate_member_ident(kind: &str, value: &str, diagnostics: &mut Vec<CsharpCodegenDiagnostic>) {
    if let Some(reason) = csharp_member_ident_error(value) {
        push_codegen_diagnostic(
            diagnostics,
            format!("invalid C# {kind} name `{value}`: {reason}"),
        );
    }
}
