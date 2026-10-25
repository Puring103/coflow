use crate::api::{CfdSourceCatalog, DiagnosticSet};
use std::collections::{BTreeSet, HashMap};

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
        if executable[cursor].execution.field_batch_key().is_some() {
            let end = executable[cursor..]
                .iter()
                .position(|item| item.execution.field_batch_key().is_none())
                .map_or(executable.len(), |offset| cursor + offset);
            match stage_independent_field_writes(session, &executable[cursor..end]) {
                Ok(applied) => staged.extend(applied),
                Err(failure) => {
                    failed.push(failure);
                    return report_without_publish(session, false, failed);
                }
            }
            cursor = end;
        } else {
            let item = &executable[cursor];
            match stage_mutation_op(session, &item.planned.op, &item.execution) {
                Ok(outcome) => staged.push(applied_op(&item.planned, outcome)),
                Err(diagnostics) => {
                    failed.push(failed_op(&item.planned, diagnostics));
                    return report_without_publish(session, false, failed);
                }
            }
            cursor += 1;
        }
    }

    // 物理文件可以乱序暂存，语义影响必须按原计划和对应的结果成对计算。
    staged.sort_by_key(|applied| applied.index);
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
    let preloaded_sources = match writer.take_preloaded_sources() {
        Ok(sources) => sources,
        Err(diagnostics) => {
            if let Some(last) = executable.last() {
                failed.push(failed_op(&last.planned, diagnostics));
            }
            return report_without_publish(session, false, failed);
        }
    };
    let rebuilt =
        match rebuild_after_mutation(session, &impact, &source_overrides, preloaded_sources) {
            Ok(rebuilt) => rebuilt,
            Err(diagnostics) => {
                if let Some(last) = executable.last() {
                    failed.push(failed_op(&last.planned, diagnostics));
                }
                return report_without_publish(session, false, failed);
            }
        };
    let new_session = rebuilt.session;
    let mut additional_files = match prepare_additional_files(&new_session, &staged) {
        Ok(files) => files,
        Err(diagnostics) => {
            if let Some(last) = executable.last() {
                failed.push(failed_op(&last.planned, diagnostics));
            }
            return report_without_publish(session, false, failed);
        }
    };
    additional_files.extend(rebuilt.project_file_updates);
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
    *session = new_session;
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

fn stage_independent_field_writes(
    session: &ProjectSession,
    executable: &[ExecutableMutation],
) -> Result<Vec<MutationAppliedOp>, MutationFailedOp> {
    let mut source_groups = HashMap::new();
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for (index, item) in executable.iter().enumerate() {
        let key = item.execution.field_batch_key();
        let group = *source_groups.entry(key).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        groups[group].push(index);
    }
    let mut applied = Vec::with_capacity(executable.len());
    let mut first_failure: Option<(usize, MutationFailedOp)> = None;
    for group in groups {
        if first_failure
            .as_ref()
            .is_some_and(|(index, _)| group[0] >= *index)
        {
            break;
        }
        let batch = group
            .iter()
            .map(|&index| {
                let item = &executable[index];
                (&item.planned.op, &item.execution)
            })
            .collect::<Vec<_>>();
        match crate::writes::stage_field_mutation_batch(session, &batch) {
            Ok(outcomes) => applied.extend(
                group
                    .iter()
                    .zip(outcomes)
                    .map(|(&index, outcome)| applied_op(&executable[index].planned, outcome)),
            ),
            Err(MutationBatchFailure { index, diagnostics }) => {
                let index = group[index.min(group.len() - 1)];
                if first_failure
                    .as_ref()
                    .is_none_or(|(previous, _)| index < *previous)
                {
                    first_failure =
                        Some((index, failed_op(&executable[index].planned, diagnostics)));
                }
            }
        }
    }
    // 文件组彼此独立；检查所有更早的操作所在组，保证仍报告原请求的首个失败。
    first_failure.map_or(Ok(applied), |(_, failure)| Err(failure))
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
    crate::project_path(session.project.root_dir(), path)
}

