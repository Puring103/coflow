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

use crate::api::DiagnosticSet;
use crate::data_model::{CfdPath, CfdPathSegment, CfdRecord, CfdValue};
use coflow_language::cft::{FieldName, TypeName};
use std::collections::{BTreeMap, BTreeSet};

use super::{ProjectSession, RecordCoordinate};
use crate::checks::impact::{ChangedField, ChangedProjection, ChangedRecordFields, CheckImpact};
pub(crate) use plan::{prepare_mutation_execution, ExecutableMutationOp, FieldWrite};
pub(crate) use stage::{stage_field_mutation_batch, stage_mutation_op, MutationBatchFailure};
pub(crate) use target::WriteTarget;

#[derive(Debug, Default)]
pub(crate) struct MutationImpact {
    pub(crate) affected_files: BTreeSet<String>,
    pub(crate) record_changes: BTreeMap<RecordCoordinate, ChangedRecordFields>,
    membership_types: BTreeSet<TypeName>,
    pub(crate) structural_change: bool,
}

impl MutationImpact {
    pub(crate) fn from_operations<'a>(
        operations: impl IntoIterator<Item = (&'a Self, &'a crate::WriteOutcome)>,
    ) -> Self {
        let mut impact = Self::default();
        for (operation, outcome) in operations {
            impact
                .affected_files
                .extend(outcome.affected_files.iter().cloned());
            impact.merge(operation);
            for touched in &outcome.touched {
                impact
                    .record_changes
                    .entry(touched.clone())
                    .or_insert(ChangedRecordFields::All);
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

    pub(crate) fn for_operation(operation: &crate::mutation::PreparedMutationOp) -> Self {
        let mut impact = Self::default();
        impact.add_operation_change(operation);
        impact
    }

    pub(crate) fn merge(&mut self, other: &Self) {
        self.affected_files
            .extend(other.affected_files.iter().cloned());
        self.membership_types
            .extend(other.membership_types.iter().cloned());
        self.structural_change |= other.structural_change;
        for (record, change) in &other.record_changes {
            match change {
                ChangedRecordFields::All => self.add_all(record.clone()),
                ChangedRecordFields::Fields(fields) => {
                    for field in fields {
                        self.add_field(record.clone(), field.clone());
                    }
                }
            }
        }
    }

    pub(crate) fn check_impact(&self, schema: &coflow_language::cft::CftSchema) -> CheckImpact {
        let mut memberships = BTreeSet::new();
        for actual_type in &self.membership_types {
            memberships.insert(actual_type.clone());
            if let Some(ancestors) = schema.ancestor_type_names(actual_type) {
                memberships.extend(ancestors.iter().cloned());
            }
        }
        CheckImpact {
            records: self.record_changes.clone(),
            record_sets: memberships,
        }
    }

    fn add_operation_change(&mut self, operation: &crate::mutation::PreparedMutationOp) {
        use crate::mutation::PreparedMutationOp;
        match operation {
            PreparedMutationOp::SetField { target, .. }
            | PreparedMutationOp::UnsetField { target } => self.add_path(
                target.coordinate.clone(),
                &CfdPath {
                    segments: target.field_path.clone(),
                },
            ),
            PreparedMutationOp::FoldedSetField { record, path, .. } => {
                self.add_path(record.clone(), path);
            }
            PreparedMutationOp::WriteDimensionValue {
                record, coordinate, ..
            } => self.add_field(
                record.clone(),
                ChangedField {
                    field: coordinate.field.clone(),
                    projection: ChangedProjection::Dimension {
                        dimension: coordinate.dimension.clone(),
                        variant: coordinate.variant.clone(),
                    },
                },
            ),
            PreparedMutationOp::InsertRecord {
                actual_type, id, ..
            } => {
                self.add_structural_record(&RecordCoordinate::new(actual_type.clone(), id.clone()));
            }
            PreparedMutationOp::RestoreRecord { snapshot } => {
                self.add_structural_record(&snapshot.record);
            }
            PreparedMutationOp::CancelledInsert { record, .. }
            | PreparedMutationOp::DeleteRecord { record, .. }
            | PreparedMutationOp::FoldedDeleteRecord { record, .. } => {
                self.add_structural_record(record);
            }
            PreparedMutationOp::RenameRecord { record, new_id, .. } => {
                self.add_structural_record(record);
                self.add_structural_record(&RecordCoordinate::new(
                    record.actual_type.clone(),
                    new_id.clone(),
                ));
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

    fn add_path(&mut self, record: RecordCoordinate, path: &CfdPath) {
        let Some(field) = path.segments.iter().find_map(|segment| match segment {
            CfdPathSegment::Field(field) => FieldName::new(field).ok(),
            CfdPathSegment::Index(_) | CfdPathSegment::DictKey(_) => None,
        }) else {
            self.add_all(record);
            return;
        };
        self.add_field(
            record,
            ChangedField {
                field,
                projection: ChangedProjection::Base,
            },
        );
    }

    fn add_field(&mut self, record: RecordCoordinate, field: ChangedField) {
        match self.record_changes.entry(record) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(ChangedRecordFields::Fields(BTreeSet::from([field])));
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                if let ChangedRecordFields::Fields(fields) = entry.get_mut() {
                    fields.insert(field);
                }
            }
        }
    }

    fn add_all(&mut self, record: RecordCoordinate) {
        self.record_changes.insert(record, ChangedRecordFields::All);
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

pub(crate) fn rebuild_after_mutation(
    session: &ProjectSession,
    impact: &MutationImpact,
    source_overrides: &[crate::DataSourceTextOverride],
    preloaded_sources: BTreeMap<std::path::PathBuf, crate::api::LoadedCfdSource>,
) -> Result<crate::sessions::session_build::SessionBuildOutput, DiagnosticSet> {
    crate::sessions::session_build::rebuild_project_session_from_generation(
        session,
        impact,
        source_overrides,
        preloaded_sources,
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;
    use crate::data_model::CfdPathSegment;
    use crate::mutation::PreparedMutationOp;
    use coflow_language::cft::RecordId;

    fn coordinate(key: &str) -> RecordCoordinate {
        RecordCoordinate::new(
            TypeName::new("Item").expect("valid type name"),
            RecordId::new(key).expect("valid record key"),
        )
    }

    #[test]
    fn mutation_impact_unions_precise_paths_and_structural_changes_absorb_them() {
        let record = coordinate("sword");
        let price = MutationImpact::for_operation(&PreparedMutationOp::FoldedSetField {
            record: record.clone(),
            write_file: "items.cfd".to_string(),
            path: CfdPath {
                segments: vec![CfdPathSegment::Field("price".to_string())],
            },
        });
        let name = MutationImpact::for_operation(&PreparedMutationOp::FoldedSetField {
            record: record.clone(),
            write_file: "items.cfd".to_string(),
            path: CfdPath {
                segments: vec![CfdPathSegment::Field("name".to_string())],
            },
        });
        let touched = crate::WriteOutcome::touch(record.clone());
        let operations = [(&price, &touched), (&name, &touched)];
        let impact = MutationImpact::from_operations(operations);
        assert_eq!(
            impact.record_changes.get(&record),
            Some(&ChangedRecordFields::Fields(BTreeSet::from([
                ChangedField {
                    field: FieldName::new("name").expect("field"),
                    projection: ChangedProjection::Base,
                },
                ChangedField {
                    field: FieldName::new("price").expect("field"),
                    projection: ChangedProjection::Base,
                },
            ])))
        );
        assert!(impact.membership_types.is_empty());

        let deleted = MutationImpact::for_operation(&PreparedMutationOp::DeleteRecord {
            record: record.clone(),
            report_file: Some("items.cfd".to_string()),
        });
        let deleted_outcome = crate::WriteOutcome {
            deleted: Some(record.clone()),
            ..Default::default()
        };
        let impact =
            MutationImpact::from_operations([(&price, &touched), (&deleted, &deleted_outcome)]);
        assert_eq!(
            impact.record_changes.get(&record),
            Some(&ChangedRecordFields::All)
        );
        assert_eq!(
            impact.membership_types,
            BTreeSet::from([record.actual_type])
        );
    }
}
