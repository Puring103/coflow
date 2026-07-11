use coflow_api::{Diagnostic, DiagnosticSet, ProviderRegistry, Severity, WriteContext};

use crate::writes::{
    mutation_sources, preflight_mutation_op, rebuild_after_mutation, stage_mutation_op,
    MutationTransaction,
};
use crate::{ProjectSession, RecordCoordinate};

use super::plan::{mutation_op_name, plan_mutations, PlannedMutationOp};
use super::prepare::prepare_mutation_request;
use super::types::PreparedMutationOp;
use super::{MutationAppliedOp, MutationFailedOp, MutationReport, MutationRequest};

impl ProjectSession {
    /// Prepare, stage, and atomically publish a mutation request.
    ///
    /// # Errors
    ///
    /// Returns diagnostics only when execution cannot produce a report.
    /// Validation, provider, transaction, and rebuild failures are represented
    /// in the returned report.
    pub fn apply_mutation(
        &mut self,
        registry: &ProviderRegistry,
        request: MutationRequest,
    ) -> Result<MutationReport, DiagnosticSet> {
        let prepared = prepare_mutation_request(request);
        let (planned, mut failed, mut write_ok, stopped) = plan_mutations(self, prepared);
        if stopped || planned.is_empty() {
            return Ok(report_without_publish(self, write_ok, failed));
        }

        for planned_op in &planned {
            if let Err(diagnostics) = preflight_mutation_op(self, registry, &planned_op.op) {
                failed.push(failed_op(planned_op, &diagnostics));
                return Ok(report_without_publish(self, false, failed));
            }
        }

        let mut enlisted = Vec::new();
        for planned_op in &planned {
            match mutation_sources(self, registry, &planned_op.op) {
                Ok(sources) => enlisted.extend(sources),
                Err(diagnostics) => {
                    write_ok = false;
                    failed.push(failed_op(planned_op, &diagnostics));
                    return Ok(report_without_publish(self, write_ok, failed));
                }
            }
        }

        let compiled_schema = self.compiled_schema();
        let ctx = WriteContext {
            project_root: &self.project.root_dir,
            schema: &compiled_schema,
            model: Some(&self.model),
        };
        let transaction = match MutationTransaction::begin(ctx, enlisted) {
            Ok(transaction) => transaction,
            Err(diagnostics) => {
                if let Some(first_planned) = planned.first() {
                    failed.push(failed_op(first_planned, &diagnostics));
                }
                return Ok(report_without_publish(self, false, failed));
            }
        };

        let mut staged = Vec::with_capacity(planned.len());
        for planned_op in &planned {
            match stage_mutation_op(self, registry, &planned_op.op) {
                Ok(outcome) => match applied_op(planned_op, outcome) {
                    Ok(applied) => staged.push(applied),
                    Err(mut diagnostics) => {
                        transaction.compensate_into(&mut diagnostics);
                        failed.push(failed_op(planned_op, &diagnostics));
                        return Ok(report_without_publish(self, false, failed));
                    }
                },
                Err(mut diagnostics) => {
                    transaction.compensate_into(&mut diagnostics);
                    failed.push(failed_op(planned_op, &diagnostics));
                    return Ok(report_without_publish(self, false, failed));
                }
            }
        }

        let new_session = match rebuild_after_mutation(self, registry) {
            Ok(session) => session,
            Err(mut diagnostics) => {
                transaction.compensate_into(&mut diagnostics);
                if let Some(last_planned) = planned.last() {
                    failed.push(failed_op(last_planned, &diagnostics));
                }
                return Ok(report_without_publish(self, false, failed));
            }
        };
        let mut rebuild_diagnostics = blocking_rebuild_diagnostics(&new_session);
        if !rebuild_diagnostics.is_empty() {
            transaction.compensate_into(&mut rebuild_diagnostics);
            if let Some(last_planned) = planned.last() {
                failed.push(failed_op(last_planned, &rebuild_diagnostics));
            }
            return Ok(report_without_publish(self, false, failed));
        }

        if let Err(diagnostics) = transaction.commit() {
            if let Some(last_planned) = planned.last() {
                failed.push(failed_op(last_planned, &diagnostics));
            }
            return Ok(report_without_publish(self, false, failed));
        }

        let diagnostics_set = new_session.diagnostics.as_set().clone();
        let diagnostics = new_session.diagnostics.flat_diagnostics();
        for applied in &mut staged {
            applied.outcome.diagnostics = diagnostics_set.clone();
        }
        *self = new_session;
        staged.sort_by_key(|applied| applied.index);
        failed.sort_by_key(|failure| failure.index);
        let check_ok = write_ok
            && diagnostics
                .iter()
                .all(|diagnostic| diagnostic.severity != "error");
        Ok(MutationReport {
            write_ok,
            check_ok,
            applied: staged,
            failed,
            diagnostics,
        })
    }
}

