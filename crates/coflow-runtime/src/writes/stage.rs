use coflow_api::{
    DeleteRecordRequest, Diagnostic, DiagnosticSet, InsertRecordRequest, RenameRecordRequest,
    WriteCellRequest, WriteContext,
};
use coflow_data_model::CfdValue;
use std::collections::BTreeSet;

use crate::mutation::PreparedMutationOp;
use crate::{ProjectSession, RecordCoordinate, WriteOutcome};

use super::plan::{
    DeletePlan, InsertPlan, MutationExecutionPlan, RenamePlan, RenameWritePlan, WriteFieldPlan,
};

pub(crate) fn preflight_mutation_op(
    session: &ProjectSession,
    op: &PreparedMutationOp,
    execution: &MutationExecutionPlan,
) -> Result<(), DiagnosticSet> {
    let (PreparedMutationOp::SetField { value, .. }, MutationExecutionPlan::WriteField(plan)) =
        (op, execution)
    else {
        return Ok(());
    };
    let compiled_schema = session.compiled_schema();
    let request = WriteCellRequest {
        origin: &plan.target.origin,
        record_key: &plan.target.coordinate.key,
        actual_type: &plan.target.coordinate.actual_type,
        field_path: &plan.target.field_path,
        new_value: value,
        schema: compiled_schema,
        source: &plan.source,
    };
    let diagnostics = plan.writer.preflight(
        WriteContext {
            project_root: &session.project.root_dir,
            schema: compiled_schema,
            model: Some(&session.model),
        },
        &request,
    );
    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(diagnostics)
    }
}

pub(crate) fn stage_mutation_op(
    session: &ProjectSession,
    op: &PreparedMutationOp,
    execution: &MutationExecutionPlan,
) -> Result<WriteOutcome, DiagnosticSet> {
    match (op, execution) {
        (
            PreparedMutationOp::InsertRecord {
                file,
                actual_type,
                key,
                fields,
                ..
            },
            MutationExecutionPlan::Insert(plan),
        ) => stage_insert_record(session, plan, file, key, actual_type, fields),
        (
            PreparedMutationOp::SetField { record, value, .. },
            MutationExecutionPlan::WriteField(plan),
        ) => stage_write_field(session, plan, record, value),
        (
            PreparedMutationOp::SetField { record, value, .. },
            MutationExecutionPlan::Rename(plan),
        ) => {
            let CfdValue::String(new_key) = value else {
                return Err(plan_mismatch("rename execution value is not a string"));
            };
            stage_rename_record_key(session, plan, record, new_key)
        }
        (
            PreparedMutationOp::RenameRecord {
                record, new_key, ..
            },
            MutationExecutionPlan::Rename(plan),
        ) => stage_rename_record_key(session, plan, record, new_key),
        (PreparedMutationOp::DeleteRecord { record, .. }, MutationExecutionPlan::Delete(plan)) => {
            stage_delete_record(session, plan, record)
        }
        (PreparedMutationOp::SetField { .. }, MutationExecutionPlan::Noop { coordinate }) => {
            Ok(WriteOutcome::touch(coordinate.clone()))
        }
        (PreparedMutationOp::FoldedSetField { record, .. }, MutationExecutionPlan::Folded) => {
            Ok(WriteOutcome::touch(record.clone()))
        }
        (
            PreparedMutationOp::FoldedRenameRecord {
                old_record,
                new_record,
                ..
            },
            MutationExecutionPlan::Folded,
        ) => Ok(WriteOutcome {
            touched: vec![old_record.clone(), new_record.clone()],
            inserted: None,
            deleted: None,
            renamed: Some((old_record.clone(), new_record.clone())),
            affected_files: Vec::new(),
            diagnostics: DiagnosticSet::empty(),
        }),
        (PreparedMutationOp::FoldedDeleteRecord { record, .. }, MutationExecutionPlan::Folded) => {
            Ok(WriteOutcome {
                touched: Vec::new(),
                inserted: None,
                deleted: Some(record.clone()),
                renamed: None,
                affected_files: Vec::new(),
                diagnostics: DiagnosticSet::empty(),
            })
        }
        (PreparedMutationOp::CancelledInsert { record, .. }, MutationExecutionPlan::Folded) => {
            Ok(WriteOutcome {
                touched: vec![record.clone()],
                inserted: Some(record.clone()),
                deleted: None,
                renamed: None,
                affected_files: Vec::new(),
                diagnostics: DiagnosticSet::empty(),
            })
        }
        _ => Err(plan_mismatch(
            "prepared mutation and provider execution plan do not match",
        )),
    }
}

