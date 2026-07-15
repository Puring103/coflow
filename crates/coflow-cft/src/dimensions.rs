use crate::{
    is_cft_identifier, CftAnnotation, CftAnnotationValue, CftDiagnostic,
    CftDiagnostics, CftDimensions, CftErrorCode, CftField, CftSchema, CftSchemaTypeRef, CftType,
    FieldName, ModuleId, Span, TypeName,
};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

const RUNTIME_MODULE_ID: &str = "__runtime__";
const STORAGE_PREFIX: &str = "__coflow_dimension";

pub(crate) fn add_dimension_storage(
    schema: CftSchema,
    dimensions: &CftDimensions,
) -> Result<CftSchema, CftDiagnostics> {
    let mut types = Vec::new();
    for source_type in schema.all_types() {
        for field in &source_type.own_fields {
            let Some(dimension) = &field.dimension else {
                continue;
            };
            let Some(variants) = dimensions.variants(dimension.dimension.as_str()) else {
                continue;
            };
            validate_variants(&dimension.dimension, variants)?;
            types.push(storage_type(
                &dimension.dimension,
                source_type.name.as_str(),
                field.name.as_str(),
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
) -> CftType {
    let mut fields = Vec::with_capacity(variants.len() + 1);
    let type_name = TypeName::from_validated(format!(
        "{STORAGE_PREFIX}_{source_type}_{source_field}"
    ));
    fields.push(Arc::new(storage_field(&type_name, "default", source_ty)));
    fields.extend(
        variants
            .iter()
            .map(|variant| Arc::new(storage_field(&type_name, variant, source_ty))),
    );
    let field_by_name = fields
        .iter()
        .enumerate()
        .map(|(index, field)| (field.name.clone(), index))
        .collect();
    CftType {
        module: ModuleId::from(RUNTIME_MODULE_ID),
        name: type_name,
        parent: None,
        is_abstract: false,
        is_sealed: false,
        is_singleton: false,
        own_fields: fields.clone(),
        all_fields: fields,
        field_by_name,
        check: None,
        annotations: vec![CftAnnotation {
            name: "__coflow_dimension_storage".to_string(),
            args: vec![
                CftAnnotationValue::String(dimension.to_string()),
                CftAnnotationValue::String(source_type.to_string()),
                CftAnnotationValue::String(source_field.to_string()),
            ],
        }],
        dimension_checks: BTreeMap::new(),
        span: Span::new(0, 0),
    }
}

fn storage_field(owner: &TypeName, name: &str, source_ty: &CftSchemaTypeRef) -> CftField {
    let ty_ref = CftSchemaTypeRef::Nullable(Box::new(non_nullable(source_ty).clone()));
    CftField {
        declaring_type: owner.clone(),
        name: FieldName::from_validated(name.to_string()),
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
