use coflow_language::cft::{CftNameError, RecordId, TypeName};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Stable business identity of a top-level record across model generations.
///
/// [`CfdRecordHandle`] is only valid inside one model generation. Persisted and
/// wire-facing references to top-level records must use this coordinate.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct RecordCoordinate {
    #[cfg_attr(feature = "ts-export", ts(type = "string"))]
    pub actual_type: TypeName,
    #[cfg_attr(feature = "ts-export", ts(type = "string"))]
    pub id: RecordId,
}

impl RecordCoordinate {
    #[must_use]
    pub const fn new(actual_type: TypeName, id: RecordId) -> Self {
        Self { actual_type, id }
    }

    /// Validates a raw wire coordinate before it enters the successful model
    /// identity domain.
    ///
    /// # Errors
    ///
    /// Returns an error when either component is not a valid CFT identifier.
    pub fn try_new(
        actual_type: impl AsRef<str>,
        id: impl AsRef<str>,
    ) -> Result<Self, CftNameError> {
        Ok(Self::new(TypeName::new(actual_type)?, RecordId::new(id)?))
    }

    #[must_use]
    pub fn actual_type(&self) -> &str {
        self.actual_type.as_str()
    }

    #[must_use]
    pub fn id(&self) -> &str {
        self.id.as_str()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CfdRecordHandle(usize);

impl CfdRecordHandle {
    #[must_use]
    pub(crate) fn new(index: usize) -> Self {
        Self(index)
    }

    #[must_use]
    pub fn index(self) -> usize {
        self.0
    }
}

impl fmt::Display for CfdRecordHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
