use crate::CftDimension;
#[cfg(feature = "cft-compiler")]
use crate::{CftType, DimensionName, TypeName};
#[cfg(feature = "cft-compiler")]
use std::collections::BTreeMap;

/// VM 编译期句柄标识，不注册为 Schema 类型，也不能在 CFT/CFD 中引用。
#[must_use]
pub(crate) fn dimension_value_marker(
    dimension: &str,
    source_type: &str,
    source_field: &str,
) -> String {
    format!("$dimension::{dimension}::{source_type}::{source_field}")
}

pub(crate) fn dimension_field_for_marker<'a>(
    schema: &'a crate::schema::CftSchema,
    marker: &str,
) -> Option<(&'a CftDimension, &'a crate::schema::CftField)> {
    schema.all_dimensions().find_map(|dimension| {
        dimension
            .fields
            .iter()
            .find(|field| {
                dimension_value_marker(
                    dimension.name.as_str(),
                    field.declaring_type.as_str(),
                    field.name.as_str(),
                ) == marker
            })
            .map(|field| (dimension, field.as_ref()))
    })
}

impl crate::schema::CftSchema {
    pub fn resolve_record_type_name(&self, name: &str) -> Result<String, String> {
        self.resolve_type(name)
            .map(|_| name.to_string())
            .ok_or_else(|| format!("unknown record type `{name}`"))
    }
}

#[cfg(feature = "cft-compiler")]
pub(crate) fn build_dimensions(
    types: &BTreeMap<TypeName, CftType>,
) -> BTreeMap<DimensionName, CftDimension> {
    let mut fields_by_dimension = BTreeMap::new();
    for schema_type in types.values() {
        for field in &schema_type.own_fields {
            let Some(binding) = &field.dimension else {
                continue;
            };
            fields_by_dimension
                .entry(binding.dimension.clone())
                .or_insert_with(Vec::new)
                .push(field.clone());
        }
    }
    fields_by_dimension
        .into_iter()
        .map(|(name, fields)| (name.clone(), CftDimension { name, fields }))
        .collect()
}
