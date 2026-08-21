use super::ids::CfdRecordId;
use crate::data_model::diagnostics::CfdPath;
use coflow_language::{DimensionName, FieldName, VariantName};

/// Logical address of a `CfdValue::Ref` instance inside the model: the host
/// record and the `CfdPath` to the ref.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RefSite {
    pub host: CfdRecordId,
    pub path: CfdPath,
    pub dimension: Option<DimensionRefCoordinate>,
}

impl RefSite {
    #[must_use]
    pub const fn new(host: CfdRecordId, path: CfdPath) -> Self {
        Self {
            host,
            path,
            dimension: None,
        }
    }

    #[must_use]
    pub const fn in_dimension(
        host: CfdRecordId,
        path: CfdPath,
        dimension: DimensionRefCoordinate,
    ) -> Self {
        Self {
            host,
            path,
            dimension: Some(dimension),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DimensionRefCoordinate {
    pub field: FieldName,
    pub dimension: DimensionName,
    pub variant: VariantName,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct RefEdgeId(usize);

impl RefEdgeId {
    #[must_use]
    pub(crate) const fn new(index: usize) -> Self {
        Self(index)
    }

    #[must_use]
    pub(crate) fn index(self) -> usize {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefEdge {
    pub site: RefSite,
    pub target: CfdRecordId,
}
