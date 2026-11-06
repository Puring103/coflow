use crate::api::{Diagnostic, WriteFieldPathSegment};
use crate::data_model::RecordOrigin;

use crate::indexes::{RecordRef, SourceId};
use crate::RecordCoordinate;

pub(super) fn not_found(actual_type: &str, key: &str) -> Diagnostic {
    Diagnostic::error(
        "WRITE-NOT-FOUND",
        "WRITE",
        format!("record `{actual_type}.{key}` was not found in the session"),
    )
}

#[derive(Debug, Clone)]
pub(crate) struct WriteTarget {
    pub(crate) coordinate: RecordCoordinate,
    pub(crate) origin: RecordOrigin,
    pub(crate) source_id: SourceId,
    pub(crate) display_path: String,
    pub(crate) field_path: Vec<WriteFieldPathSegment>,
}

impl WriteTarget {
    pub(crate) fn new(record: &RecordRef, field_path: Vec<WriteFieldPathSegment>) -> Self {
        Self {
            coordinate: record.coordinate.clone(),
            origin: record.origin.clone(),
            source_id: record.source_id,
            display_path: record.display_path.clone(),
            field_path,
        }
    }
}
