use crate::emit::{build_csharp_enum, build_csharp_type, function_adapter_expression};
use crate::emit::types::csharp_type;
use crate::lowering::CsharpLoweringPlan;
use crate::model::{
    CsharpConstant, CsharpDimension, CsharpEnum, CsharpEnumVariant, CsharpProject,
};
use crate::names::{csharp_ident_error, csharp_namespace_error, csharp_type_name};
use crate::CsharpCodegenError;
use coflow_language::cft::{CftConstValue, CftSchema, CftValueType};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CsharpCodegenOptions {
    pub namespace: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CsharpIdAsEnumVariant {
    pub source_name: String,
    pub name: String,
    pub value: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CsharpCodegenDiagnostic {
    message: String,
}

impl CsharpCodegenOptions {
    #[must_use]
    pub fn new(namespace: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
        }
    }
}

pub fn build_project(
    schema: &CftSchema,
    options: &CsharpCodegenOptions,
    id_as_enum_variants: BTreeMap<String, Vec<CsharpIdAsEnumVariant>>,
    non_empty_tables: Option<&BTreeSet<String>>,
) -> Result<CsharpProject, CsharpCodegenError> {
    let view =
        CsharpLoweringPlan::lower(
            schema,
            &options.namespace,
            non_empty_tables,
        )?;
    let diagnostics = validate_csharp_codegen(&view, options, &id_as_enum_variants);
    if !diagnostics.is_empty() {
        return Err(CsharpCodegenError::from_messages(
            diagnostics.into_iter().map(|diagnostic| diagnostic.message),
        ));
    }
    let mut id_as_enum_variants =
        build_id_as_enums(&view, view.id_as_enum_names(), id_as_enum_variants);
    let enums = view
        .cft_enum_metas()
        .map(|schema_enum| {
            id_as_enum_variants
                .remove(schema_enum.name.as_str())
                .unwrap_or_else(|| build_csharp_enum(schema_enum, &view))
        })
        .collect::<Vec<_>>();

    let mut types = view
        .all_types()
        .map(|schema_type| build_csharp_type(schema_type, &view))
        .collect::<Result<Vec<_>, _>>()?;
    types.sort_by(|left, right| left.name.cmp(&right.name));

    let singletons = build_csharp_singletons(&view);
    let dimensions = view
        .dimensions()
        .map(|dimension| CsharpDimension {
            name: csharp_type_name(dimension.name.as_str()),
            source_name: dimension.name.to_string(),
        })
        .collect();
    let delegate_adapters = build_delegate_adapters(schema, &view);
    let constants = schema
        .all_consts()
        .map(|constant| Ok(CsharpConstant {
            source_name: constant.name.to_string(),
            runtime_type: csharp_type(&constant.value_type, &view),
            value_expression: render_constant_value(
                &constant.value,
                &constant.value_type,
                &view,
            )?,
            deferred: contains_record_reference(&constant.value),
        }))
        .collect::<Result<Vec<_>, CsharpCodegenError>>()?;

    Ok(CsharpProject {
        namespace: options.namespace.clone(),
        dimensions,
        delegate_adapters,
        enums,
        types,
        singletons,
        constants,
    })
}

fn build_delegate_adapters(
    schema: &CftSchema,
    view: &CsharpLoweringPlan<'_>,
) -> Vec<String> {
    fn collect(
        ty: &CftValueType,
        view: &CsharpLoweringPlan<'_>,
        adapters: &mut BTreeMap<String, String>,
    ) {
        match ty {
            CftValueType::Function(parameters, result) => {
                let delegate_type = csharp_type(ty, view);
                adapters.entry(delegate_type).or_insert_with(||
                    function_adapter_expression(parameters, result, view));
                for parameter in parameters {
                    collect(&parameter.value_type, view, adapters);
                }
                collect(result, view, adapters);
            }
            CftValueType::Array(item) | CftValueType::Option(item) => collect(item, view, adapters),
            CftValueType::Dict(key, value) | CftValueType::Result(key, value) => {
                collect(key, view, adapters);
                collect(value, view, adapters);
            }
            _ => { }
        }
    }

    let mut adapters = BTreeMap::new();
    for schema_type in schema.all_types() {
        for field in schema_type.own_fields() {
            collect(&field.value_type, view, &mut adapters);
        }
    }
    adapters.into_iter().map(|(delegate_type, adapter)|
        format!("CoflowDelegateAdapter.Register<{delegate_type}>({adapter});"))
        .collect()
}

fn render_constant_value(
    value: &CftConstValue,
    ty: &CftValueType,
    view: &CsharpLoweringPlan<'_>,
) -> Result<String, CsharpCodegenError> {
    Ok(match (value, ty) {
        (CftConstValue::Int(value), CftValueType::Int) => format!("{value}L"),
        (CftConstValue::Float(value), CftValueType::Float) => format!("{value}D"),
        (CftConstValue::Bool(value), CftValueType::Bool) => value.to_string(),
        (CftConstValue::String(value), CftValueType::String) => format!(
            "\"{}\"",
            crate::render::escape_csharp_string(value)
        ),
        (CftConstValue::Enum { value, .. }, CftValueType::Enum(name)) => {
            format!("({}){value}L", view.csharp_enum_ref(name))
        }
        (CftConstValue::OptionNone, CftValueType::Option(inner)) => {
            format!("Option<{}>.None", csharp_type(inner, view))
        }
        (CftConstValue::OptionSome(value), CftValueType::Option(inner)) => format!(
            "Option<{}>.Some({})",
            csharp_type(inner, view),
            render_constant_value(value, inner, view)?
        ),
        (CftConstValue::ResultOk(value), CftValueType::Result(ok, error)) => format!(
            "Result<{}, {}>.Ok({})",
            csharp_type(ok, view),
            csharp_type(error, view),
            render_constant_value(value, ok, view)?
        ),
        (CftConstValue::ResultErr(value), CftValueType::Result(ok, error)) => format!(
            "Result<{}, {}>.Err({})",
            csharp_type(ok, view),
            csharp_type(error, view),
            render_constant_value(value, error, view)?
        ),
        (CftConstValue::Array(values), CftValueType::Array(inner)) => {
            let values = values
                .iter()
                .map(|value| render_constant_value(value, inner, view))
                .collect::<Result<Vec<_>, _>>()?
                .join(", ");
            format!(
                "CoflowConstantValues.List<{}>({values})",
                csharp_type(inner, view)
            )
        }
        (CftConstValue::Dictionary(entries), CftValueType::Dict(key, item)) => {
            let entries = entries
                .iter()
                .map(|(entry_key, value)| {
                    Ok(format!(
                        "new KeyValuePair<{}, {}>({}, {})",
                        csharp_type(key, view),
                        csharp_type(item, view),
                        render_constant_value(entry_key, key, view)?,
                        render_constant_value(value, item, view)?,
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
        (CftConstValue::Object { type_name, fields }, CftValueType::Object(expected))
            if type_name == expected =>
        {
            let schema_type = view.resolve_type(type_name)?;
            let values = fields
                .iter()
                .map(|(name, value)| (name, value))
                .collect::<BTreeMap<_, _>>();
            let mut arguments = Vec::new();
            if !schema_type.is_struct {
                arguments.push("null".to_string());
            }
            if view.is_ref_target_loadable(type_name) {
                arguments.push(match view.key_field_type(type_name) {
                    CftValueType::String => "string.Empty".to_string(),
                    CftValueType::Enum(name) => {
                        format!("default({})", view.csharp_enum_ref(&name))
                    }
                    _ => unreachable!("record keys are string or enum"),
                });
            }
            arguments.extend(view
                .fields(type_name.as_str())?
                .map(|field| {
                    if matches!(field.value_type, CftValueType::Function(_, _)) {
                        return Err(CsharpCodegenError::new(format!(
                            "constant object `{type_name}` cannot contain function field `{}`",
                            field.name
                        )));
                    }
                    let value = values.get(&field.name).ok_or_else(|| {
                        CsharpCodegenError::new(format!(
                            "constant object `{type_name}` is missing field `{}`",
                            field.name
                        ))
                    })?;
                    render_constant_value(value, &field.value_type, view)
                })
                .collect::<Result<Vec<_>, _>>()?);
            let arguments = arguments.join(", ");
            format!("new {}({arguments})", view.csharp_type_ref(type_name))
        }
        (
            CftConstValue::RecordReference { type_name, key },
            CftValueType::RecordRef(expected),
        ) if type_name == expected => format!(
            "context.Resolve<{}>(\"{}\", \"{}\")",
            view.csharp_type_ref(type_name),
            crate::render::escape_csharp_string(type_name.as_str()),
            crate::render::escape_csharp_string(key),
        ),
        _ => {
            return Err(CsharpCodegenError::new(format!(
                "constant value does not match generated type `{ty}`"
            )))
        }
    })
}

fn contains_record_reference(value: &CftConstValue) -> bool {
    match value {
        CftConstValue::RecordReference { .. } => true,
        CftConstValue::OptionSome(value)
        | CftConstValue::ResultOk(value)
        | CftConstValue::ResultErr(value) => contains_record_reference(value),
        CftConstValue::Array(values) => values.iter().any(contains_record_reference),
        CftConstValue::Dictionary(entries) => entries.iter().any(|(key, value)| {
            contains_record_reference(key) || contains_record_reference(value)
        }),
        CftConstValue::Object { fields, .. } => fields
            .iter()
            .any(|(_, value)| contains_record_reference(value)),
        CftConstValue::Int(_)
        | CftConstValue::Float(_)
        | CftConstValue::Bool(_)
        | CftConstValue::String(_)
        | CftConstValue::FormattedString(_)
        | CftConstValue::Function(_)
        | CftConstValue::Enum { .. }
        | CftConstValue::OptionNone => false,
    }
}

fn build_csharp_singletons(view: &CsharpLoweringPlan<'_>) -> Vec<crate::model::CsharpSingleton> {
    view.singleton_type_names()
        .iter()
        .cloned()
        .map(|name| crate::model::CsharpSingleton { source_name: name })
        .collect()
}

fn validate_csharp_codegen(
    view: &CsharpLoweringPlan<'_>,
    options: &CsharpCodegenOptions,
    id_as_enum_variants: &BTreeMap<String, Vec<CsharpIdAsEnumVariant>>,
) -> Vec<CsharpCodegenDiagnostic> {
    let mut diagnostics = Vec::new();
    validate_options(options, &mut diagnostics);
    validate_schema_names(view, &mut diagnostics);
    validate_id_as_enum_variants(
        view,
        view.id_as_enum_names(),
        id_as_enum_variants,
        &mut diagnostics,
    );
    validate_generated_names(view, &mut diagnostics);
    diagnostics
}

fn codegen_diagnostic(message: impl Into<String>) -> CsharpCodegenDiagnostic {
    CsharpCodegenDiagnostic {
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
}

fn validate_schema_names(
    view: &CsharpLoweringPlan<'_>,
    diagnostics: &mut Vec<CsharpCodegenDiagnostic>,
) {
    for dimension in view.dimensions() {
        validate_ident(
            "dimension type",
            &csharp_type_name(dimension.name.as_str()),
            diagnostics,
        );
    }

    for schema_enum in view.cft_enum_metas() {
        validate_ident("enum", &csharp_type_name(&schema_enum.name), diagnostics);
        let mut variants = BTreeMap::<String, String>::new();
        for variant in &schema_enum.variants {
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

    for schema_type in view.all_types() {
        validate_ident("type", &csharp_type_name(&schema_type.name), diagnostics);
        if let Some(parent) = &schema_type.parent {
            validate_ident("parent type", &csharp_type_name(parent), diagnostics);
        }
        for field in schema_type.own_fields() {
            let property_name = csharp_type_name(&field.name);
            validate_ident("field property", &property_name, diagnostics);
        }
    }
}

fn validate_generated_names(
    view: &CsharpLoweringPlan<'_>,
    diagnostics: &mut Vec<CsharpCodegenDiagnostic>,
) {
    let root_names = view
        .all_type_names()
        .chain(view.enum_names())
        .filter(|name| !name.contains("::"))
        .map(|name| (csharp_type_name(name), name.to_string()))
        .collect::<BTreeMap<_, _>>();
    for (generated, source) in &root_names {
        if matches!(generated.as_str(), "CoflowData" | "CoflowGeneratedContract") {
            push_codegen_diagnostic(
                diagnostics,
                format!("generated C# type name `{generated}` from `{source}` is reserved by runtime metadata"),
            );
        }
    }
    let mut dimension_names = BTreeMap::new();
    for dimension in view.dimensions() {
        let generated = csharp_type_name(dimension.name.as_str());
        let source_name = dimension.name.to_string();
        if matches!(generated.as_str(), "CoflowData" | "CoflowGeneratedContract") {
            push_codegen_diagnostic(
                diagnostics,
                format!("generated C# type name `{generated}` from `{source_name}` is reserved by runtime metadata"),
            );
        }
        if let Some(source) = root_names.get(&generated) {
            push_codegen_diagnostic(
                diagnostics,
                format!(
                    "generated C# dimension type `{generated}` collides with `{source}`"
                ),
            );
        }
        if let Some(existing) = dimension_names.insert(generated.clone(), source_name.clone()) {
            push_codegen_diagnostic(
                diagnostics,
                format!(
                    "generated C# dimension type `{generated}` collides between `{existing}` and `{source_name}`"
                ),
            );
        }
    }
    validate_generated_file_names(view, diagnostics);
    validate_generated_member_names(view, diagnostics);
}

fn validate_generated_file_names(
    view: &CsharpLoweringPlan<'_>,
    diagnostics: &mut Vec<CsharpCodegenDiagnostic>,
) {
    let mut reserved = BTreeSet::new();
    reserved.insert(case_insensitive_file_key("Coflow.Metadata.cs"));
    if view.dimensions().next().is_some() {
        reserved.insert(case_insensitive_file_key("Dimensions.cs"));
    }

    let mut file_sources = BTreeMap::<String, String>::new();
    for enum_name in view.enum_names() {
        let file_name = view.csharp_relative_path(enum_name);
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
        let file_name = view.csharp_relative_path(type_name);
        insert_generated_file_name(
            &mut file_sources,
            &reserved,
            &file_name,
            "type",
            type_name,
            diagnostics,
        );
    }
}

fn validate_id_as_enum_variants(
    view: &CsharpLoweringPlan<'_>,
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
        validate_ident("@idAsEnum enum", &view.csharp_enum_name(enum_name), diagnostics);
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
        let mut generated_names = BTreeMap::<String, String>::new();
        for variant in variants.get(enum_name).into_iter().flatten() {
            if let Some(existing) = generated_names.insert(
                variant.name.clone(),
                variant.source_name.clone(),
            ) {
                push_codegen_diagnostic(
                    diagnostics,
                    format!(
                        "@idAsEnum enum `{enum_name}` generates duplicate C# member `{}` from keys `{existing}` and `{}`",
                        variant.name, variant.source_name
                    ),
                );
            }
        }
    }
}

fn build_id_as_enums(
    view: &CsharpLoweringPlan<'_>,
    declared: &BTreeSet<String>,
    mut variants: BTreeMap<String, Vec<CsharpIdAsEnumVariant>>,
) -> BTreeMap<String, CsharpEnum> {
    let mut out = BTreeMap::new();
    for name in declared {
        let is_flags = view
            .cft_enum_meta(name)
            .is_some_and(|schema_enum| schema_enum.is_flag);
        let mut enum_variants = Vec::new();
        if is_flags {
            enum_variants.push(CsharpEnumVariant {
                name: "None".to_string(),
                source_name: "None".to_string(),
                value: 0,
                annotations: Vec::new(),
                summary: None,
                obsolete: false,
            });
        }
        for variant in variants.remove(name).unwrap_or_default() {
            enum_variants.push(CsharpEnumVariant {
                name: variant.name,
                source_name: variant.source_name,
                value: variant.value,
                annotations: Vec::new(),
                summary: None,
                obsolete: false,
            });
        }
        out.insert(
            name.clone(),
            CsharpEnum {
                name: view.csharp_enum_name(name),
                namespace: view.csharp_namespace(name),
                qualified_name: view.csharp_enum_ref(name),
                relative_path: view.csharp_relative_path(name),
                metadata_name: view.metadata_name(name),
                source_name: name.clone(),
                annotations: view
                    .cft_enum_meta(name)
                    .map_or_else(Vec::new, |schema_enum| {
                        crate::emit::csharp_annotations(&schema_enum.annotations)
                    }),
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
    view: &CsharpLoweringPlan<'_>,
    diagnostics: &mut Vec<CsharpCodegenDiagnostic>,
) {
    for ty in view.all_types() {
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
