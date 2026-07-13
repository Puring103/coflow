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
use std::collections::BTreeSet;

use super::{ProjectSession, RecordCoordinate};
use crate::indexes::RecordRef;
pub(crate) use plan::{prepare_mutation_execution, MutationExecutionPlan};
use rebuild::{rebuild_session_after_write, MutationRebuild};
pub(crate) use stage::{preflight_mutation_op, stage_mutation_op};
pub(crate) use transaction::MutationTransaction;

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
    affected_files: &BTreeSet<String>,
) -> Result<MutationRebuild, DiagnosticSet> {
    rebuild_session_after_write(session, registry, affected_files)
}
