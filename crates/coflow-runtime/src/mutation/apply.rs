use crate::api::{CfdSourceCatalog, DiagnosticSet};
use std::collections::{BTreeSet, HashMap};

use crate::writes::{
    prepare_mutation_execution, rebuild_after_mutation, stage_mutation_op, ExecutableMutationOp,
    FieldWrite, MutationBatchFailure, MutationImpact,
};
use crate::ProjectSession;

use super::plan::{plan_mutations, PlannedMutationOp};
use super::{
    MutationAppliedOp, MutationFailedOp, MutationReport, MutationRequest, ProjectFileUpdate,
};

struct ExecutableMutation {
    index: usize,
    op_name: &'static str,
    record: Option<crate::RecordCoordinate>,
    file: Option<String>,
    impact: MutationImpact,
    execution: ExecutableMutationOp,
}

impl ProjectSession {
    /// Prepare, stage, and atomically publish a mutation request.
    #[tracing::instrument(skip_all, fields(operation = "mutation", op_count = request.ops.len()))]
    pub fn apply_mutation<F>(
        &mut self,
        request: MutationRequest,
        prepare_additional_files: F,
    ) -> MutationReport
    where
        F: FnOnce(&Self, &[MutationAppliedOp]) -> Result<Vec<ProjectFileUpdate>, DiagnosticSet>,
    {
        tracing::info!(event = "mutation_plan_started");
        if request.ops.iter().any(|op| {
            matches!(op, super::MutationOp::RestoreRecord { .. })
                || (request.ops.len() > 1 && changes_record_identity(op))
        }) && !request.ops.iter().any(|op| {
            matches!(
                op,
                super::MutationOp::SwapRecords { .. }
                    | super::MutationOp::MoveRecord { .. }
                    | super::MutationOp::TransferRecord { .. }
            )
        }) {
            return execute_structural_sequence(self, request, prepare_additional_files);
        }
        let (planned, mut failed, write_ok, stopped) = plan_mutations(self, request);
        if stopped || planned.is_empty() {
            return report_without_publish(self, write_ok, failed);
        }

        let staged_catalog = CfdSourceCatalog::staged_writes(std::sync::Arc::clone(&self.schema));
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
    let staged = match stage_executable(session, catalog, executable) {
        Ok(staged) => staged,
        Err(failure) => {
            failed.push(failure);
            return report_without_publish(session, false, failed);
        }
    };
    let impact = mutation_impact(executable, &staged);
    let writer = catalog.writer();
    let rebuilt = match writer.source_overrides().and_then(|overrides| {
        let preloaded = writer.take_preloaded_sources()?;
        rebuild_after_mutation(session, &impact, &overrides, preloaded)
    }) {
        Ok(rebuilt) => rebuilt,
        Err(diagnostics) => {
            if let Some(last) = executable.last() {
                failed.push(failed_op(last, diagnostics));
            }
            return report_without_publish(session, false, failed);
        }
    };
    publish_staged(
        session,
        catalog,
        write_ok,
        failed,
        executable,
        staged,
        impact,
        rebuilt,
        prepare_additional_files,
    )
}

fn stage_executable(
    session: &ProjectSession,
    catalog: &CfdSourceCatalog,
    executable: &[ExecutableMutation],
) -> Result<Vec<MutationAppliedOp>, MutationFailedOp> {
    let mut staged = Vec::with_capacity(executable.len());
    let mut cursor = 0;
    while cursor < executable.len() {
        if executable[cursor].execution.field_write().is_some() {
            let end = executable[cursor..]
                .iter()
                .position(|item| item.execution.field_write().is_none())
                .map_or(executable.len(), |offset| cursor + offset);
            let fields = executable[cursor..end]
                .iter()
                .filter_map(|item| item.execution.field_write().map(|write| (item, write)))
                .collect::<Vec<_>>();
            match stage_independent_field_writes(session, &fields) {
                Ok(applied) => staged.extend(applied),
                Err(failure) => {
                    return Err(failure);
                }
            }
            cursor = end;
        } else {
            let item = &executable[cursor];
            match stage_mutation_op(session, &item.execution) {
                Ok(outcome) => {
                    let mut applied = applied_op(item, outcome);
                    if let Some(record) = item.execution.deleted_record() {
                        match catalog
                            .writer()
                            .take_record_restore(record, session.project.root_dir())
                        {
                            Ok(snapshot) => applied.restore = Some(snapshot),
                            Err(diagnostics) => {
                                return Err(failed_op(item, diagnostics));
                            }
                        }
                    }
                    staged.push(applied);
                }
                Err(diagnostics) => {
                    return Err(failed_op(item, diagnostics));
                }
            }
            cursor += 1;
        }
    }

    staged.sort_by_key(|applied| applied.index);
    Ok(staged)
}

fn mutation_impact(
    executable: &[ExecutableMutation],
    staged: &[MutationAppliedOp],
) -> MutationImpact {
    // 物理文件可以乱序暂存，语义影响按原计划和对应结果成对计算。
    MutationImpact::from_operations(
        executable
            .iter()
            .zip(staged)
            .map(|(item, applied)| (&item.impact, &applied.outcome)),
    )
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn publish_staged<F>(
    session: &mut ProjectSession,
    catalog: &CfdSourceCatalog,
    write_ok: bool,
    mut failed: Vec<MutationFailedOp>,
    executable: &[ExecutableMutation],
    staged: Vec<MutationAppliedOp>,
    impact: MutationImpact,
    rebuilt: crate::sessions::session_build::SessionBuildOutput,
    prepare_additional_files: F,
) -> MutationReport
where
    F: FnOnce(
        &ProjectSession,
        &[MutationAppliedOp],
    ) -> Result<Vec<ProjectFileUpdate>, DiagnosticSet>,
{
    let writer = catalog.writer();
    let new_session = rebuilt.session;
    let mut additional_files = match prepare_additional_files(&new_session, &staged) {
        Ok(files) => files,
        Err(diagnostics) => {
            if let Some(last) = executable.last() {
                failed.push(failed_op(last, diagnostics));
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
            failed.push(failed_op(last, diagnostics));
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
    tracing::info!(
        event = "mutation_publish_started",
        file_count = written_files.len()
    );
    let published_files = match writer.publish() {
        Ok(files) => files,
        Err(diagnostics) => {
            if let Some(last) = executable.last() {
                failed.push(failed_op(last, diagnostics));
            }
            return report_without_publish(session, false, failed);
        }
    };
    *session = new_session;
    tracing::warn!(
        event = "mutation_not_published",
        failure_count = failed.len(),
        write_ok
    );
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
        published_files,
        diagnostics,
    }
}

fn changes_record_identity(op: &super::MutationOp) -> bool {
    match op {
        super::MutationOp::RenameRecord { .. } => true,
        super::MutationOp::SetField { path, .. } => {
            matches!(path.as_slice(), [crate::CfdPathSegment::Field(field)] if field == "id")
        }
        _ => false,
    }
}

/// 恢复与改名段推进内存候选模型，使后续操作使用当时的记录身份；连续恢复共用一次重建，最终只发布一次。
#[allow(clippy::too_many_lines)]
fn execute_structural_sequence<F>(
    session: &mut ProjectSession,
    request: MutationRequest,
    prepare_additional_files: F,
) -> MutationReport
where
    F: FnOnce(
        &ProjectSession,
        &[MutationAppliedOp],
    ) -> Result<Vec<ProjectFileUpdate>, DiagnosticSet>,
{
    let catalog = CfdSourceCatalog::staged_writes(std::sync::Arc::clone(&session.schema));
    let writer = catalog.writer();
    let mut operations = request.ops.into_iter().peekable();
    let mut candidate: Option<crate::sessions::session_build::SessionBuildOutput> = None;
    let mut executable = Vec::new();
    let mut applied = Vec::new();
    let mut failed = Vec::new();
    let mut write_ok = true;
    let mut offset = 0;
    let mut project_updates = std::collections::BTreeMap::new();
    let mut total_impact = MutationImpact::default();
    while let Some(first) = operations.next() {
        let restore = matches!(first, super::MutationOp::RestoreRecord { .. });
        let identity_change = changes_record_identity(&first);
        let mut group = vec![first];
        while !identity_change
            && operations.peek().is_some_and(|op| {
                !changes_record_identity(op)
                    && matches!(op, super::MutationOp::RestoreRecord { .. }) == restore
            })
        {
            if let Some(next) = operations.next() {
                group.push(next);
            }
        }
        let count = group.len();
        let current = candidate
            .as_ref()
            .map_or(&*session, |output| &output.session);
        let (mut planned, mut group_failed, group_ok, stopped) = plan_mutations(
            current,
            MutationRequest {
                stop_on_write_error: request.stop_on_write_error,
                ops: group,
            },
        );
        for op in &mut planned {
            op.index += offset;
        }
        for failure in &mut group_failed {
            failure.index += offset;
        }
        offset += count;
        failed.extend(group_failed);
        write_ok &= group_ok;
        if stopped {
            return report_without_publish(session, false, failed);
        }
        if planned.is_empty() {
            continue;
        }
        let group_execution = match prepare_execution_plans(current, &catalog, planned) {
            Ok(execution) => execution,
            Err(failure) => {
                failed.push(failure);
                return report_without_publish(session, false, failed);
            }
        };
        let group_applied = match stage_executable(current, &catalog, &group_execution) {
            Ok(applied) => applied,
            Err(failure) => {
                failed.push(failure);
                return report_without_publish(session, false, failed);
            }
        };
        let impact = mutation_impact(&group_execution, &group_applied);
        total_impact.merge(&impact);
        let rebuilt = writer.source_overrides().and_then(|overrides| {
            let preloaded = writer.take_preloaded_sources()?;
            rebuild_after_mutation(current, &impact, &overrides, preloaded)
        });
        match rebuilt {
            Ok(mut output) => {
                for update in std::mem::take(&mut output.project_file_updates) {
                    // 多段都扩展 data 配置时，发布前置条件始终针对事务开始时的磁盘内容。
                    match project_updates.entry(update.path().to_path_buf()) {
                        std::collections::btree_map::Entry::Vacant(entry) => {
                            entry.insert(update);
                        }
                        std::collections::btree_map::Entry::Occupied(mut entry) => {
                            let pending: &mut ProjectFileUpdate = entry.get_mut();
                            pending.contents = update.contents;
                        }
                    }
                }
                candidate = Some(output);
            }
            Err(diagnostics) => {
                if let Some(last) = group_execution.last() {
                    failed.push(failed_op(last, diagnostics));
                }
                return report_without_publish(session, false, failed);
            }
        }
        executable.extend(group_execution);
        applied.extend(group_applied);
    }
    let Some(mut rebuilt) = candidate else {
        return report_without_publish(session, write_ok, failed);
    };
    rebuilt.project_file_updates = project_updates.into_values().collect();
    publish_staged(
        session,
        &catalog,
        write_ok,
        failed,
        &executable,
        applied,
        total_impact,
        rebuilt,
        prepare_additional_files,
    )
}

fn stage_independent_field_writes(
    session: &ProjectSession,
    executable: &[(&ExecutableMutation, &FieldWrite)],
) -> Result<Vec<MutationAppliedOp>, MutationFailedOp> {
    let mut source_groups = HashMap::new();
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for (index, (_, write)) in executable.iter().enumerate() {
        let group = *source_groups.entry(write.batch_key()).or_insert_with(|| {
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
            .map(|&index| executable[index].1)
            .collect::<Vec<_>>();
        match crate::writes::stage_field_mutation_batch(session, &batch) {
            Ok(outcomes) => applied.extend(
                group
                    .iter()
                    .zip(outcomes)
                    .map(|(&index, outcome)| applied_op(executable[index].0, outcome)),
            ),
            Err(MutationBatchFailure { index, diagnostics }) => {
                let index = group[index.min(group.len() - 1)];
                if first_failure
                    .as_ref()
                    .is_none_or(|(previous, _)| index < *previous)
                {
                    first_failure = Some((index, failed_op(executable[index].0, diagnostics)));
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
        match stage_mutation_op(session, &item.execution) {
            Ok(outcome) => applied.push(applied_op(item, outcome)),
            Err(diagnostics) => failed.push(failed_op(item, diagnostics)),
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
        published_files: Vec::new(),
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
            let index = planned.index;
            let (op_name, record, file) = planned.op.report_metadata();
            let impact = MutationImpact::for_operation(&planned.op);
            let execution = prepare_mutation_execution(session, catalog, planned.op, allow_noop)
                .map_err(|diagnostics| {
                    MutationFailedOp::from_diagnostics(index, op_name, diagnostics)
                })?;
            Ok(ExecutableMutation {
                index,
                op_name,
                record,
                file,
                impact,
                execution,
            })
        })
        .collect()
}

fn applied_op(item: &ExecutableMutation, outcome: crate::WriteOutcome) -> MutationAppliedOp {
    MutationAppliedOp {
        restore: None,
        index: item.index,
        op: item.op_name.to_string(),
        record: item.record.clone(),
        file: item.file.clone(),
        outcome,
    }
}

fn failed_op(item: &ExecutableMutation, diagnostics: DiagnosticSet) -> MutationFailedOp {
    MutationFailedOp::from_diagnostics(item.index, item.op_name, diagnostics)
}

fn report_without_publish(
    session: &ProjectSession,
    write_ok: bool,
    mut failed: Vec<MutationFailedOp>,
) -> MutationReport {
    tracing::warn!(
        event = "mutation_not_published",
        failure_count = failed.len(),
        write_ok
    );
    failed.sort_by_key(|failure| failure.index);
    let written_files = failed
        .iter()
        .filter_map(MutationFailedOp::publication)
        .flat_map(|report| report.written_files.iter().cloned())
        .collect();
    MutationReport {
        write_ok,
        check_ok: false,
        generation_changed: false,
        applied: Vec::new(),
        failed,
        affected_files: Vec::new(),
        written_files,
        published_files: Vec::new(),
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
        fixture_with_schema("table Item { number: int = 0; label: string = \"\"; }\n")
    }

    fn fixture_with_schema(schema: &str) -> (tempfile::TempDir, ProjectSession) {
        let directory = tempfile::tempdir().expect("field batch project");
        fs::create_dir(directory.path().join("data")).expect("data directory");
        fs::write(directory.path().join("coflow.yaml"),
            "schema: schema.cft\ndata: data/\ncodegen:\n  - language: csharp\n    dir: generated/csharp\n",
        ).expect("config");
        fs::write(directory.path().join("schema.cft"), schema).expect("schema");
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
        let session =
            crate::sessions::session_build::open_project_session(project).expect("session");
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
        let catalog = CfdSourceCatalog::staged_writes(std::sync::Arc::clone(&session.schema));
        let (planned, failed, ok, stopped) = plan_mutations(&session, interleaved_request());
        assert!(ok && failed.is_empty() && !stopped);
        let executable = prepare_execution_plans(&session, &catalog, planned).expect("plans");
        let applied = stage_executable(&session, &catalog, &executable).expect("staging");
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
    #[test]
    fn existing_record_can_reference_a_pending_insert_before_rename() {
        let (_directory, mut session) = fixture_with_schema(
            "table Item { number: int = 0; label: string = \"\"; target: [&Item] = []; }",
        );
        let report = session.apply_mutation(
            MutationRequest {
                stop_on_write_error: true,
                ops: vec![
                    MutationOp::InsertRecord {
                        file: "data/a.cfd".into(),
                        actual_type: "Item".into(),
                        id: "new".into(),
                        fields: crate::MutationFields::Empty,
                        materialization: crate::DefaultMaterialization::Minimal,
                    },
                    set(
                        "a1",
                        "target",
                        CfdValue::Array(vec![CfdValue::Ref("new".try_into().expect("id"))]),
                    ),
                    MutationOp::RenameRecord {
                        record: RecordCoordinate::try_new("Item", "new").expect("coordinate"),
                        file: None,
                        new_id: "renamed".into(),
                    },
                ],
            },
            |_, _| Ok(Vec::new()),
        );
        assert!(
            report.write_ok && report.check_ok && report.generation_changed,
            "{report:?}"
        );
        assert_eq!(
            report.applied.iter().map(|op| op.index).collect::<Vec<_>>(),
            [0, 1, 2]
        );
        assert_eq!(
            session.field_value("Item", "a1", &[CfdPathSegment::Field("target".into())]),
            Some(&CfdValue::Array(vec![CfdValue::Ref(
                "renamed".try_into().expect("id")
            )]))
        );
        assert_eq!(report.affected_files, ["data/a.cfd"]);
        assert!(session.record_view("Item", "new").is_none());
        assert!(session.record_view("Item", "renamed").is_some());
    }

    #[test]
    fn inserted_field_and_delete_still_fold_without_publishing() {
        let (directory, mut session) = fixture();
        let before = fs::read(directory.path().join("data/a.cfd")).expect("before");
        let report = session.apply_mutation(
            MutationRequest {
                stop_on_write_error: true,
                ops: vec![
                    MutationOp::InsertRecord {
                        file: "data/a.cfd".into(),
                        actual_type: "Item".into(),
                        id: "new".into(),
                        fields: crate::MutationFields::Empty,
                        materialization: crate::DefaultMaterialization::Minimal,
                    },
                    set("new", "number", CfdValue::Int(7)),
                    MutationOp::DeleteRecord {
                        record: RecordCoordinate::try_new("Item", "new").expect("coordinate"),
                        file: None,
                    },
                ],
            },
            |_, _| Ok(Vec::new()),
        );
        assert!(
            report.write_ok && report.check_ok && !report.generation_changed,
            "{report:?}"
        );
        assert_eq!(report.applied.len(), 3);
        assert_eq!(report.written_files, Vec::<String>::new());
        assert_eq!(
            fs::read(directory.path().join("data/a.cfd")).expect("after"),
            before
        );
        assert!(session.record_view("Item", "new").is_none());
    }

    #[test]
    fn rename_preserves_nested_options_in_base_and_dimension_references() {
        let (_directory, mut session) = fixture_with_schema(
            "table Item { number: int = 0; label: string = \"\"; @localized target: Option<Option<&Item>> = None; }",
        );
        let nested_ref = |id: &str| {
            CfdValue::OptionSome(Box::new(CfdValue::OptionSome(Box::new(CfdValue::Ref(
                id.try_into().expect("id"),
            )))))
        };
        let report = session.apply_mutation(
            MutationRequest {
                stop_on_write_error: true,
                ops: vec![
                    MutationOp::InsertRecord {
                        file: "data/a.cfd".into(),
                        actual_type: "Item".into(),
                        id: "new".into(),
                        fields: crate::MutationFields::Empty,
                        materialization: crate::DefaultMaterialization::Minimal,
                    },
                    set("a1", "target", nested_ref("new")),
                    MutationOp::SetDimensionValue {
                        file: Some("data/ja.cfd".into()),
                        coordinate: crate::DimensionValueCoordinate {
                            actual_type: "Item".try_into().expect("type"),
                            record_id: "a1".try_into().expect("id"),
                            field: "target".try_into().expect("field"),
                            dimension: "language".try_into().expect("dimension"),
                            variant: "ja".try_into().expect("variant"),
                            path: vec![],
                        },
                        expected: crate::DimensionValueExpectation::Any,
                        value: MutationValue::Cfd(nested_ref("new")),
                    },
                    MutationOp::RenameRecord {
                        record: RecordCoordinate::try_new("Item", "new").expect("coordinate"),
                        file: None,
                        new_id: "renamed".into(),
                    },
                ],
            },
            |_, _| Ok(Vec::new()),
        );
        assert!(report.write_ok && report.check_ok, "{report:?}");
        assert_eq!(report.affected_files, ["data/a.cfd", "data/ja.cfd"]);
        let record = session.record_view("Item", "a1").expect("host").record;
        assert_eq!(record.field("target"), Some(&nested_ref("renamed")));
        let dimension = record.dimension_field("target").expect("dimension");
        assert_eq!(
            dimension
                .variants
                .get("ja")
                .expect("variant")
                .value
                .as_ref(),
            &nested_ref("renamed")
        );
    }
}
