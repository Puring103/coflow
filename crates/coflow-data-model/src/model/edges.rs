use super::ids::{CfdDomainId, CfdRecordId, CfdTypeId};
use crate::diagnostic::{CfdPath, CfdPathSegment};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Logical address of a `CfdValue::Ref` instance inside the model: the host
/// record and the `CfdPath` to the ref.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RefSite {
    pub host: CfdRecordId,
    pub path: CfdPath,
}

impl RefSite {
    #[must_use]
    pub const fn new(host: CfdRecordId, path: CfdPath) -> Self {
        Self { host, path }
    }
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
    pub host: CfdRecordId,
    pub path: CfdPath,
    pub expected_type: CfdTypeId,
    pub domain: CfdDomainId,
    pub key: String,
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
    pub fields: BTreeSet<String>,
    pub expected_type: CfdTypeId,
    pub domain: CfdDomainId,
    pub source_key: String,
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
        self.fields.contains(field)
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
