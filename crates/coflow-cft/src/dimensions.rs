use crate::{
    CftDiagnostic, CftDiagnostics, CftDimension, CftDimensionInputs, CftErrorCode, CftType,
    DimensionName, ModuleId, Span, TypeName, VariantName,
};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn build_dimensions(
    types: &BTreeMap<TypeName, CftType>,
    inputs: &CftDimensionInputs,
) -> Result<BTreeMap<DimensionName, CftDimension>, CftDiagnostics> {
    if !inputs.diagnostics.is_empty() {
        return Err(inputs.diagnostics.clone());
    }

    let mut fields_by_dimension = BTreeMap::new();
    for schema_type in types.values() {
        for field in &schema_type.own_fields {
            let Some(binding) = &field.dimension else {
                continue;
            };
            if inputs.dimension(binding.dimension.as_str()).is_none() {
                return Err(CftDiagnostics::one(CftDiagnostic::error(
                    CftErrorCode::InvalidAnnotationArgument,
                    schema_type.module.clone(),
                    field.span,
                    format!(
                        "field `{}.{}` uses unconfigured dimension `{}`",
                        schema_type.name, field.name, binding.dimension
                    ),
                )));
            }
            fields_by_dimension
                .entry(binding.dimension.clone())
                .or_insert_with(Vec::new)
                .push(field.clone());
        }
    }

    inputs
        .dimensions
        .iter()
        .map(|(name, input)| {
            validate_variants(name, &input.variants)?;
            let variant_by_name = input
                .variants
                .iter()
                .cloned()
                .enumerate()
                .map(|(index, variant)| (variant, index))
                .collect();
            Ok((
                name.clone(),
                CftDimension {
                    name: name.clone(),
                    variants: input.variants.clone(),
                    variant_by_name,
                    fields: fields_by_dimension.remove(name).unwrap_or_default(),
                },
            ))
        })
        .collect()
}

fn validate_variants(
    dimension: &DimensionName,
    variants: &[VariantName],
) -> Result<(), CftDiagnostics> {
    let mut seen = BTreeSet::new();
    for variant in variants {
        if !seen.insert(variant) {
            return Err(CftDiagnostics::one(CftDiagnostic::error(
                CftErrorCode::InvalidAnnotationArgument,
                ModuleId::from("__project__"),
                Span::default(),
                format!("dimension `{dimension}` has duplicate variant `{variant}`"),
            )));
        }
    }
    Ok(())
}
