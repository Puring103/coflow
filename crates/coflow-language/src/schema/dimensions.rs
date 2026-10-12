use crate::{CftDimension, CftType, DimensionName, TypeName};
use std::collections::BTreeMap;

// 声明只定义字段所属维度；变体完全由实际选中的 CFD 覆盖块提供。
pub(crate) fn build_dimensions(
    types: &BTreeMap<TypeName, CftType>,
) -> BTreeMap<DimensionName, CftDimension> {
    let mut dimensions = BTreeMap::new();
    for schema_type in types.values() {
        for field in &schema_type.own_fields {
            let Some(binding) = &field.dimension else {
                continue;
            };
            dimensions
                .entry(binding.dimension.clone())
                .or_insert_with(|| CftDimension {
                    name: binding.dimension.clone(),
                    fields: Vec::new(),
                })
                .fields
                .push(field.clone());
        }
    }
    dimensions
}
