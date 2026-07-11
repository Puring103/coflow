//! Source-write staging behind the runtime mutation transaction.
//!
//! Hosts write through [`crate::WriteProjectSession`]. This module resolves
//! stable record coordinates, performs provider I/O, and leaves transaction
//! compensation plus the single post-write rebuild to `mutation::apply`.

mod plan;
mod rebuild;
mod refs;
mod stage;
mod target;
mod transaction;
mod writer;

use coflow_api::{DiagnosticSet, ProviderRegistry, WriteFieldPathSegment};
use coflow_data_model::{CfdPath, CfdRecord, CfdValue};
use std::sync::Arc;

use super::{ProjectSession, RecordCoordinate, RecordRef};
use rebuild::rebuild_session_after_write;
use refs::{reference_update_actions, source_rewrite_actions};
pub(crate) use stage::{preflight_mutation_op, stage_mutation_op};
use target::not_found;
pub(crate) use transaction::MutationTransaction;
use writer::{lookup_source_writer, source_for_file};

use crate::mutation::PreparedMutationOp;

type MutationSource = (
    coflow_api::ResolvedSource,
    Arc<dyn coflow_api::SourceWriter>,
);

pub(crate) fn record_value_at_path<'a>(
    record: &'a CfdRecord,
    path: &CfdPath,
) -> Option<&'a CfdValue> {
    record.value_at_path(path)
}

pub(crate) fn effective_write_target_for_path(
    session: &ProjectSession,
    host_ref: &RecordRef,
    path: &[WriteFieldPathSegment],
) -> Result<(RecordCoordinate, String, Vec<WriteFieldPathSegment>), DiagnosticSet> {
    let target = target::write_target_for_path(session, host_ref, path)?;
    Ok((target.coordinate, target.display_path, target.field_path))
}

pub(crate) fn rebuild_after_mutation(
    session: &ProjectSession,
    registry: &ProviderRegistry,
) -> Result<ProjectSession, DiagnosticSet> {
    rebuild_session_after_write(session, registry)
}

pub(crate) fn mutation_sources(
    session: &ProjectSession,
    registry: &ProviderRegistry,
    op: &PreparedMutationOp,
) -> Result<Vec<MutationSource>, DiagnosticSet> {
    match op {
        PreparedMutationOp::InsertRecord { file, .. }
        | PreparedMutationOp::SetField {
            write_file: file, ..
        } => {
            let source = source_for_file(session, file)?;
            let writer = lookup_source_writer(registry, &source)?;
            Ok(vec![(source, writer)])
        }
        PreparedMutationOp::DeleteRecord { record, .. } => {
            let file = session
                .file_for_record(&record.actual_type, &record.key)
                .ok_or_else(|| DiagnosticSet::one(not_found(&record.actual_type, &record.key)))?;
            let source = source_for_file(session, file)?;
            let writer = lookup_source_writer(registry, &source)?;
            Ok(vec![(source, writer)])
        }
        PreparedMutationOp::RenameRecord {
            record, new_key, ..
        } => {
            let Some(target_ref) = session
                .records
                .get_by_coordinate(&record.actual_type, &record.key)
            else {
                return Err(DiagnosticSet::one(not_found(
                    &record.actual_type,
                    &record.key,
                )));
            };
            let target_source = source_for_file(session, &target_ref.display_path)?;
            let target_writer = lookup_source_writer(registry, &target_source)?;
            let mut sources = vec![(target_source, target_writer)];
            sources.extend(
                reference_update_actions(session, registry, target_ref.id, new_key)?
                    .into_iter()
                    .map(|action| (action.source().clone(), action.writer)),
            );
            sources.extend(
                source_rewrite_actions(session, registry, target_ref.id, &record.key, new_key)?
                    .into_iter()
                    .map(|action| (action.source().clone(), action.writer)),
            );
            Ok(sources)
        }
        PreparedMutationOp::FoldedSetField { .. }
        | PreparedMutationOp::FoldedRenameRecord { .. }
        | PreparedMutationOp::FoldedDeleteRecord { .. }
        | PreparedMutationOp::CancelledInsert { .. }
        | PreparedMutationOp::Pending { .. } => Ok(Vec::new()),
    }
}