pub(crate) struct MutationBatchFailure {
    pub(crate) index: usize,
    pub(crate) diagnostics: DiagnosticSet,
}

pub(crate) fn stage_field_mutation_batch(
    session: &ProjectSession,
    batch: &[(&PreparedMutationOp, &MutationExecutionPlan)],
) -> Result<Vec<WriteOutcome>, MutationBatchFailure> {
    let Some((_, MutationExecutionPlan::WriteField(first_plan))) = batch.first() else {
        return Err(MutationBatchFailure {
            index: 0,
            diagnostics: plan_mismatch("field batch does not start with a field write"),
        });
    };
    let mut requests = Vec::with_capacity(batch.len());
    for (index, (op, execution)) in batch.iter().enumerate() {
        let (PreparedMutationOp::SetField { value, .. }, MutationExecutionPlan::WriteField(plan)) =
            (op, execution)
        else {
            return Err(MutationBatchFailure {
                index,
                diagnostics: plan_mismatch("field batch contains a non-field mutation"),
            });
        };
        if !batch[0].1.can_batch_field_write_with(execution) {
            return Err(MutationBatchFailure {
                index,
                diagnostics: plan_mismatch("field batch spans more than one resolved source"),
            });
        }
        requests.push(WriteCellRequest {
            origin: &plan.target.origin,
            record_key: &plan.target.coordinate.key,
            actual_type: &plan.target.coordinate.actual_type,
            field_path: &plan.target.field_path,
            new_value: value,
            schema: session.compiled_schema(),
            source: &plan.source,
        });
    }
    let ctx = WriteContext {
        project_root: &session.project.root_dir,
        schema: session.compiled_schema(),
        model: Some(&session.model),
    };
    let provider_outcomes = first_plan
        .writer
        .write_field_batch(ctx, &requests)
        .map_err(|failure| MutationBatchFailure {
            index: failure.index,
            diagnostics: failure.diagnostics,
        })?;
    if provider_outcomes.len() != batch.len() {
        return Err(MutationBatchFailure {
            index: 0,
            diagnostics: plan_mismatch("writer returned the wrong number of field batch outcomes"),
        });
    }
    batch
        .iter()
        .enumerate()
        .zip(provider_outcomes)
        .map(|((index, (op, execution)), provider_outcome)| {
            let (
                PreparedMutationOp::SetField { record, .. },
                MutationExecutionPlan::WriteField(plan),
            ) = (op, execution)
            else {
                return Err(MutationBatchFailure {
                    index,
                    diagnostics: plan_mismatch(
                        "validated field batch changed before outcome assembly",
                    ),
                });
            };
            Ok(field_write_outcome(plan, record, provider_outcome))
        })
        .collect()
}

fn stage_write_field(
    session: &ProjectSession,
    plan: &WriteFieldPlan,
    host_record: &RecordCoordinate,
    new_value: &CfdValue,
) -> Result<WriteOutcome, DiagnosticSet> {
    let compiled_schema = session.compiled_schema();
    let request = WriteCellRequest {
        origin: &plan.target.origin,
        record_key: &plan.target.coordinate.key,
        actual_type: &plan.target.coordinate.actual_type,
        field_path: &plan.target.field_path,
        new_value,
        schema: compiled_schema,
        source: &plan.source,
    };
    let ctx = WriteContext {
        project_root: &session.project.root_dir,
        schema: compiled_schema,
        model: Some(&session.model),
    };
    let provider_outcome = plan.writer.write_field(ctx, &request)?;
    Ok(field_write_outcome(plan, host_record, provider_outcome))
}

fn field_write_outcome(
    plan: &WriteFieldPlan,
    host_record: &RecordCoordinate,
    provider_outcome: coflow_api::WriteOutcome,
) -> WriteOutcome {
    WriteOutcome {
        touched: if host_record == &plan.target.coordinate {
            vec![host_record.clone()]
        } else {
            vec![host_record.clone(), plan.target.coordinate.clone()]
        },
        inserted: None,
        deleted: None,
        renamed: None,
        affected_files: vec![plan.target.display_path.clone()],
        diagnostics: provider_outcome.diagnostics,
    }
}

