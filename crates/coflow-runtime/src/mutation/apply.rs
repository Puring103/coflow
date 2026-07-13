use coflow_api::{DiagnosticSet, ProviderRegistry, Severity, WriteContext};
use std::collections::BTreeSet;

use crate::writes::{
    preflight_mutation_op, prepare_mutation_execution, rebuild_after_mutation, stage_mutation_op,
    MutationExecutionPlan, MutationImpact, MutationTransaction,
};
use crate::{ProjectSession, RecordCoordinate};

use super::plan::{plan_mutations, PlannedMutationOp};
use super::types::PreparedMutationOp;
use super::{MutationAppliedOp, MutationFailedOp, MutationReport, MutationRequest};

struct ExecutableMutation {
    planned: PlannedMutationOp,
    execution: MutationExecutionPlan,
}

impl ProjectSession {
    /// Prepare, stage, and atomically publish a mutation request.
    pub fn apply_mutation(
        &mut self,
        registry: &ProviderRegistry,
        request: MutationRequest,
    ) -> MutationReport {
        let (planned, mut failed, write_ok, stopped) = plan_mutations(self, request);
        if stopped || planned.is_empty() {
            return report_without_publish(self, write_ok, failed);
        }

        let executable = match prepare_execution_plans(self, registry, planned) {
            Ok(executable) => executable,
            Err(failure) => {
                failed.push(failure);
                return report_without_publish(self, false, failed);
            }
        };

        for item in &executable {
            if let Err(diagnostics) = preflight_mutation_op(self, &item.planned.op, &item.execution)
            {
                failed.push(failed_op(&item.planned, diagnostics));
                return report_without_publish(self, false, failed);
            }
        }

        if executable
            .iter()
            .all(|item| !item.execution.changes_generation())
        {
            return stage_without_generation(self, write_ok, failed, &executable);
        }

        execute_generation_mutation(self, registry, write_ok, failed, &executable)
    }
}

fn execute_generation_mutation(
    session: &mut ProjectSession,
    registry: &ProviderRegistry,
    write_ok: bool,
    mut failed: Vec<MutationFailedOp>,
    executable: &[ExecutableMutation],
) -> MutationReport {
    let compiled_schema = session.compiled_schema();
    let ctx = WriteContext {
        project_root: &session.project.root_dir,
        schema: compiled_schema,
        model: Some(&session.model),
    };
    let transaction =
        match MutationTransaction::begin(ctx, executable.iter().map(|item| &item.execution)) {
            Ok(transaction) => transaction,
            Err(diagnostics) => {
                if let Some(first) = executable.first() {
                    failed.push(failed_op(&first.planned, diagnostics));
                }
                return report_without_publish(session, false, failed);
            }
        };

    let mut staged = Vec::with_capacity(executable.len());
    for item in executable {
        match stage_mutation_op(session, &item.planned.op, &item.execution) {
            Ok(outcome) => staged.push(applied_op(&item.planned, outcome)),
            Err(mut diagnostics) => {
                transaction.compensate_into(&mut diagnostics);
                failed.push(failed_op(&item.planned, diagnostics));
                return report_without_publish(session, false, failed);
            }
        }
    }

    let impact = MutationImpact::from_outcomes(staged.iter().map(|applied| &applied.outcome));
    let rebuilt = match rebuild_after_mutation(session, registry, &impact) {
        Ok(rebuilt) => rebuilt,
        Err(mut diagnostics) => {
            transaction.compensate_into(&mut diagnostics);
            if let Some(last) = executable.last() {
                failed.push(failed_op(&last.planned, diagnostics));
            }
            return report_without_publish(session, false, failed);
        }
    };
    let new_session = rebuilt.session;
    let mut rebuild_diagnostics = blocking_rebuild_diagnostics(&new_session);
    if !rebuild_diagnostics.is_empty() {
        transaction.compensate_into(&mut rebuild_diagnostics);
        if let Some(last) = executable.last() {
            failed.push(failed_op(&last.planned, rebuild_diagnostics));
        }
        return report_without_publish(session, false, failed);
    }

    if let Err(diagnostics) = transaction.commit() {
        if let Some(last) = executable.last() {
            failed.push(failed_op(&last.planned, diagnostics));
        }
        return report_without_publish(session, false, failed);
    }

    let affected_files = impact
        .affected_files
        .into_iter()
        .chain(rebuilt.changed_dimension_files)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let mut diagnostics = staged
        .iter()
        .flat_map(|applied| applied.outcome.diagnostics.flat_diagnostics())
        .collect::<Vec<_>>();
    diagnostics.extend(new_session.diagnostics.flat_diagnostics());
    *session = new_session;
    staged.sort_by_key(|applied| applied.index);
    failed.sort_by_key(|failure| failure.index);
    let check_ok = write_ok
        && diagnostics
            .iter()
            .all(|diagnostic| diagnostic.severity != "error");
    MutationReport {
        write_ok,
        check_ok,
        generation_changed: true,
        applied: staged,
        failed,
        affected_files,
        diagnostics,
    }
}

