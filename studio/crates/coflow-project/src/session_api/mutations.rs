use super::WriteProjectSession;
use crate::api::{Diagnostic, DiagnosticSet};
use crate::data_model::{CfdPathSegment, CfdValue};
use crate::{
    DefaultMaterialization, DimensionValueCoordinate, DimensionValueExpectation, MutationFields,
    MutationOp, MutationRequest, MutationValue, RecordCoordinate, WriteOutcome,
};
use std::collections::BTreeMap;

impl WriteProjectSession {
    /// Writes one field and returns its writer outcome.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the mutation is rejected or produces no
    /// applied operation.
    pub fn write_field(
        &mut self,
        actual_type: &str,
        key: &str,
        path: &[CfdPathSegment],
        new_value: &CfdValue,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        self.apply_one(MutationOp::SetField {
            record: validated_coordinate(actual_type, key)?,
            file: None,
            path: path.to_vec(),
            value: MutationValue::Cfd(new_value.clone()),
        })
    }

    /// Removes one existing field from its CFD source.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the mutation is rejected or produces no
    /// applied operation.
    pub fn unset_field(
        &mut self,
        actual_type: &str,
        key: &str,
        path: &[CfdPathSegment],
    ) -> Result<WriteOutcome, DiagnosticSet> {
        self.apply_one(MutationOp::UnsetField {
            record: validated_coordinate(actual_type, key)?,
            file: None,
            path: path.to_vec(),
        })
    }

    /// Writes one record-owned dimension variant value.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the coordinate is invalid or the managed
    /// dimension source cannot be written.
    pub fn write_dimension_value(
        &mut self,
        coordinate: DimensionValueCoordinate,
        new_value: &CfdValue,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        self.apply_one(MutationOp::SetDimensionValue {
            coordinate,
            expected: DimensionValueExpectation::Any,
            value: MutationValue::Cfd(new_value.clone()),
        })
    }

    /// Clears one record-owned dimension variant so it becomes missing.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the coordinate is invalid or the managed
    /// dimension source cannot be written.
    pub fn clear_dimension_value(
        &mut self,
        coordinate: DimensionValueCoordinate,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        self.apply_one(MutationOp::ClearDimensionValue {
            coordinate,
            expected: DimensionValueExpectation::Any,
        })
    }

    /// Renames one record key and its references.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the mutation is rejected or produces no
    /// applied operation.
    pub fn rename_record_key(
        &mut self,
        actual_type: &str,
        old_key: &str,
        new_key: &str,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        self.apply_one(MutationOp::RenameRecord {
            record: validated_coordinate(actual_type, old_key)?,
            file: None,
            new_key: new_key.to_string(),
        })
    }

    /// Inserts one record into the selected source.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the mutation is rejected or produces no
    /// applied operation.
    pub fn insert_record(
        &mut self,
        file: &str,
        record_key: &str,
        actual_type: &str,
        fields: &BTreeMap<String, CfdValue>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        self.apply_one(MutationOp::InsertRecord {
            file: file.to_string(),
            actual_type: actual_type.to_string(),
            key: record_key.to_string(),
            fields: MutationFields::Cfd(fields.clone()),
            materialization: DefaultMaterialization::Minimal,
        })
    }

    /// Deletes one record and updates affected references.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the mutation is rejected or produces no
    /// applied operation.
    pub fn delete_record(
        &mut self,
        actual_type: &str,
        key: &str,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        self.apply_one(MutationOp::DeleteRecord {
            record: validated_coordinate(actual_type, key)?,
            file: None,
        })
    }

    /// Atomically exchange two records inside the same physical source container.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when either record is missing, the records belong
    /// to different containers, or the writer cannot persist record order.
    pub fn swap_records(
        &mut self,
        first: &RecordCoordinate,
        second: &RecordCoordinate,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        self.apply_one(MutationOp::SwapRecords {
            first: first.clone(),
            second: second.clone(),
            file: None,
        })
    }

    /// Move one record to a zero-based final index in its physical container.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the record is missing, the index is outside
    /// the container, or the writer cannot persist record order.
    pub fn move_record(
        &mut self,
        record: &RecordCoordinate,
        target_index: usize,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        self.apply_one(MutationOp::MoveRecord {
            record: record.clone(),
            target_index,
            file: None,
        })
    }

    /// Move a record to a zero-based insertion index in another source file.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the destination cannot host the record type,
    /// the insertion index is invalid, or either source cannot participate in
    /// the atomic transfer transaction.
    pub fn transfer_record(
        &mut self,
        record: &RecordCoordinate,
        destination_file: &str,
        target_index: usize,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        self.apply_one(MutationOp::TransferRecord {
            record: record.clone(),
            destination_file: destination_file.to_string(),
            target_index,
            source_file: None,
        })
    }

    fn apply_one(&mut self, op: MutationOp) -> Result<WriteOutcome, DiagnosticSet> {
        let report = self.apply_mutation(MutationRequest {
            stop_on_write_error: true,
            ops: vec![op],
        });
        if let Some(applied) = report.applied.into_iter().next() {
            return Ok(applied.outcome);
        }
        let mut diagnostics = DiagnosticSet::empty();
        for failed in report.failed {
            diagnostics.extend(failed.into_source_diagnostics());
        }
        if diagnostics.is_empty() {
            diagnostics.push(Diagnostic::error(
                "WRITE-TXN-NO-OUTCOME",
                "WRITE",
                "mutation transaction produced neither an applied operation nor a failure",
            ));
        }
        Err(diagnostics)
    }
}

fn validated_coordinate(actual_type: &str, key: &str) -> Result<RecordCoordinate, DiagnosticSet> {
    RecordCoordinate::try_new(actual_type, key).map_err(|error| {
        DiagnosticSet::one(Diagnostic::error(
            "MUTATION-COORDINATE",
            "MUTATION",
            error.to_string(),
        ))
    })
}
