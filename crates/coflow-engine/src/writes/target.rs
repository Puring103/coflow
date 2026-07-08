use coflow_api::{Diagnostic, DiagnosticSet, RecordOrigin, WriteFieldPathSegment};
use coflow_data_model::CfdValue;

use super::path::{cfd_path_from_write_path, cfd_path_to_write_path};
use crate::{ProjectSession, RecordCoordinate, RecordRef};

/// Compute the post-write coordinate. Writers don't tell us the new key, so
/// we walk the path: only a write at exactly `[Field("id")]` can rename the
/// record. Everything else preserves the original coordinate.
pub(super) fn guess_new_coordinate(
    session: &ProjectSession,
    old: &RecordCoordinate,
    path: &[WriteFieldPathSegment],
    new_value: &CfdValue,
) -> RecordCoordinate {
    if path.len() == 1 {
        if let WriteFieldPathSegment::Field(name) = &path[0] {
            if name == "id" {
                if let CfdValue::String(new_key) = new_value {
                    if session
                        .records
                        .get_by_coordinate(&old.actual_type, new_key)
                        .is_some()
                    {
                        return RecordCoordinate::new(&old.actual_type, new_key.clone());
                    }
                }
            }
        }
    }
    let _ = session;
    let _ = (path, new_value);
    old.clone()
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
    let path = cfd_path_from_write_path(path);
    let Some((source_id, source_path)) = session.model.spread_source_path(host_ref.id, &path)
    else {
        return Ok(WriteTarget {
            coordinate: host_ref.coordinate.clone(),
            origin: host_ref.origin.clone(),
            display_path: host_ref.display_path.clone(),
            field_path: cfd_path_to_write_path(&path),
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
        field_path: cfd_path_to_write_path(&source_path),
    })
}
