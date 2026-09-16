#[cfg(feature = "cft-compiler")]
use crate::{CftDiagnostic, CftDiagnostics, CftDimension, CftErrorCode, CftType, TypeName};
use crate::{DimensionName, VariantName};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// 维度记录使用完整坐标命名；读写端共享此规则，不拆分名称反推字段。
#[must_use]
pub fn dimension_record_type(dimension: &str, source_type: &str, source_field: &str) -> String {
    // 每个输入分段使用 UTF-8 十六进制编码，命名空间与下划线都不会发生拼接碰撞。
    fn encode(value: &str) -> String {
        value
            .as_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }
    format!(
        "Coflow::dimension::T{}_F{}_D{}",
        encode(source_type),
        encode(source_field),
        encode(dimension)
    )
}

impl crate::schema::CftSchema {
    /// 生成短名只用于 CFD 记录类型位置；与用户短名或其他生成短名冲突时要求限定名。
    pub fn resolve_record_type_name(&self, name: &str) -> Result<String, String> {
        if name.contains("::") {
            return self
                .resolve_type(name)
                .map(|_| name.to_string())
                .ok_or_else(|| format!("unknown record type `{name}`"));
        }
        let mut candidates = BTreeSet::new();
        if self.resolve_type(name).is_some() {
            candidates.insert(name.to_string());
        }
        for dimension in self.all_dimensions() {
            for field in &dimension.fields {
                let owner = field
                    .declaring_type
                    .rsplit("::")
                    .next()
                    .unwrap_or(field.declaring_type.as_str());
                if format!("{owner}_{}_{}", field.name, dimension.name) == name {
                    candidates.insert(dimension_record_type(
                        dimension.name.as_str(),
                        field.declaring_type.as_str(),
                        field.name.as_str(),
                    ));
                }
            }
        }
        if candidates.len() > 1 {
            return Err(format!(
                "ambiguous record type `{name}`; use a qualified name"
            ));
        }
        let candidate = candidates
            .into_iter()
            .next()
            .ok_or_else(|| format!("unknown record type `{name}`"))?;
        Ok(candidate)
    }
}

#[cfg(feature = "cft-compiler")]
pub(crate) fn generated_types(
    types: &mut BTreeMap<TypeName, CftType>,
    dimensions: &BTreeMap<DimensionName, CftDimension>,
) {
    use crate::schema::{CftField, CftValueType};
    use std::sync::Arc;
    let mut generated = Vec::new();
    for dimension in dimensions.values() {
        for field in &dimension.fields {
            let name = TypeName::from_validated(dimension_record_type(
                dimension.name.as_str(),
                field.declaring_type.as_str(),
                field.name.as_str(),
            ));
            let Some(owner) = types.get(&field.declaring_type) else {
                continue;
            };
            let optional = match &field.value_type {
                CftValueType::Option(_) => field.value_type.clone(),
                value => CftValueType::Option(Box::new(value.clone())),
            };
            let fields: Vec<_> = dimension
                .variants
                .iter()
                .map(|variant| {
                    Arc::new(CftField {
                        declaring_type: name.clone(),
                        name: crate::FieldName::from_validated(variant.to_string()),
                        value_type: optional.clone(),
                        default: None,
                        dimension: None,
                        annotations: Vec::new(),
                        display: None,
                        span: field.span,
                    })
                })
                .collect();
            generated.push(CftType {
                kind: coflow_language::cft::syntax::ast::TypeKind::Table,
                module: owner.module.clone(),
                name,
                parent: None,
                is_abstract: false,
                is_sealed: true,
                is_struct: false,
                is_singleton: false,
                is_host: false,
                id_as_enum: None,
                annotations: Vec::new(),
                display: None,
                own_fields: fields.clone(),
                field_by_name: fields
                    .iter()
                    .enumerate()
                    .map(|(i, f)| (f.name.clone(), i))
                    .collect(),
                all_fields: fields,
                check: None,
                span: field.span,
            });
        }
    }
    for ty in generated {
        types.insert(ty.name.clone(), ty);
    }
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

#[cfg(feature = "cft-compiler")]
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
