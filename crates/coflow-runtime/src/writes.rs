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
use coflow_checker::{ChangedPaths, CheckChangeSet};
use coflow_cft::{CftSchema, TypeName};
use coflow_data_model::{CfdPath, CfdRecord, CfdValue};
use std::collections::{BTreeMap, BTreeSet};

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
    pub(crate) record_changes: BTreeMap<RecordCoordinate, ChangedPaths>,
    membership_types: BTreeSet<TypeName>,
    pub(crate) structural_change: bool,
    pub(crate) fallback_reason: Option<IncrementalFallbackReason>,
}

impl MutationImpact {
    pub(crate) fn from_operations<'a>(
        operations: impl IntoIterator<
            Item = (
                &'a crate::mutation::PreparedMutationOp,
                &'a crate::WriteOutcome,
            ),
        >,
    ) -> Self {
        let mut impact = Self::default();
        for (operation, outcome) in operations {
            impact
                .affected_files
                .extend(outcome.affected_files.iter().cloned());
            impact.add_operation_change(operation);
            for touched in &outcome.touched {
                impact
                    .record_changes
                    .entry(touched.clone())
                    .or_insert(ChangedPaths::All);
            }
            if let Some(inserted) = &outcome.inserted {
                impact.structural_change = true;
                impact
                    .fallback_reason
                    .get_or_insert(IncrementalFallbackReason::RecordInserted);
                impact.add_structural_record(inserted);
            }
            if let Some(deleted) = &outcome.deleted {
                impact.structural_change = true;
                impact
                    .fallback_reason
                    .get_or_insert(IncrementalFallbackReason::RecordDeleted);
                impact.add_structural_record(deleted);
            }
            if let Some((old, new)) = &outcome.renamed {
                impact.structural_change = true;
                impact
                    .fallback_reason
                    .get_or_insert(IncrementalFallbackReason::RecordRenamed);
                impact.add_structural_record(old);
                impact.add_structural_record(new);
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

    pub(crate) fn changed_records(&self) -> BTreeSet<RecordCoordinate> {
        self.record_changes.keys().cloned().collect()
    }

    pub(crate) fn check_change_set(&self, schema: &CftSchema) -> CheckChangeSet {
        let mut memberships = BTreeSet::new();
        for actual_type in &self.membership_types {
            memberships.insert(actual_type.clone());
            if let Some(ancestors) = schema.ancestor_type_names(actual_type) {
                memberships.extend(ancestors.iter().cloned());
            }
        }
        CheckChangeSet {
            records: self.record_changes.clone(),
            memberships,
        }
    }

    fn add_operation_change(&mut self, operation: &crate::mutation::PreparedMutationOp) {
        use crate::mutation::PreparedMutationOp;
        match operation {
            PreparedMutationOp::SetField {
                write_record, path, ..
            } => self.add_path(
                write_record.clone(),
                CfdPath {
                    segments: path.clone(),
                },
            ),
            PreparedMutationOp::FoldedSetField { record, path, .. } => {
                self.add_path(record.clone(), path.clone());
            }
            PreparedMutationOp::WriteDimensionValue {
                record, coordinate, ..
            } => self.add_path(record.clone(), coordinate.path.clone()),
            PreparedMutationOp::InsertRecord {
                actual_type, key, ..
            } => self.add_structural_record(&RecordCoordinate::new(
                actual_type.clone(),
                key.clone(),
            )),
            PreparedMutationOp::CancelledInsert { record, .. }
            | PreparedMutationOp::DeleteRecord { record, .. }
            | PreparedMutationOp::FoldedDeleteRecord { record, .. } => {
                self.add_structural_record(record);
            }
            PreparedMutationOp::RenameRecord {
                record, new_key, ..
            } => {
                self.add_structural_record(record);
                self.add_structural_record(&RecordCoordinate::new(
                    record.actual_type.clone(),
                    new_key.clone(),
                ));
            }
            PreparedMutationOp::FoldedRenameRecord {
                old_record,
                new_record,
                ..
            } => {
                self.add_structural_record(old_record);
                self.add_structural_record(new_record);
            }
            PreparedMutationOp::SwapRecords { first, second, .. } => {
                self.add_all(first.clone());
                self.add_all(second.clone());
            }
            PreparedMutationOp::MoveRecord { record, .. }
            | PreparedMutationOp::TransferRecord { record, .. } => {
                self.add_all(record.clone());
            }
        }
    }

    fn add_path(&mut self, record: RecordCoordinate, path: CfdPath) {
        match self.record_changes.entry(record) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(ChangedPaths::Paths(BTreeSet::from([path])));
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                if let ChangedPaths::Paths(paths) = entry.get_mut() {
                    paths.insert(path);
                }
            }
        }
    }

    fn add_all(&mut self, record: RecordCoordinate) {
        self.record_changes.insert(record, ChangedPaths::All);
    }

    fn add_structural_record(&mut self, record: &RecordCoordinate) {
        self.membership_types.insert(record.actual_type.clone());
        self.add_all(record.clone());
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mutation::PreparedMutationOp;
    use coflow_cft::RecordKey;
    use coflow_data_model::CfdPathSegment;

    fn coordinate(key: &str) -> RecordCoordinate {
        RecordCoordinate::new(
            TypeName::new("Item").expect("valid type name"),
            RecordKey::new(key).expect("valid record key"),
        )
    }

    #[test]
    fn mutation_impact_unions_precise_paths_and_structural_changes_absorb_them() {
        let record = coordinate("sword");
        let price = PreparedMutationOp::SetField {
            record: record.clone(),
            write_record: record.clone(),
            write_file: "items.cfd".to_string(),
            path: vec![CfdPathSegment::Field("price".to_string())],
            value: CfdValue::Int(10),
        };
        let name = PreparedMutationOp::SetField {
            record: record.clone(),
            write_record: record.clone(),
            write_file: "items.cfd".to_string(),
            path: vec![CfdPathSegment::Field("name".to_string())],
            value: CfdValue::String("Sword".to_string()),
        };
        let touched = crate::WriteOutcome::touch(record.clone());
        let operations = [(&price, &touched), (&name, &touched)];
        let impact = MutationImpact::from_operations(operations);
        assert_eq!(
            impact.record_changes.get(&record),
            Some(&ChangedPaths::Paths(BTreeSet::from([
                CfdPath::root().field("name"),
                CfdPath::root().field("price"),
            ])))
        );
        assert!(impact.membership_types.is_empty());

        let deleted = PreparedMutationOp::DeleteRecord {
            record: record.clone(),
            report_file: Some("items.cfd".to_string()),
        };
        let deleted_outcome = crate::WriteOutcome {
            deleted: Some(record.clone()),
            ..Default::default()
        };
        let impact = MutationImpact::from_operations([
            (&price, &touched),
            (&deleted, &deleted_outcome),
        ]);
        assert_eq!(impact.record_changes.get(&record), Some(&ChangedPaths::All));
        assert_eq!(impact.membership_types, BTreeSet::from([record.actual_type]));
    }
}
