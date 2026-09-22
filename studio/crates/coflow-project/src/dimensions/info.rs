use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-export")]
use ts_rs::TS;

use super::sources::DimensionField;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
pub struct DimensionInfo {
    pub name: String,
    pub display_name: String,
    pub variants: Vec<String>,
    pub fields: Vec<DimensionFieldInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
pub struct DimensionFieldInfo {
    pub source_type: String,
    pub source_field: String,
    pub is_singleton: bool,
}

#[must_use]
pub(crate) fn dimensions_for_model(
    model: &crate::data_model::CfdDataModel,
    fields: &[DimensionField],
) -> Vec<DimensionInfo> {
    let mut variants =
        std::collections::BTreeMap::<String, std::collections::BTreeSet<String>>::new();
    for (_, record) in model.records() {
        for values in record.dimension_fields.values() {
            variants
                .entry(values.dimension.to_string())
                .or_default()
                .extend(values.variants.keys().map(ToString::to_string));
        }
    }
    let mut grouped = std::collections::BTreeMap::<String, Vec<DimensionFieldInfo>>::new();
    for field in fields {
        grouped
            .entry(field.dimension.to_string())
            .or_default()
            .push(DimensionFieldInfo {
                source_type: field.source_type.to_string(),
                source_field: field.source_field.to_string(),
                is_singleton: field.is_singleton,
            });
    }
    grouped
        .into_iter()
        .map(|(name, fields)| DimensionInfo {
            display_name: if name == "language" {
                "本地化".into()
            } else {
                name.clone()
            },
            variants: variants
                .remove(&name)
                .unwrap_or_default()
                .into_iter()
                .collect(),
            name,
            fields,
        })
        .collect()
}
