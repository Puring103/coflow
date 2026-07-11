use std::sync::Arc;

use coflow_api::{
    DiagnosticSet, ProviderRegistry, ResolvedSource, SourceWriter, WriteFieldPathSegment,
};
use coflow_data_model::CfdValue;

use crate::{ProjectSession, RecordCoordinate};

use super::target::{not_found, write_target_for_path, WriteTarget};
use super::writer::{lookup_source_writer, source_for_file};
use crate::write_rules;

pub(super) struct WriteFieldPlan {
    pub(super) host_coordinate: RecordCoordinate,
    pub(super) target: WriteTarget,
    pub(super) source: ResolvedSource,
    pub(super) writer: Arc<dyn SourceWriter>,
}

pub(super) fn prepare_write_field(
    session: &ProjectSession,
    registry: &ProviderRegistry,
    actual_type: &str,
    key: &str,
    path: &[WriteFieldPathSegment],
    new_value: &CfdValue,
) -> Result<WriteFieldPlan, DiagnosticSet> {
    let Some(record_ref) = session.records.get_by_coordinate(actual_type, key) else {
        return Err(DiagnosticSet::one(not_found(actual_type, key)));
    };
    let Some(_record) = session.model.record(record_ref.id) else {
        return Err(DiagnosticSet::one(not_found(actual_type, key)));
    };
    let target = write_target_for_path(session, record_ref, path)?;
    write_rules::validate_value_at_write_path(
        session,
        &target.coordinate.actual_type,
        &target.field_path,
        new_value,
        "WRITE-SHAPE",
        "WRITE",
    )?;
    let source = source_for_file(session, &target.display_path)?;
    let writer = lookup_source_writer(registry, &source)?;
    Ok(WriteFieldPlan {
        host_coordinate: record_ref.coordinate.clone(),
        target,
        source,
        writer,
    })
}