fn stage_rename_record_key(
    session: &ProjectSession,
    plan: &RenamePlan,
    host_record: &RecordCoordinate,
    new_key: &str,
) -> Result<WriteOutcome, DiagnosticSet> {
    let plan = match plan {
        RenamePlan::Noop { coordinate } => {
            let mut outcome = WriteOutcome::touch(coordinate.clone());
            if host_record != coordinate {
                outcome.touched.insert(0, host_record.clone());
            }
            return Ok(outcome);
        }
        RenamePlan::Write(plan) => plan,
    };
    let plan: &RenameWritePlan = plan;
    let compiled_schema = session.compiled_schema();
    let ctx = WriteContext {
        project_root: &session.project.root_dir,
        schema: compiled_schema,
        model: Some(&session.model),
    };
    let target_request = RenameRecordRequest {
        origin: &plan.origin,
        old_key: &plan.old_coordinate.key,
        new_key,
        actual_type: &plan.old_coordinate.actual_type,
        source: &plan.source,
        schema: compiled_schema,
    };
    let mut diagnostics = plan.writer.rename_record(ctx, &target_request)?.diagnostics;
    let mut affected_files = BTreeSet::from([plan.display_path.clone()]);
    for action in &plan.reference_actions {
        let outcome = action
            .writer
            .write_field(ctx, &action.request.as_request(compiled_schema))?;
        diagnostics.extend(outcome.diagnostics);
        affected_files.insert(action.display_path().to_string());
    }
    for action in &plan.rewrite_actions {
        let outcome = action
            .writer
            .rewrite_record_references(ctx, &action.request.as_request(compiled_schema))?;
        diagnostics.extend(outcome.diagnostics);
        affected_files.insert(action.display_path().to_string());
    }

    let new_coordinate = RecordCoordinate::new(&plan.old_coordinate.actual_type, new_key);
    let mut touched = vec![plan.old_coordinate.clone(), new_coordinate.clone()];
    if host_record != &plan.old_coordinate {
        touched.insert(0, host_record.clone());
    }
    Ok(WriteOutcome {
        touched,
        inserted: None,
        deleted: None,
        renamed: Some((plan.old_coordinate.clone(), new_coordinate)),
        affected_files: affected_files.into_iter().collect(),
        diagnostics,
    })
}

fn stage_insert_record(
    session: &ProjectSession,
    plan: &InsertPlan,
    file: &str,
    record_key: &str,
    actual_type: &str,
    fields: &std::collections::BTreeMap<String, CfdValue>,
) -> Result<WriteOutcome, DiagnosticSet> {
    let compiled_schema = session.compiled_schema();
    let request = InsertRecordRequest {
        source: &plan.source,
        sheet: plan.sheet.as_deref(),
        record_key,
        actual_type,
        fields,
        schema: compiled_schema,
    };
    let ctx = WriteContext {
        project_root: &session.project.root_dir,
        schema: compiled_schema,
        model: Some(&session.model),
    };
    let provider_outcome = plan.writer.insert_record(ctx, &request)?;
    let inserted = RecordCoordinate::new(actual_type, record_key);
    Ok(WriteOutcome {
        touched: vec![inserted.clone()],
        inserted: Some(inserted),
        deleted: None,
        renamed: None,
        affected_files: vec![file.to_string()],
        diagnostics: provider_outcome.diagnostics,
    })
}

fn stage_delete_record(
    session: &ProjectSession,
    plan: &DeletePlan,
    record: &RecordCoordinate,
) -> Result<WriteOutcome, DiagnosticSet> {
    let compiled_schema = session.compiled_schema();
    let request = DeleteRecordRequest {
        origin: &plan.origin,
        record_key: &record.key,
        actual_type: &record.actual_type,
        source: &plan.source,
    };
    let ctx = WriteContext {
        project_root: &session.project.root_dir,
        schema: compiled_schema,
        model: Some(&session.model),
    };
    let provider_outcome = plan.writer.delete_record(ctx, &request)?;
    Ok(WriteOutcome {
        touched: Vec::new(),
        inserted: None,
        deleted: Some(plan.coordinate.clone()),
        renamed: None,
        affected_files: vec![plan.display_path.clone()],
        diagnostics: provider_outcome.diagnostics,
    })
}

fn plan_mismatch(message: &str) -> DiagnosticSet {
    DiagnosticSet::one(Diagnostic::error(
        "MUTATION-TXN-INVARIANT",
        "MUTATION",
        message,
    ))
}
