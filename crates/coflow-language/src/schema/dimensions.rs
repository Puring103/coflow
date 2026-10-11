use crate::{
    CftDiagnostic, CftDiagnostics, CftDimension, CftErrorCode, CftType, DimensionName, TypeName,
    VariantName,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// 维度记录使用完整坐标命名；读写端共享此规则，不拆分名称反推字段。
#[must_use]
pub fn dimension_record_type(dimension: &str, source_type: &str, source_field: &str) -> String {
    format!("__coflow_{dimension}_{source_type}_{source_field}")
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CftDimensionInputs {
    pub(crate) dimensions: BTreeMap<DimensionName, CftDimensionInput>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CftDimensionInput {
    pub variants: Vec<VariantName>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CftDimensionInputError {
    message: String,
}

impl fmt::Display for CftDimensionInputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for CftDimensionInputError {}

impl CftDimensionInputs {
    /// Normalizes dimension names and variants before schema compilation.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid names, empty or duplicate variant lists,
    /// and the reserved `default` variant.
    pub fn try_new(
        entries: impl IntoIterator<Item = (impl Into<String>, Vec<String>)>,
    ) -> Result<Self, CftDimensionInputError> {
        let mut dimensions = BTreeMap::new();
        for (dimension, variants) in entries {
            let dimension = dimension.into();
            let name =
                DimensionName::new(dimension.clone()).map_err(|_| CftDimensionInputError {
                    message: format!("invalid dimension name `{dimension}`"),
                })?;
            if dimensions.contains_key(&name) {
                return Err(CftDimensionInputError {
                    message: format!("duplicate dimension `{name}`"),
                });
            }
            if variants.is_empty() {
                return Err(CftDimensionInputError {
                    message: format!("dimension `{name}` must declare at least one variant"),
                });
            }
            let mut seen = BTreeSet::new();
            let mut typed_variants = Vec::with_capacity(variants.len());
            for variant in variants {
                let typed =
                    VariantName::new(variant.clone()).map_err(|_| CftDimensionInputError {
                        message: format!("dimension `{name}` has invalid variant `{variant}`"),
                    })?;
                if !seen.insert(typed.clone()) {
                    return Err(CftDimensionInputError {
                        message: format!("dimension `{name}` has duplicate variant `{typed}`"),
                    });
                }
                typed_variants.push(typed);
            }
            dimensions.insert(
                name,
                CftDimensionInput {
                    variants: typed_variants,
                },
            );
        }
        Ok(Self { dimensions })
    }

    #[must_use]
    pub fn dimension(&self, name: &str) -> Option<&CftDimensionInput> {
        self.dimensions.get(name)
    }
}

pub(crate) fn build_dimensions(
    types: &BTreeMap<TypeName, CftType>,
    inputs: &CftDimensionInputs,
) -> Result<BTreeMap<DimensionName, CftDimension>, CftDiagnostics> {
    let mut fields_by_dimension = BTreeMap::new();
    let mut record_types = BTreeSet::new();
    for schema_type in types.values() {
        for field in &schema_type.own_fields {
            let Some(binding) = &field.dimension else {
                continue;
            };
            let record_type = dimension_record_type(
                binding.dimension.as_str(),
                field.declaring_type.as_str(),
                field.name.as_str(),
            );
            if !record_types.insert(record_type.clone()) {
                return Err(CftDiagnostics::one(CftDiagnostic::error(
                    CftErrorCode::InvalidAnnotationArgument,
                    schema_type.module.clone(),
                    field.span,
                    format!("dimension record type `{record_type}` maps to multiple fields"),
                )));
            }
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
