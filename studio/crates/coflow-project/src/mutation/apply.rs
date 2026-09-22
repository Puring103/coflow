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
        request: MutationRequest,
        prepare_additional_files: F,
    ) -> MutationReport
    where
        F: FnOnce(&Self, &[MutationAppliedOp]) -> Result<Vec<ProjectFileUpdate>, DiagnosticSet>,
    {
        let (planned, mut failed, write_ok, stopped) = plan_mutations(self, request);
        if stopped || planned.is_empty() {
            return report_without_publish(self, write_ok, failed);
        }

        let staged_catalog = CfdSourceCatalog::staged_writes();
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
    F: FnOnce(
        &ProjectSession,
        &[MutationAppliedOp],
    ) -> Result<Vec<ProjectFileUpdate>, DiagnosticSet>,
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
                Err(MutationBatchFailure { index, diagnostics }) => {
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
    let rebuilt = match rebuild_after_mutation(session, &impact, &source_overrides) {
        Ok(rebuilt) => rebuilt,
        Err(diagnostics) => {
            if let Some(last) = executable.last() {
                failed.push(failed_op(&last.planned, diagnostics));
            }
            return report_without_publish(session, false, failed);
        }
    };
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
    // 发布成功后统一失效共享磁盘基线，CLI、编辑器和 LSP 使用相同规则。
    for file in &written_files {
        session
            .project
            .source_store()
            .invalidate(&session.project.root_dir().join(file));
    }
    let changed_records = changed_records(session, &new_session, &affected_files);
    *session = new_session;
    staged.sort_by_key(|applied| applied.index);
    failed.sort_by_key(|failure| failure.index);
    let check_ok = write_ok
        && diagnostics
            .iter()
            .all(|diagnostic| diagnostic.severity != "error");
    MutationReport {
        changed_records,
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
        changed_records: Default::default(),
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
        changed_records: Default::default(),
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
    crate::project_path(session.project.root_dir(), path)
}

/// 比较完整发布结果，因此引用、维度和诊断变化不局限于写入目标文件。
fn changed_records(
    previous: &ProjectSession,
    next: &ProjectSession,
    affected: &[String],
) -> std::collections::BTreeMap<String, Vec<crate::RecordCoordinate>> {
    let before = crate::ProjectQueries::new(previous, 0);
    let after = crate::ProjectQueries::new(next, 0);
    let mut changes = affected
        .iter()
        .cloned()
        .map(|file| (file, BTreeSet::new()))
        .collect::<std::collections::BTreeMap<_, _>>();
    // 同一坐标只比较一次；移动记录同时使旧文件和新文件进入变更集。
    let mut compared = BTreeSet::new();
    for queries in [before, after] {
        for file in queries.source_files() {
            for view in queries.record_views_in_file(file) {
                if !compared.insert(view.coordinate.clone()) {
                    continue;
                }
                let old = before.record_view(&view.coordinate.actual_type, &view.coordinate.key);
                let new = after.record_view(&view.coordinate.actual_type, &view.coordinate.key);
                let unchanged = old.as_ref().zip(new.as_ref()).is_some_and(|(old, new)| {
                    old.display_path == new.display_path
                        && old.record.object == new.record.object
                        && old.record.dimension_fields == new.record.dimension_fields
                });
                if !unchanged {
                    for changed in old.iter().chain(new.iter()) {
                        changes
                            .entry(changed.display_path.to_string())
                            .or_default()
                            .insert(view.coordinate.clone());
                    }
                }
            }
        }
    }
    // 仅更新诊断实际变化的行，同时包含已消失的诊断以清除旧标记。
    let old_diagnostics = record_diagnostics(before);
    let new_diagnostics = record_diagnostics(after);
    let keys = old_diagnostics
        .keys()
        .chain(new_diagnostics.keys())
        .collect::<BTreeSet<_>>();
    for key in keys {
        if old_diagnostics.get(key) != new_diagnostics.get(key) {
            changes
                .entry(key.0.clone())
                .or_default()
                .insert(key.1.clone());
        }
    }
    changes
        .into_iter()
        .map(|(file, records)| (file, records.into_iter().collect()))
        .collect()
}

fn record_diagnostics(
    queries: crate::ProjectQueries<'_>,
) -> std::collections::BTreeMap<(String, crate::RecordCoordinate), Vec<crate::FlatDiagnostic>> {
    let mut records = std::collections::BTreeMap::<_, Vec<_>>::new();
    for diagnostic in queries.diagnostics().flat_diagnostics() {
        let (file_path, coordinate) = match &diagnostic.target {
            crate::DiagnosticTarget::Record {
                file_path,
                coordinate,
            }
            | crate::DiagnosticTarget::TableField {
                file_path,
                coordinate,
                ..
            } => (file_path, coordinate),
            _ => continue,
        };
        let file = queries
            .file_for_record(&coordinate.actual_type, &coordinate.key)
            .unwrap_or(file_path);
        records
            .entry((file.to_string(), coordinate.clone()))
            .or_default()
            .push(diagnostic);
    }
    for diagnostics in records.values_mut() {
        diagnostics.sort_by(|a, b| a.id.cmp(&b.id));
    }
    records
}
