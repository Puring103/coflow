use super::ids::{CfdDomainId, CfdRecordId, CfdTypeId};
use crate::diagnostics::{CfdPath, CfdPathSegment};
use coflow_cft::{DimensionName, FieldName, RecordKey, VariantName};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RefEdgeId(usize);

impl RefEdgeId {
    #[must_use]
    pub(crate) const fn new(index: usize) -> Self {
        Self(index)
    }

    #[must_use]
    pub fn index(self) -> usize {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefEdge {
    pub id: RefEdgeId,
    pub site: RefSite,
    pub expected_type: CfdTypeId,
    pub domain: CfdDomainId,
    pub key: RecordKey,
    pub target: CfdRecordId,
    pub target_type: CfdTypeId,
}

/// Logical address of a field value inherited through object/record spread.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SpreadSite {
    pub host: CfdRecordId,
    pub path: CfdPath,
}

impl SpreadSite {
    #[must_use]
    pub const fn new(host: CfdRecordId, path: CfdPath) -> Self {
        Self { host, path }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SpreadEdgeId(usize);

impl SpreadEdgeId {
    #[must_use]
    pub(crate) const fn new(index: usize) -> Self {
        Self(index)
    }

    #[must_use]
    pub fn index(self) -> usize {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpreadEdge {
    pub id: SpreadEdgeId,
    pub site: SpreadSite,
    pub host: CfdRecordId,
    pub path: CfdPath,
    pub fields: BTreeSet<FieldName>,
    pub expected_type: CfdTypeId,
    pub domain: CfdDomainId,
    pub source_key: RecordKey,
    pub source: CfdRecordId,
    pub source_type: CfdTypeId,
}

impl SpreadEdge {
    #[must_use]
    pub fn covers_path(&self, path: &CfdPath) -> bool {
        if !path.segments.starts_with(&self.path.segments) {
            return false;
        }
        let relative = &path.segments[self.path.segments.len()..];
        let Some(CfdPathSegment::Field(field)) = relative.first() else {
            return false;
        };
        self.fields.contains(field.as_str())
    }

    #[must_use]
    pub fn source_path_for(&self, host_path: &CfdPath) -> Option<CfdPath> {
        if !self.covers_path(host_path) {
            return None;
        }
        let relative = &host_path.segments[self.path.segments.len()..];
        Some(CfdPath {
            segments: relative.to_vec(),
        })
    }
}
