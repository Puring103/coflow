use std::collections::{BTreeMap, BTreeSet};

use coflow_cft::{DimensionName, FieldName, TypeName, VariantName};

use crate::RecordCoordinate;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CheckImpact {
    pub(crate) records: BTreeMap<RecordCoordinate, ChangedRecordFields>,
    pub(crate) record_sets: BTreeSet<TypeName>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ChangedRecordFields {
    All,
    Fields(BTreeSet<ChangedField>),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ChangedField {
    pub(crate) field: FieldName,
    pub(crate) projection: ChangedProjection,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ChangedProjection {
    Base,
    Dimension {
        dimension: DimensionName,
        variant: VariantName,
    },
}
