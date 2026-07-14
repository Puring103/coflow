use crate::{
    format_schema_type_ref, is_cft_identifier, CftAnnotation, CftAnnotationValue, CftDiagnostic,
    CftDiagnostics, CftDimensions, CftErrorCode, CftSchema, CftSchemaField, CftSchemaType,
    CftSchemaTypeRef, ModuleId, Span,
};
use std::collections::BTreeSet;

const RUNTIME_MODULE_ID: &str = "__runtime__";
const STORAGE_PREFIX: &str = "__coflow_dimension";

pub(crate) fn add_dimension_storage(
    schema: CftSchema,
    dimensions: &CftDimensions,
) -> Result<CftSchema, CftDiagnostics> {
    let mut types = Vec::new();
    for source_type in schema.type_metas() {
        for field in &source_type.own_fields {
            let Some(dimension) = &field.dimension else {
                continue;
            };
            let Some(variants) = dimensions.variants(&dimension.dimension) else {
                continue;
            };
            validate_variants(&dimension.dimension, variants)?;
            types.push(storage_type(
                &dimension.dimension,
                &source_type.name,
                &field.name,
                &field.ty_ref,
                variants,
            ));
        }
    }
    schema.with_extension_types(types)
}

fn validate_variants(dimension: &str, variants: &[String]) -> Result<(), CftDiagnostics> {
    let mut seen = BTreeSet::new();
    for variant in variants {
        if variant == "default" || !is_cft_identifier(variant) || !seen.insert(variant) {
            return Err(CftDiagnostics::one(CftDiagnostic::error(
                CftErrorCode::InvalidAnnotationArgument,
                ModuleId::from(RUNTIME_MODULE_ID),
                Span::new(0, 0),
                format!("dimension `{dimension}` has invalid variant `{variant}`"),
            )));
        }
    }
    Ok(())
}

fn storage_type(
    dimension: &str,
    source_type: &str,
    source_field: &str,
    source_ty: &CftSchemaTypeRef,
    variants: &[String],
) -> CftSchemaType {
    let mut fields = Vec::with_capacity(variants.len() + 1);
    fields.push(storage_field("default", source_ty));
    fields.extend(variants.iter().map(|variant| storage_field(variant, source_ty)));
    CftSchemaType {
        module: ModuleId::from(RUNTIME_MODULE_ID),
        name: format!("{STORAGE_PREFIX}_{source_type}_{source_field}"),
        parent: None,
        is_abstract: false,
        is_sealed: false,
        is_singleton: false,
        fields: fields.clone(),
        all_fields: fields,
        check: None,
        annotations: vec![CftAnnotation {
            name: "__coflow_dimension_storage".to_string(),
            args: vec![
                CftAnnotationValue::String(dimension.to_string()),
                CftAnnotationValue::String(source_type.to_string()),
                CftAnnotationValue::String(source_field.to_string()),
            ],
        }],
        span: Span::new(0, 0),
    }
}

fn storage_field(name: &str, source_ty: &CftSchemaTypeRef) -> CftSchemaField {
    let ty_ref = CftSchemaTypeRef::Nullable(Box::new(non_nullable(source_ty).clone()));
    CftSchemaField {
        name: name.to_string(),
        ty: format!("{}?", format_schema_type_ref(&non_nullable(source_ty))),
        ty_ref,
        has_default: false,
        default: None,
        annotations: Vec::new(),
        dimension: None,
        span: Span::new(0, 0),
    }
}

fn non_nullable(ty: &CftSchemaTypeRef) -> &CftSchemaTypeRef {
    match ty {
        CftSchemaTypeRef::Nullable(inner) => non_nullable(inner),
        other => other,
    }
}
