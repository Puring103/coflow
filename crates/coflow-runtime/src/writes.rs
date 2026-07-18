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
use crate::IncrementalFallbackReason;
pub(crate) use plan::{prepare_mutation_execution, MutationExecutionPlan};
use rebuild::{rebuild_session_after_write, MutationRebuild};
pub(crate) use stage::{
    preflight_mutation_op, stage_field_mutation_batch, stage_mutation_op, MutationBatchFailure,
};
pub(crate) use transaction::MutationTransaction;

#[derive(Debug, Default)]
pub(crate) struct MutationImpact {
    pub(crate) affected_files: BTreeSet<String>,
    pub(crate) changed_records: BTreeSet<RecordCoordinate>,
    pub(crate) structural_change: bool,
    pub(crate) fallback_reason: Option<IncrementalFallbackReason>,
}

impl MutationImpact {
    pub(crate) fn from_outcomes<'a>(
        outcomes: impl IntoIterator<Item = &'a crate::WriteOutcome>,
    ) -> Self {
        let mut impact = Self::default();
        for outcome in outcomes {
            impact
                .affected_files
                .extend(outcome.affected_files.iter().cloned());
            impact
                .changed_records
                .extend(outcome.touched.iter().cloned());
            if let Some(inserted) = &outcome.inserted {
                impact.structural_change = true;
                impact
                    .fallback_reason
                    .get_or_insert(IncrementalFallbackReason::RecordInserted);
                impact.changed_records.insert(inserted.clone());
            }
            if let Some(deleted) = &outcome.deleted {
                impact.structural_change = true;
                impact
                    .fallback_reason
                    .get_or_insert(IncrementalFallbackReason::RecordDeleted);
                impact.changed_records.insert(deleted.clone());
            }
            if let Some((old, new)) = &outcome.renamed {
                impact.structural_change = true;
                impact
                    .fallback_reason
                    .get_or_insert(IncrementalFallbackReason::RecordRenamed);
                impact.changed_records.insert(old.clone());
                impact.changed_records.insert(new.clone());
            }
            if outcome.reordered {
                impact.structural_change = true;
                impact
                    .fallback_reason
                    .get_or_insert(IncrementalFallbackReason::RecordReordered);
            }
        }
        impact
    }
}

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
    impact: &MutationImpact,
) -> Result<MutationRebuild, DiagnosticSet> {
    rebuild_session_after_write(session, registry, impact)
}
