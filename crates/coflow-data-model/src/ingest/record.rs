use super::LoadedValueDraft;
use crate::diagnostics::RecordOrigin;
use coflow_cft::{DimensionName, FieldName, RecordKey, TypeName, VariantName};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub struct DimensionValueDraft {
    pub source_type: TypeName,
    pub source_key: RecordKey,
    pub field: FieldName,
    pub dimension: DimensionName,
    pub variant: VariantName,
    pub value: LoadedValueDraft,
    pub origin: RecordOrigin,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoadedRecordDraft {
    pub key: String,
    pub actual_type: String,
    pub spreads: Vec<LoadedValueDraft>,
    pub fields: BTreeMap<String, LoadedValueDraft>,
    /// Where this top-level record originated. Loaders set this when parsing;
    /// synthetic records leave it as [`RecordOrigin::None`].
    pub origin: RecordOrigin,
}

impl LoadedRecordDraft {
    #[must_use]
    pub fn new(
        key: impl Into<String>,
        actual_type: impl Into<String>,
        fields: impl IntoIterator<Item = (impl Into<String>, LoadedValueDraft)>,
    ) -> Self {
        Self {
            key: key.into(),
            actual_type: actual_type.into(),
            spreads: Vec::new(),
            fields: fields
                .into_iter()
                .map(|(name, value)| (name.into(), value))
                .collect(),
            origin: RecordOrigin::None,
        }
    }

    #[must_use]
    pub fn with_spreads(
        key: impl Into<String>,
        actual_type: impl Into<String>,
        spreads: impl IntoIterator<Item = LoadedValueDraft>,
        fields: impl IntoIterator<Item = (impl Into<String>, LoadedValueDraft)>,
    ) -> Self {
        Self {
            key: key.into(),
            actual_type: actual_type.into(),
            spreads: spreads.into_iter().collect(),
            fields: fields
                .into_iter()
                .map(|(name, value)| (name.into(), value))
                .collect(),
            origin: RecordOrigin::None,
        }
    }

    #[must_use]
    pub fn with_origin(mut self, origin: RecordOrigin) -> Self {
        self.origin = origin;
        self
    }
}
