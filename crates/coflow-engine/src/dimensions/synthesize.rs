use coflow_api::{ResolvedSource, SourceLocationSpec};
use coflow_cft::{
    CftContainer, CftSchemaField, CftSchemaType, CftSchemaTypeRef, Dimension, DimensionSpec,
    ModuleId, Span,
};
use coflow_project::{DimensionConfig, Project};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DimensionField {
    pub source_type: String,
    pub source_field: String,
    pub bucket: String,
    pub synthesized_type: String,
    pub is_singleton: bool,
}

pub fn inject_language_dimension_types(
    schema: &mut CftContainer,
    config: &DimensionConfig,
) -> Result<Vec<DimensionField>, coflow_cft::CftDiagnostics> {
    let fields = language_dimension_fields(schema);
    for field in &fields {
        let Some(source_type) = schema.resolve_type(&field.source_type) else {
            continue;
        };
        let Some(source_field) = source_type
            .fields
            .iter()
            .find(|candidate| candidate.name == field.source_field)
        else {
            continue;
        };
        let synthesized = synthesized_type(
            &field.synthesized_type,
            &source_field.ty_ref,
            &config.variants,
        );
        schema.register_runtime_type(synthesized)?;
    }
    Ok(fields)
}

pub fn language_dimension_sources(
    project: &Project,
    fields: &[DimensionField],
) -> Vec<ResolvedSource> {
    let Some(config) = project.config.dimensions.get("language") else {
        return Vec::new();
    };
    let Some(out_dir) = config.out_dir.as_ref() else {
        return Vec::new();
    };
    let dir = project.resolve_path(out_dir);
    if !dir.exists() {
        return Vec::new();
    }

    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut entries = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    entries.sort();
    entries
        .into_iter()
        .filter_map(|path| source_for_dimension_file(project, &dir, fields, path))
        .collect()
}

pub fn language_dimension_fields(schema: &CftContainer) -> Vec<DimensionField> {
    let mut fields = Vec::new();
    for schema_type in schema.all_types() {
        for field in &schema_type.fields {
            if !is_language_dimension(field.dimension.as_ref()) {
                continue;
            }
            fields.push(DimensionField {
                source_type: schema_type.name.clone(),
                source_field: field.name.clone(),
                bucket: field
                    .dimension
                    .as_ref()
                    .and_then(|dimension| dimension.bucket.clone())
                    .unwrap_or_else(|| schema_type.name.clone()),
                synthesized_type: format!("{}_{}Variants", schema_type.name, field.name),
                is_singleton: schema_type.is_singleton,
            });
        }
    }
    fields
}

fn is_language_dimension(dimension: Option<&DimensionSpec>) -> bool {
    dimension.is_some_and(|dimension| matches!(dimension.kind, Dimension::Localized))
}

fn synthesized_type(
    name: &str,
    source_ty: &CftSchemaTypeRef,
    variants: &[String],
) -> CftSchemaType {
    let mut fields = Vec::with_capacity(variants.len() + 1);
    fields.push(synthesized_field("default", source_ty));
    fields.extend(
        variants
            .iter()
            .map(|variant| synthesized_field(variant, source_ty)),
    );
    CftSchemaType {
        module: ModuleId::from("__runtime__"),
        name: name.to_string(),
        parent: None,
        is_abstract: false,
        is_sealed: false,
        is_singleton: false,
        fields: fields.clone(),
        all_fields: fields,
        check: None,
        annotations: Vec::new(),
        span: Span::new(0, 0),
    }
}

fn synthesized_field(name: &str, source_ty: &CftSchemaTypeRef) -> CftSchemaField {
    let inner_ty = non_nullable_type(source_ty).clone();
    CftSchemaField {
        name: name.to_string(),
        ty: format!("{}?", format_type_ref(&inner_ty)),
        ty_ref: CftSchemaTypeRef::Nullable(Box::new(inner_ty)),
        has_default: false,
        default: None,
        annotations: Vec::new(),
        dimension: None,
        span: Span::new(0, 0),
    }
}

fn non_nullable_type(ty: &CftSchemaTypeRef) -> &CftSchemaTypeRef {
    match ty {
        CftSchemaTypeRef::Nullable(inner) => non_nullable_type(inner),
        other => other,
    }
}

fn format_type_ref(ty: &CftSchemaTypeRef) -> String {
    match ty {
        CftSchemaTypeRef::Int => "int".to_string(),
        CftSchemaTypeRef::Float => "float".to_string(),
        CftSchemaTypeRef::Bool => "bool".to_string(),
        CftSchemaTypeRef::String => "string".to_string(),
        CftSchemaTypeRef::Named(name) => name.clone(),
        CftSchemaTypeRef::Ref(name) => format!("&{name}"),
        CftSchemaTypeRef::Array(inner) => format!("[{}]", format_type_ref(inner)),
        CftSchemaTypeRef::Dict(key, value) => {
            format!("{{{}: {}}}", format_type_ref(key), format_type_ref(value))
        }
        CftSchemaTypeRef::Nullable(inner) => format!("{}?", format_type_ref(inner)),
    }
}

fn source_for_dimension_file(
    project: &Project,
    dir: &Path,
    fields: &[DimensionField],
    path: PathBuf,
) -> Option<ResolvedSource> {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_string)?;
    let stem = path.file_stem().and_then(|stem| stem.to_str())?;
    let field = field_for_file_stem(fields, stem, &extension)?;
    let provider_id = match extension.as_str() {
        "csv" => "csv",
        "cfd" => "cfd",
        _ => return None,
    };
    let display_name = path.strip_prefix(&project.root_dir).map_or_else(
        |_| path.display().to_string(),
        coflow_project::path_to_slash,
    );
    Some(ResolvedSource {
        provider_id: provider_id.to_string(),
        location: SourceLocationSpec::Path(path),
        options: source_options(field, &extension),
        display_name: if display_name.is_empty() {
            dir.display().to_string()
        } else {
            display_name
        },
    })
}

fn field_for_file_stem<'a>(
    fields: &'a [DimensionField],
    stem: &str,
    extension: &str,
) -> Option<&'a DimensionField> {
    fields.iter().find(|field| {
        if extension == "cfd" && field.is_singleton {
            stem == field.source_type
        } else {
            stem == format!("{}_{}", field.bucket, field.source_field)
        }
    })
}

fn source_options(field: &DimensionField, extension: &str) -> Value {
    if extension == "csv" {
        json!({
            "sheets": [{
                "sheet": format!("{}_{}", field.bucket, field.source_field),
                "type": field.synthesized_type,
            }]
        })
    } else {
        Value::Object(serde_json::Map::new())
    }
}
