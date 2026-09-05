use crate::api::{CfdSourceCatalog, DiagnosticSet};
use std::collections::BTreeSet;

use crate::writes::{
    prepare_mutation_execution, rebuild_after_mutation, stage_mutation_op, MutationBatchFailure,
    MutationExecutionPlan, MutationImpact,
};
use crate::ProjectSession;

use super::plan::{plan_mutations, PlannedMutationOp};
use super::{
    MutationAppliedOp, MutationFailedOp, MutationReport, MutationRequest, ProjectFileUpdate,
};

struct ExecutableMutation {
    planned: PlannedMutationOp,
    execution: MutationExecutionPlan,
}

impl ProjectSession {
    /// Prepare, stage, and atomically publish a mutation request.
    pub fn apply_mutation<F>(
        &mut self,
        catalog: &CfdSourceCatalog,
        request: MutationRequest,
        prepare_additional_files: F,
    ) -> MutationReport
    where
        F: FnOnce(&ProjectSession, &[MutationAppliedOp]) -> Result<Vec<ProjectFileUpdate>, DiagnosticSet>,
    {
        let (planned, mut failed, write_ok, stopped) = plan_mutations(self, request);
        if stopped || planned.is_empty() {
            return report_without_publish(self, write_ok, failed);
        }

        let staged_catalog = catalog.staged_writes();
        let executable = match prepare_execution_plans(self, &staged_catalog, planned) {
            Ok(executable) => executable,
            Err(failure) => {
                failed.push(failure);
                return report_without_publish(self, false, failed);
            }
        };

        if executable
            .iter()
            .all(|item| !item.execution.changes_generation())
        {
            return stage_without_generation(self, write_ok, failed, &executable);
        }

        execute_generation_mutation(
            self,
            &staged_catalog,
            write_ok,
            failed,
            &executable,
            prepare_additional_files,
        )
    }
}