fn blocking_rebuild_diagnostics(session: &ProjectSession) -> DiagnosticSet {
    session
        .diagnostics
        .as_set()
        .iter()
        .filter(|diagnostic| {
            diagnostic.severity == Severity::Error && diagnostic.stage != "CHECK"
        })
        .cloned()
        .collect::<Vec<_>>()
        .into()
}

fn applied_op(
    planned: &PlannedMutationOp,
    outcome: crate::WriteOutcome,
) -> Result<MutationAppliedOp, DiagnosticSet> {
    let (op, record, file) = match &planned.op {
        PreparedMutationOp::InsertRecord {
            file,
            actual_type,
            key,
            ..
        } => (
            "insert_record",
            Some(RecordCoordinate::new(actual_type, key)),
            Some(file.clone()),
        ),
        PreparedMutationOp::CancelledInsert {
            record,
            write_file,
        } => (
            "insert_record",
            Some(record.clone()),
            Some(write_file.clone()),
        ),
        PreparedMutationOp::SetField {
            record, write_file, ..
        }
        | PreparedMutationOp::FoldedSetField {
            record,
            write_file,
        } => ("set_field", Some(record.clone()), Some(write_file.clone())),
        PreparedMutationOp::RenameRecord {
            record,
            new_key,
            report_file,
        } => (
            "rename_record",
            Some(RecordCoordinate::new(&record.actual_type, new_key)),
            report_file.clone(),
        ),
        PreparedMutationOp::FoldedRenameRecord {
            new_record,
            write_file,
            ..
        } => (
            "rename_record",
            Some(new_record.clone()),
            Some(write_file.clone()),
        ),
        PreparedMutationOp::DeleteRecord {
            record,
            report_file,
        } => ("delete_record", Some(record.clone()), report_file.clone()),
        PreparedMutationOp::FoldedDeleteRecord {
            record,
            write_file,
        } => (
            "delete_record",
            Some(record.clone()),
            Some(write_file.clone()),
        ),
        PreparedMutationOp::Pending { .. } => {
            return Err(DiagnosticSet::one(Diagnostic::error(
                "MUTATION-TXN-INVARIANT",
                "MUTATION",
                "pending operation reached applied mutation reporting",
            )));
        }
    };
    Ok(MutationAppliedOp {
        index: planned.index,
        op: op.to_string(),
        record,
        file,
        outcome,
    })
}

fn failed_op(planned: &PlannedMutationOp, diagnostics: &DiagnosticSet) -> MutationFailedOp {
    MutationFailedOp {
        index: planned.index,
        op: prepared_op_name(&planned.op).to_string(),
        diagnostics: diagnostics.flat_diagnostics(),
    }
}

fn report_without_publish(
    session: &ProjectSession,
    write_ok: bool,
    mut failed: Vec<MutationFailedOp>,
) -> MutationReport {
    failed.sort_by_key(|failure| failure.index);
    MutationReport {
        write_ok,
        check_ok: false,
        applied: Vec::new(),
        failed,
        diagnostics: session.diagnostics.flat_diagnostics(),
    }
}

fn prepared_op_name(op: &PreparedMutationOp) -> &'static str {
    match op {
        PreparedMutationOp::Pending { op } => mutation_op_name(op),
        PreparedMutationOp::InsertRecord { .. } | PreparedMutationOp::CancelledInsert { .. } => {
            "insert_record"
        }
        PreparedMutationOp::SetField { .. } | PreparedMutationOp::FoldedSetField { .. } => {
            "set_field"
        }
        PreparedMutationOp::RenameRecord { .. }
        | PreparedMutationOp::FoldedRenameRecord { .. } => "rename_record",
        PreparedMutationOp::DeleteRecord { .. }
        | PreparedMutationOp::FoldedDeleteRecord { .. } => "delete_record",
    }
}
