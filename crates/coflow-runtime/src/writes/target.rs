use coflow_api::{Diagnostic, DiagnosticSet, WriteFieldPathSegment};
use coflow_data_model::CfdPath;
use coflow_data_model::RecordOrigin;

use crate::{ProjectSession, RecordCoordinate, RecordRef};

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
    pub(super) display_path: String,
    pub(super) field_path: Vec<WriteFieldPathSegment>,
}

pub(super) fn write_target_for_path(
    session: &ProjectSession,
    host_ref: &RecordRef,
    path: &[WriteFieldPathSegment],
) -> Result<WriteTarget, DiagnosticSet> {
    let Some(WriteFieldPathSegment::Field(top_field)) = path.first() else {
        return Ok(WriteTarget {
            coordinate: host_ref.coordinate.clone(),
            origin: host_ref.origin.clone(),
            display_path: host_ref.display_path.clone(),
            field_path: path.to_vec(),
        });
    };
    let path = CfdPath {
        segments: path.to_vec(),
    };
    let Some((source_id, source_path)) = session.model.spread_source_path(host_ref.id, &path)
    else {
        return Ok(WriteTarget {
            coordinate: host_ref.coordinate.clone(),
            origin: host_ref.origin.clone(),
            display_path: host_ref.display_path.clone(),
            field_path: path.segments,
        });
    };
    let Some(source_ref) = session.records.get(source_id) else {
        return Err(DiagnosticSet::one(Diagnostic::error(
            "WRITE-SPREAD-SOURCE",
            "WRITE",
            format!("spread source for field `{top_field}` is no longer indexed"),
        )));
    };
    Ok(WriteTarget {
        coordinate: source_ref.coordinate.clone(),
        origin: source_ref.origin.clone(),
        display_path: source_ref.display_path.clone(),
        field_path: source_path.segments,
    })
}
