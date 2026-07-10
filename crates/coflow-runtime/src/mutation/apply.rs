use coflow_api::{DiagnosticSet, FlatDiagnostic, ProviderRegistry};

use crate::{ProjectSession, RecordCoordinate};

use super::prepare::prepare_one;
use super::types::PreparedMutationOp;
use super::{
    MutationAppliedOp, MutationFailedOp, MutationOp, MutationReport, MutationRequest,
    PreparedMutation,
};

impl ProjectSession {
    /// Execute a prepared mutation request through provider writers.
    ///
    /// # Errors
    ///
    /// Returns diagnostics only when execution cannot produce a report.
    /// Per-operation validation and writer failures are represented in the
    /// returned [`MutationReport`].
    pub fn apply_prepared_mutation(
        &mut self,
        registry: &ProviderRegistry,
        prepared: PreparedMutation,
    ) -> Result<MutationReport, DiagnosticSet> {
        let PreparedMutation {
            stop_on_write_error,
            ops,
        } = prepared;
        let mut applied = Vec::new();
        let mut failed = Vec::new();
        let mut write_ok = true;

        for (index, op) in ops.iter().enumerate() {
            match apply_prepared_one(self, registry, op) {
                Ok(applied_op) => applied.push(MutationAppliedOp {
                    index,
                    ..applied_op
                }),
                Err(err) => {
                    write_ok = false;
                    let diagnostics = err.diagnostics();
                    let flat = flat_diagnostics(diagnostics);
                    failed.push(MutationFailedOp {
                        index,
                        op: prepared_op_name(op),
                        diagnostics: flat.clone(),
                    });
                    if stop_on_write_error || err.is_terminal() {
                        return Ok(MutationReport {
                            write_ok: false,
                            check_ok: false,
                            applied,
                            failed,
                            diagnostics: session_flat_diagnostics(self),
                        });
                    }
                }
            }
        }

        let diagnostics = session_flat_diagnostics(self);
        let check_ok = write_ok
            && diagnostics
                .iter()
                .all(|diagnostic| diagnostic.severity != "error");
        Ok(MutationReport {
            write_ok,
            check_ok,
            applied,
            failed,
            diagnostics,
        })
    }

    /// Prepare and execute a mutation request.
    ///
    /// # Errors
    ///
    /// Returns diagnostics only when mutation execution cannot produce a
    /// report. Per-operation validation and writer failures are represented in
    /// the returned [`MutationReport`].
    pub fn apply_mutation(
        &mut self,
        registry: &ProviderRegistry,
        request: MutationRequest,
    ) -> Result<MutationReport, DiagnosticSet> {
        let prepared = self.prepare_mutation(request)?;
        self.apply_prepared_mutation(registry, prepared)
    }
}

fn apply_prepared_one(
    session: &mut ProjectSession,
    registry: &ProviderRegistry,
    op: &PreparedMutationOp,
) -> Result<MutationAppliedOp, MutationApplyError> {
    match op {
        PreparedMutationOp::Pending { op } => {
            let prepared = prepare_one(session, op.clone())
                .map_err(|diagnostics| classify_prepare_error(op, diagnostics))?;
            apply_prepared_one(session, registry, &prepared)
        }
        PreparedMutationOp::InsertRecord {
            file,
            sheet,
            actual_type,
            key,
            fields,
        } => {
            let outcome = session
                .insert_record(registry, file, sheet.as_deref(), key, actual_type, fields)
                .map_err(MutationApplyError::Terminal)?;
            Ok(MutationAppliedOp {
                index: 0,
                op: "insert_record".to_string(),
                record: Some(RecordCoordinate::new(actual_type, key)),
                file: Some(file.clone()),
                outcome,
            })
        }
        PreparedMutationOp::SetField {
            record,
            write_file,
            path,
            value,
            ..
        } => {
            let outcome = session
                .write_field(registry, &record.actual_type, &record.key, path, value)
                .map_err(MutationApplyError::Terminal)?;
            Ok(MutationAppliedOp {
                index: 0,
                op: "set_field".to_string(),
                record: Some(record.clone()),
                file: Some(write_file.clone()),
                outcome,
            })
        }
        PreparedMutationOp::RenameRecord {
            record,
            new_key,
            report_file,
        } => {
            let outcome = session
                .rename_record_key(registry, &record.actual_type, &record.key, new_key)
                .map_err(MutationApplyError::Terminal)?;
            let record = outcome.renamed.as_ref().map_or_else(
                || RecordCoordinate::new(&record.actual_type, new_key),
                |(_, new)| new.clone(),
            );
            Ok(MutationAppliedOp {
                index: 0,
                op: "rename_record".to_string(),
                record: Some(record),
                file: report_file.clone(),
                outcome,
            })
        }
        PreparedMutationOp::DeleteRecord {
            record,
            report_file,
            ..
        } => {
            let outcome = session
                .delete_record(registry, &record.actual_type, &record.key)
                .map_err(MutationApplyError::Terminal)?;
            Ok(MutationAppliedOp {
                index: 0,
                op: "delete_record".to_string(),
                record: Some(record.clone()),
                file: report_file.clone(),
                outcome,
            })
        }
    }
}

#[derive(Debug)]
enum MutationApplyError {
    Recoverable(DiagnosticSet),
    Terminal(DiagnosticSet),
}

impl MutationApplyError {
    const fn is_terminal(&self) -> bool {
        matches!(self, Self::Terminal(_))
    }

    const fn diagnostics(&self) -> &DiagnosticSet {
        match self {
            Self::Recoverable(diagnostics) | Self::Terminal(diagnostics) => diagnostics,
        }
    }
}

fn prepared_op_name(op: &PreparedMutationOp) -> String {
    match op {
        PreparedMutationOp::Pending { op } => mutation_op_name(op).to_string(),
        PreparedMutationOp::InsertRecord { .. } => "insert_record".to_string(),
        PreparedMutationOp::SetField { .. } => "set_field".to_string(),
        PreparedMutationOp::RenameRecord { .. } => "rename_record".to_string(),
        PreparedMutationOp::DeleteRecord { .. } => "delete_record".to_string(),
    }
}

const fn mutation_op_name(op: &MutationOp) -> &'static str {
    match op {
        MutationOp::InsertRecord { .. } => "insert_record",
        MutationOp::SetField { .. } => "set_field",
        MutationOp::RenameRecord { .. } => "rename_record",
        MutationOp::DeleteRecord { .. } => "delete_record",
    }
}

fn classify_prepare_error(op: &MutationOp, diagnostics: DiagnosticSet) -> MutationApplyError {
    let terminal_insert_conflict = matches!(op, MutationOp::InsertRecord { .. })
        && diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "MUTATION-INSERT-CONFLICT"
        });
    if terminal_insert_conflict {
        MutationApplyError::Terminal(diagnostics)
    } else {
        MutationApplyError::Recoverable(diagnostics)
    }
}

fn session_flat_diagnostics(session: &ProjectSession) -> Vec<FlatDiagnostic> {
    session
        .diagnostics
        .as_set()
        .diagnostics
        .iter()
        .enumerate()
        .map(|(index, diagnostic)| {
            let location = session.diagnostics.logical_location(index);
            let actual_type = location.and_then(|l| l.actual_type.clone());
            let record_key = location.and_then(|l| l.record_key.clone());
            let field_path = location.and_then(|l| l.field_path.clone());
            diagnostic.flat_view(actual_type, record_key, field_path)
        })
        .collect()
}

fn flat_diagnostics(diagnostics: &DiagnosticSet) -> Vec<FlatDiagnostic> {
    diagnostics
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.flat_view(None, None, None))
        .collect()
}