#[allow(clippy::too_many_lines)]
fn execute_generation_mutation<F>(
    session: &mut ProjectSession,
    catalog: &CfdSourceCatalog,
    write_ok: bool,
    mut failed: Vec<MutationFailedOp>,
    executable: &[ExecutableMutation],
    prepare_additional_files: F,
) -> MutationReport
where
    F: FnOnce(&ProjectSession, &[MutationAppliedOp]) -> Result<Vec<ProjectFileUpdate>, DiagnosticSet>,
{
    let mut staged = Vec::with_capacity(executable.len());
    let mut cursor = 0;
    while cursor < executable.len() {
        let mut end = cursor + 1;
        while end < executable.len()
            && executable[cursor]
                .execution
                .can_batch_field_write_with(&executable[end].execution)
        {
            end += 1;
        }
        if end - cursor > 1 {
            let batch = executable[cursor..end]
                .iter()
                .map(|item| (&item.planned.op, &item.execution))
                .collect::<Vec<_>>();
            match crate::writes::stage_field_mutation_batch(session, &batch) {
                Ok(outcomes) => staged.extend(
                    executable[cursor..end]
                        .iter()
                        .zip(outcomes)
                        .map(|(item, outcome)| applied_op(&item.planned, outcome)),
                ),
                Err(MutationBatchFailure {
                    index,
                    diagnostics,
                }) => {
                    let failed_item = &executable[cursor + index.min(end - cursor - 1)];
                    failed.push(failed_op(&failed_item.planned, diagnostics));
                    return report_without_publish(session, false, failed);
                }
            }
        } else {
            let item = &executable[cursor];
            match stage_mutation_op(session, &item.planned.op, &item.execution) {
                Ok(outcome) => staged.push(applied_op(&item.planned, outcome)),
                Err(diagnostics) => {
                    failed.push(failed_op(&item.planned, diagnostics));
                    return report_without_publish(session, false, failed);
                }
            }
        }
        cursor = end;
    }

    let impact = MutationImpact::from_operations(
        executable
            .iter()
            .zip(&staged)
            .map(|(item, applied)| (&item.planned.op, &applied.outcome)),
    );
    let writer = catalog.writer();
    let source_overrides = match writer.source_overrides() {
        Ok(source_overrides) => source_overrides,
        Err(diagnostics) => {
            if let Some(last) = executable.last() {
                failed.push(failed_op(&last.planned, diagnostics));
            }
            return report_without_publish(session, false, failed);
        }
    };
    let rebuilt = match rebuild_after_mutation(session, catalog, &impact, &source_overrides) {
        Ok(rebuilt) => rebuilt,
        Err(diagnostics) => {
            if let Some(last) = executable.last() {
                failed.push(failed_op(&last.planned, diagnostics));
            }
            return report_without_publish(session, false, failed);
        }
    };
    let changed_dimension_files = rebuilt
        .changed_dimension_paths
        .iter()
        .map(|path| {
            path.strip_prefix(session.project.root_dir()).map_or_else(
                |_| path.display().to_string(),
                crate::project::path_to_slash,
            )
        })
        .collect::<Vec<_>>();
    let new_session = rebuilt.session;
    let additional_files = match prepare_additional_files(&new_session, &staged) {
        Ok(files) => files,
        Err(diagnostics) => {
            if let Some(last) = executable.last() {
                failed.push(failed_op(&last.planned, diagnostics));
            }
            return report_without_publish(session, false, failed);
        }
    };
    let additional_paths = additional_files
        .iter()
        .map(|update| project_display_path(session, update.path()))
        .collect::<Vec<_>>();
    if let Err(diagnostics) = writer.add_project_file_updates(additional_files) {
        if let Some(last) = executable.last() {
            failed.push(failed_op(&last.planned, diagnostics));
        }
        return report_without_publish(session, false, failed);
    }

    let affected_files = impact
        .affected_files
        .into_iter()
        .chain(changed_dimension_files)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let written_files = affected_files
        .iter()
        .cloned()
        .chain(additional_paths)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let mut diagnostics = staged
        .iter()
        .flat_map(|applied| applied.outcome.diagnostics.flat_diagnostics())
        .collect::<Vec<_>>();
    diagnostics.extend(new_session.diagnostics.flat_diagnostics());
    if let Err(diagnostics) = writer.publish() {
        if let Some(last) = executable.last() {
            failed.push(failed_op(&last.planned, diagnostics));
        }
        return report_without_publish(session, false, failed);
    }
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
        written_files,
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
        written_files: Vec::new(),
        diagnostics,
    }
}

fn prepare_execution_plans(
    session: &ProjectSession,
    catalog: &CfdSourceCatalog,
    planned: Vec<PlannedMutationOp>,
) -> Result<Vec<ExecutableMutation>, MutationFailedOp> {
    let allow_noop = planned.len() == 1;
    planned
        .into_iter()
        .map(|planned| {
            match prepare_mutation_execution(session, catalog, &planned.op, allow_noop) {
                Ok(execution) => Ok(ExecutableMutation { planned, execution }),
                Err(diagnostics) => Err(failed_op(&planned, diagnostics)),
            }
        })
        .collect()
}

fn applied_op(planned: &PlannedMutationOp, outcome: crate::WriteOutcome) -> MutationAppliedOp {
    let (op, record, file) = planned.op.report_metadata();
    MutationAppliedOp {
        index: planned.index,
        op: op.to_string(),
        record,
        file,
        outcome,
    }
}

fn failed_op(planned: &PlannedMutationOp, diagnostics: DiagnosticSet) -> MutationFailedOp {
    MutationFailedOp::from_diagnostics(planned.index, planned.op.report_metadata().0, diagnostics)
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
        written_files: Vec::new(),
        diagnostics: session.diagnostics.flat_diagnostics(),
    }
}

fn project_display_path(session: &ProjectSession, path: &std::path::Path) -> String {
    path.strip_prefix(session.project.root_dir()).map_or_else(
        |_| path.display().to_string().replace('\\', "/"),
        crate::project::path_to_slash,
    )
}
