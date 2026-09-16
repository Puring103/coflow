//! Source-write staging behind the mutation publication flow.
//!
//! Hosts write through [`crate::WriteProjectSession`]. This module resolves
//! stable record coordinates, stages CFD writer I/O, and leaves candidate
//! validation plus publication to `mutation::apply`.

mod plan;
mod refs;
mod stage;
mod target;
mod writer;

use crate::api::{CfdSourceCatalog, DiagnosticSet, WriteFieldPathSegment};
use crate::data_model::{CfdPath, CfdRecord, CfdValue};
use std::collections::BTreeSet;

use super::{ProjectSession, RecordCoordinate};
use crate::indexes::RecordRef;
pub(crate) use plan::{prepare_mutation_execution, MutationExecutionPlan};
pub(crate) use stage::{stage_field_mutation_batch, stage_mutation_op, MutationBatchFailure};

#[derive(Debug, Default)]
pub(crate) struct MutationImpact {
    pub(crate) affected_files: BTreeSet<String>,
    records: BTreeSet<RecordCoordinate>,
    pub(crate) structural_change: bool,
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
                impact.records.insert(touched.clone());
            }
            if let Some(inserted) = &outcome.inserted {
                impact.structural_change = true;
                impact.add_structural_record(inserted);
            }
            if let Some(deleted) = &outcome.deleted {
                impact.structural_change = true;
                impact.add_structural_record(deleted);
            }
            if let Some((old, new)) = &outcome.renamed {
                impact.structural_change = true;
                impact.add_structural_record(old);
                impact.add_structural_record(new);
            }
            if outcome.reordered {
                impact.structural_change = true;
            }
        }
        impact
    }

    pub(crate) fn changed_records(&self) -> BTreeSet<RecordCoordinate> {
        self.records.clone()
    }

    fn add_operation_change(&mut self, operation: &crate::mutation::PreparedMutationOp) {
        use crate::mutation::PreparedMutationOp;
        match operation {
            PreparedMutationOp::SetField { write_record, .. }
            | PreparedMutationOp::UnsetField { write_record, .. } => self.add_all(write_record.clone()),
            PreparedMutationOp::FoldedSetField { record, .. }
            | PreparedMutationOp::WriteDimensionValue { record, .. } => self.add_all(record.clone()),
            PreparedMutationOp::InsertRecord {
                actual_type, key, ..
            } => {
                self.add_structural_record(&RecordCoordinate::new(
                    actual_type.clone(),
                    key.clone(),
                ));
            }
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

    fn add_all(&mut self, record: RecordCoordinate) {
        self.records.insert(record);
    }

    fn add_structural_record(&mut self, record: &RecordCoordinate) {
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
) -> (RecordCoordinate, String, Vec<WriteFieldPathSegment>) {
    let target = target::write_target_for_path(session, host_ref, path);
    (target.coordinate, target.display_path, target.field_path)
}

pub(crate) fn rebuild_after_mutation(
    session: &ProjectSession,
    catalog: &CfdSourceCatalog,
    impact: &MutationImpact,
    source_overrides: &[crate::DataSourceTextOverride],
) -> Result<crate::session_build::SessionBuildOutput, DiagnosticSet> {
    crate::session_build::rebuild_project_session_from_generation(
        session,
        catalog,
        impact,
        source_overrides,
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;
    use crate::data_model::{CfdPathSegment, CfdValue};
    use crate::mutation::PreparedMutationOp;
    use coflow_core::schema::{RecordKey, TypeName};

    fn coordinate(key: &str) -> RecordCoordinate {
        RecordCoordinate::new(
            TypeName::new("Item").expect("valid type name"),
            RecordKey::new(key).expect("valid record key"),
        )
    }

    #[test]
    fn mutation_impact_tracks_changed_records_and_structural_changes() {
        let record = coordinate("sword");
        let price = PreparedMutationOp::SetField {
            record: record.clone(),
            write_record: record.clone(),
            write_file: "items.cfd".to_string(),
            path: vec![CfdPathSegment::Field("price".to_string())],
            value: CfdValue::Int(10),
            materialized_top_level: None,
        };
        let name = PreparedMutationOp::SetField {
            record: record.clone(),
            write_record: record.clone(),
            write_file: "items.cfd".to_string(),
            path: vec![CfdPathSegment::Field("name".to_string())],
            value: CfdValue::String("Sword".to_string()),
            materialized_top_level: None,
        };
        let touched = crate::WriteOutcome::touch(record.clone());
        let operations = [(&price, &touched), (&name, &touched)];
        let impact = MutationImpact::from_operations(operations);
        assert_eq!(impact.changed_records(), BTreeSet::from([record.clone()]));

        let deleted = PreparedMutationOp::DeleteRecord {
            record: record.clone(),
            report_file: Some("items.cfd".to_string()),
        };
        let deleted_outcome = crate::WriteOutcome {
            deleted: Some(record.clone()),
            ..Default::default()
        };
        let impact =
            MutationImpact::from_operations([(&price, &touched), (&deleted, &deleted_outcome)]);
        assert_eq!(impact.changed_records(), BTreeSet::from([record]));
        assert!(impact.structural_change);
    }
}