fn stage_without_generation(
    session: &ProjectSession,
    write_ok: bool,
    mut failed: Vec<MutationFailedOp>,
    executable: &[ExecutableMutation],
) -> MutationReport {
    let mut applied = Vec::with_capacity(executable.len());
    for item in executable {
        match stage_mutation_op(session, &item.planned.op, &item.execution) {
            Ok(outcome) => applied.push(applied_op(&item.planned, outcome)),
            Err(diagnostics) => failed.push(failed_op(&item.planned, diagnostics)),
        }
    }
    applied.sort_by_key(|item| item.index);
    failed.sort_by_key(|item| item.index);
    let mut diagnostics = applied
        .iter()
        .flat_map(|item| item.outcome.diagnostics.flat_diagnostics())
        .collect::<Vec<_>>();
    diagnostics.extend(session.diagnostics.flat_diagnostics());
    let check_ok = write_ok
        && failed.is_empty()
        && diagnostics
            .iter()
            .all(|diagnostic| diagnostic.severity != "error");
    MutationReport {
        write_ok: write_ok && failed.is_empty(),
        check_ok,
        generation_changed: false,
        applied,
        failed,
        affected_files: Vec::new(),
        diagnostics,
    }
}

fn prepare_execution_plans(
    session: &ProjectSession,
    registry: &ProviderRegistry,
    planned: Vec<PlannedMutationOp>,
) -> Result<Vec<ExecutableMutation>, MutationFailedOp> {
    planned
        .into_iter()
        .map(
            |planned| match prepare_mutation_execution(session, registry, &planned.op) {
                Ok(execution) => Ok(ExecutableMutation { planned, execution }),
                Err(diagnostics) => Err(failed_op(&planned, diagnostics)),
            },
        )
        .collect()
}

fn blocking_rebuild_diagnostics(session: &ProjectSession) -> DiagnosticSet {
    session
        .diagnostics
        .as_set()
        .iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Error && diagnostic.stage != "CHECK")
        .cloned()
        .collect::<Vec<_>>()
        .into()
}

fn applied_op(planned: &PlannedMutationOp, outcome: crate::WriteOutcome) -> MutationAppliedOp {
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
        PreparedMutationOp::CancelledInsert { record, write_file } => (
            "insert_record",
            Some(record.clone()),
            Some(write_file.clone()),
        ),
        PreparedMutationOp::SetField {
            record, write_file, ..
        }
        | PreparedMutationOp::FoldedSetField { record, write_file } => {
            ("set_field", Some(record.clone()), Some(write_file.clone()))
        }
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
        PreparedMutationOp::FoldedDeleteRecord { record, write_file } => (
            "delete_record",
            Some(record.clone()),
            Some(write_file.clone()),
        ),
    };
    MutationAppliedOp {
        index: planned.index,
        op: op.to_string(),
        record,
        file,
        outcome,
    }
}

fn failed_op(planned: &PlannedMutationOp, diagnostics: DiagnosticSet) -> MutationFailedOp {
    MutationFailedOp::from_diagnostics(planned.index, prepared_op_name(&planned.op), diagnostics)
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
        generation_changed: false,
        applied: Vec::new(),
        failed,
        affected_files: Vec::new(),
        diagnostics: session.diagnostics.flat_diagnostics(),
    }
}

const fn prepared_op_name(op: &PreparedMutationOp) -> &'static str {
    match op {
        PreparedMutationOp::InsertRecord { .. } | PreparedMutationOp::CancelledInsert { .. } => {
            "insert_record"
        }
        PreparedMutationOp::SetField { .. } | PreparedMutationOp::FoldedSetField { .. } => {
            "set_field"
        }
        PreparedMutationOp::RenameRecord { .. } | PreparedMutationOp::FoldedRenameRecord { .. } => {
            "rename_record"
        }
        PreparedMutationOp::DeleteRecord { .. } | PreparedMutationOp::FoldedDeleteRecord { .. } => {
            "delete_record"
        }
    }
}
