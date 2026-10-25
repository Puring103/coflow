use super::LoadedValueDraft;
use crate::diagnostics::RecordOrigin;
use coflow_language::cft::{DimensionName, FieldName, RecordId, TypeName, VariantName};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub struct DimensionValueDraft {
    pub source_type: TypeName,
    pub source_id: RecordId,
    pub field: FieldName,
    pub dimension: DimensionName,
    pub variant: VariantName,
    pub value: LoadedValueDraft,
    pub origin: RecordOrigin,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoadedRecordDraft {
    pub id: String,
    pub actual_type: String,
    pub fields: BTreeMap<String, LoadedValueDraft>,
    /// Where this top-level record originated. Loaders set this when parsing;
    /// synthetic records leave it as [`RecordOrigin::None`].
    pub origin: RecordOrigin,
}

impl LoadedRecordDraft {
    /// 比较源值与记录身份；来源位置变化由宿主单独刷新。
    #[must_use]
    pub fn same_values(&self, other: &Self) -> bool {
        self.id == other.id
            && self.actual_type == other.actual_type
            && super::value::same_fields(&self.fields, &other.fields)
    }

    #[must_use]
    pub fn new(
        id: impl Into<String>,
        actual_type: impl Into<String>,
        fields: impl IntoIterator<Item = (impl Into<String>, LoadedValueDraft)>,
    ) -> Self {
        Self {
            id: id.into(),
            actual_type: actual_type.into(),
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