#[cfg(test)]
mod batch_tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;
    use crate::{CfdPathSegment, CfdValue, MutationOp, MutationValue, Project, RecordCoordinate};
    use std::fs;

    fn fixture() -> (tempfile::TempDir, ProjectSession) {
        let directory = tempfile::tempdir().expect("field batch project");
        fs::create_dir(directory.path().join("data")).expect("data directory");
        fs::write(directory.path().join("coflow.yaml"),
            "schema: schema.cft\ndata: data/\ncodegen:\n  - language: csharp\n    dir: generated/csharp\n",
        ).expect("config");
        fs::write(
            directory.path().join("schema.cft"),
            "table Item { number: int = 0; label: string = \"\"; }\n",
        )
        .expect("schema");
        fs::write(
            directory.path().join("data/a.cfd"),
            "a1: Item { number: 1 }\na2: Item { number: 2 }\n",
        )
        .expect("first source");
        fs::write(
            directory.path().join("data/b.cfd"),
            "b1: Item { number: 3, label: \"before\" }\n",
        )
        .expect("second source");
        let project = Project::open(Some(&directory.path().join("coflow.yaml"))).expect("project");
        let session = crate::session_build::open_project_session(project).expect("session");
        assert!(!session.has_diagnostics());
        (directory, session)
    }

    fn set(record: &str, field: &str, value: CfdValue) -> MutationOp {
        MutationOp::SetField {
            record: RecordCoordinate::try_new("Item", record).expect("coordinate"),
            file: None,
            path: vec![CfdPathSegment::Field(field.to_string())],
            value: MutationValue::Cfd(value),
        }
    }

    fn interleaved_request() -> MutationRequest {
        MutationRequest {
            stop_on_write_error: true,
            ops: vec![
                set("a1", "number", CfdValue::Int(10)),
                set("b1", "number", CfdValue::Int(30)),
                set("a2", "number", CfdValue::Int(20)),
                set("b1", "label", CfdValue::String("after".into())),
            ],
        }
    }

    #[test]
    fn interleaved_files_share_one_field_batch_per_source() {
        let (_directory, session) = fixture();
        let catalog = CfdSourceCatalog::staged_writes();
        let (planned, failed, ok, stopped) = plan_mutations(&session, interleaved_request());
        assert!(ok && failed.is_empty() && !stopped);
        let executable = prepare_execution_plans(&session, &catalog, planned).expect("plans");
        let applied = stage_independent_field_writes(&session, &executable).expect("staging");
        assert_eq!(applied.len(), 4);
        assert_eq!(catalog.writer().field_batch_parse_count(), 4);
        assert_eq!(
            catalog.writer().source_overrides().expect("sources").len(),
            2
        );
    }

    #[test]
    fn interleaved_mutation_returns_original_order_and_correct_generation() {
        let (_directory, mut session) = fixture();
        let report = session.apply_mutation(interleaved_request(), |_, _| Ok(Vec::new()));
        assert!(
            report.write_ok && report.check_ok && report.generation_changed,
            "{report:?}"
        );
        assert_eq!(
            report.applied.iter().map(|op| op.index).collect::<Vec<_>>(),
            [0, 1, 2, 3]
        );
        for (record, value) in [("a1", 10), ("a2", 20), ("b1", 30)] {
            assert_eq!(
                session.field_value("Item", record, &[CfdPathSegment::Field("number".into())]),
                Some(&CfdValue::Int(value))
            );
        }
    }

    #[test]
    fn file_group_failure_uses_earliest_request_index_and_publishes_nothing() {
        let (directory, mut session) = fixture();
        // 同一文件的后续失败先被发现，另一文件更早的失败仍应成为报告结果。
        let first = "a1: Item { number: 1 }\n";
        let second = "# b1 was externally removed\n";
        fs::write(directory.path().join("data/a.cfd"), first).expect("remove a2");
        fs::write(directory.path().join("data/b.cfd"), second).expect("remove b1");
        let report = session.apply_mutation(interleaved_request(), |_, _| Ok(Vec::new()));
        assert!(!report.write_ok && !report.generation_changed);
        assert_eq!(report.failed.len(), 1);
        assert_eq!(report.failed[0].index, 1);
        assert!(report.applied.is_empty());
        assert_eq!(
            fs::read_to_string(directory.path().join("data/a.cfd")).expect("first source"),
            first
        );
        assert_eq!(
            fs::read_to_string(directory.path().join("data/b.cfd")).expect("second source"),
            second
        );
    }

    #[test]
    fn non_field_operations_are_ordering_barriers() {
        let (_directory, mut session) = fixture();
        let request = MutationRequest {
            stop_on_write_error: true,
            ops: vec![
                set("a1", "number", CfdValue::Int(10)),
                MutationOp::UnsetField {
                    record: RecordCoordinate::try_new("Item", "a1").expect("coordinate"),
                    file: None,
                    path: vec![CfdPathSegment::Field("number".into())],
                },
                set("b1", "number", CfdValue::Int(30)),
                set("a1", "number", CfdValue::Int(40)),
            ],
        };
        let report = session.apply_mutation(request, |_, _| Ok(Vec::new()));
        assert!(report.write_ok && report.check_ok, "{report:?}");
        assert_eq!(
            session.field_value("Item", "a1", &[CfdPathSegment::Field("number".into())]),
            Some(&CfdValue::Int(40))
        );
        assert_eq!(
            report.applied.iter().map(|op| op.index).collect::<Vec<_>>(),
            [0, 1, 2, 3]
        );
    }
}
