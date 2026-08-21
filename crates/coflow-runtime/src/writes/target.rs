use crate::api::{Diagnostic, DiagnosticSet, WriteFieldPathSegment};
use coflow_data_model::RecordOrigin;

use crate::indexes::{RecordRef, SourceId};
use crate::{ProjectSession, RecordCoordinate};

pub(super) fn not_found(actual_type: &str, key: &str) -> Diagnostic {
    Diagnostic::error(
        "WRITE-NOT-FOUND",
        "WRITE",
        format!("record `{actual_type}.{key}` was not found in the session"),
    )
}

pub(super) fn is_id_path(path: &[WriteFieldPathSegment]) -> bool {
    matches!(path, [WriteFieldPathSegment::Field(name)] if name == "id")
}

#[derive(Debug, Clone)]
pub(super) struct WriteTarget {
    pub(super) coordinate: RecordCoordinate,
    pub(super) origin: RecordOrigin,
    pub(super) source_id: SourceId,
    pub(super) display_path: String,
    pub(super) field_path: Vec<WriteFieldPathSegment>,
}

pub(super) fn write_target_for_path(
    _session: &ProjectSession,
    host_ref: &RecordRef,
    path: &[WriteFieldPathSegment],
) -> Result<WriteTarget, DiagnosticSet> {
    Ok(WriteTarget {
        coordinate: host_ref.coordinate.clone(),
        origin: host_ref.origin.clone(),
        source_id: host_ref.source_id,
        display_path: host_ref.display_path.clone(),
        field_path: path.to_vec(),
    })
}
