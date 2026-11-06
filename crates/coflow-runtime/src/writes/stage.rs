use crate::api::{
    DeleteRecordRequest, Diagnostic, DiagnosticSet, DimensionSourceSchema, InsertRecordRequest,
    RenameRecordRequest, ReorderRecordsOperation, ReorderRecordsRequest,
    RewriteDimensionRecordRequest, WriteCellRequest, WriteDimensionValueRequest, WriteRecordRef,
};
use crate::data_model::CfdValue;
use coflow_language::cft::RecordId;
use std::collections::BTreeSet;

use crate::{ProjectSession, RecordCoordinate, WriteOutcome};

use super::plan::{
    DeletePlan, DimensionRecordAction, DimensionWritePlan, ExecutableMutationOp, FieldWrite,
    InsertPlan, RenamePlan, RenameWritePlan, ReorderOperation, ReorderPlan, TransferPlan,
    WriteFieldPlan,
};

pub(crate) fn stage_mutation_op(
    session: &ProjectSession,
    execution: &ExecutableMutationOp,
) -> Result<WriteOutcome, DiagnosticSet> {
    match execution {
        ExecutableMutationOp::Restore {
            snapshot,
            writer,
            sources,
        } => {
            for (patch, source) in snapshot.patches.iter().zip(sources) {
                writer.restore_source_patch(source.location.path(), patch)?;
            }
            Ok(WriteOutcome {
                inserted: Some(snapshot.record.clone()),
                affected_files: snapshot
                    .patches
                    .iter()
                    .map(|patch| patch.file.clone())
                    .collect(),
                ..WriteOutcome::default()
            })
        }
        ExecutableMutationOp::Insert {
            plan,
            coordinate,
            fields,
        } => stage_insert_record(session, plan, coordinate, fields),
        ExecutableMutationOp::WriteField(write) => stage_write_field(
            session,
            &write.plan,
            &write.value,
            write
                .materialized_top_level
                .as_deref()
                .map(crate::api::FieldMaterialization::Record),
        ),
        ExecutableMutationOp::WriteDimension {
            plan,
            record,
            coordinate,
            new_value,
        } => stage_write_dimension_value(
            session,
            plan,
            record,
            coordinate,
            new_value.as_ref(),
            &plan.display_path,
        ),
        ExecutableMutationOp::UnsetField(plan) => {
            let writer_outcome = plan.writer.unset_field(
                &plan.target.origin,
                &plan.target.coordinate.id,
                &plan.target.coordinate.actual_type,
                &plan.target.field_path,
                session.schema(),
            )?;
            Ok(field_write_outcome(plan, writer_outcome))
        }
        ExecutableMutationOp::Rename { plan, new_id } => {
            stage_rename_record_id(session, plan, new_id)
        }
        ExecutableMutationOp::Delete(plan) => stage_delete_record(session, plan, &plan.coordinate),
        ExecutableMutationOp::Reorder(plan) => stage_reorder_records(plan),
        ExecutableMutationOp::Transfer(plan) => stage_transfer_record(session, plan),
        ExecutableMutationOp::Noop { coordinate } => Ok(WriteOutcome::touch(coordinate.clone())),
        ExecutableMutationOp::Folded(outcome) => Ok(outcome.clone()),
    }
}

pub(crate) struct MutationBatchFailure {
    pub(crate) index: usize,
    pub(crate) diagnostics: DiagnosticSet,
}

pub(crate) fn stage_field_mutation_batch(
    session: &ProjectSession,
    batch: &[&FieldWrite],
) -> Result<Vec<WriteOutcome>, MutationBatchFailure> {
    let Some(first) = batch.first() else {
        return Ok(Vec::new());
    };
    let mut requests = Vec::with_capacity(batch.len());
    for (index, write) in batch.iter().enumerate() {
        if first.batch_key() != write.batch_key() {
            return Err(MutationBatchFailure {
                index,
                diagnostics: plan_mismatch("field batch spans more than one resolved source"),
            });
        }
        let plan = &write.plan;
        requests.push(WriteCellRequest {
            origin: &plan.target.origin,
            record_id: &plan.target.coordinate.id,
            actual_type: &plan.target.coordinate.actual_type,
            field_path: &plan.target.field_path,
            new_value: &write.value,
            materialized_top_level: write
                .materialized_top_level
                .as_deref()
                .map(crate::api::FieldMaterialization::Record),
            schema: session.schema(),
        });
    }
    let outcomes = first
        .plan
        .writer
        .write_field_batch(&requests)
        .map_err(|failure| MutationBatchFailure {
            index: failure.index,
            diagnostics: failure.diagnostics,
        })?;
    if outcomes.len() != batch.len() {
        return Err(MutationBatchFailure {
            index: 0,
            diagnostics: plan_mismatch("writer returned the wrong number of field batch outcomes"),
        });
    }
    Ok(batch
        .iter()
        .zip(outcomes)
        .map(|(write, outcome)| field_write_outcome(&write.plan, outcome))
        .collect())
}

fn stage_write_field(
    session: &ProjectSession,
    plan: &WriteFieldPlan,
    new_value: &CfdValue,
    materialized_top_level: Option<crate::api::FieldMaterialization<'_>>,
) -> Result<WriteOutcome, DiagnosticSet> {
    let schema = session.schema();
    let request = WriteCellRequest {
        origin: &plan.target.origin,
        record_id: &plan.target.coordinate.id,
        actual_type: &plan.target.coordinate.actual_type,
        field_path: &plan.target.field_path,
        new_value,
        materialized_top_level,
        schema,
    };
    let writer_outcome = plan.writer.write_field(&request)?;
    Ok(field_write_outcome(plan, writer_outcome))
}

fn stage_write_dimension_value(
    session: &ProjectSession,
    plan: &DimensionWritePlan,
    record: &RecordCoordinate,
    coordinate: &crate::mutation::DimensionSourceCoordinate,
    new_value: Option<&CfdValue>,
    write_file: &str,
) -> Result<WriteOutcome, DiagnosticSet> {
    let schema = session.schema();
    let source_type = schema
        .resolve_type(&coordinate.source_type)
        .ok_or_else(|| plan_mismatch("dimension source type disappeared before staging"))?;
    let source_field = schema
        .field(&coordinate.source_type, &coordinate.field)
        .ok_or_else(|| plan_mismatch("dimension source field disappeared before staging"))?;
    let dimension = schema
        .resolve_dimension(&coordinate.dimension)
        .ok_or_else(|| plan_mismatch("dimension disappeared before staging"))?;
    let result = plan
        .manager
        .write_dimension_value(&WriteDimensionValueRequest {
            source: &plan.source,
            schema: DimensionSourceSchema {
                schema,
                dimension,
                source_type,
                source_field,
            },
            source_id: &coordinate.source_id,
            variant: &coordinate.variant,
            new_value,
        })?;
    Ok(WriteOutcome {
        touched: vec![record.clone()],
        inserted: None,
        deleted: None,
        renamed: None,
        reordered: false,
        affected_files: result
            .changed
            .then(|| write_file.to_string())
            .into_iter()
            .collect(),
        diagnostics: DiagnosticSet::empty(),
    })
}

fn field_write_outcome(
    plan: &WriteFieldPlan,
    writer_outcome: crate::api::WriteOutcome,
) -> WriteOutcome {
    WriteOutcome {
        touched: vec![plan.target.coordinate.clone()],
        inserted: None,
        deleted: None,
        renamed: None,
        reordered: false,
        affected_files: vec![plan.target.display_path.clone()],
        diagnostics: writer_outcome.diagnostics,
    }
}

fn stage_rename_record_id(
    session: &ProjectSession,
    plan: &RenamePlan,
    new_id: &RecordId,
) -> Result<WriteOutcome, DiagnosticSet> {
    let plan = match plan {
        RenamePlan::Noop { coordinate } => return Ok(WriteOutcome::touch(coordinate.clone())),
        RenamePlan::Write(plan) => plan,
    };
    let plan: &RenameWritePlan = plan;
    let schema = session.schema();
    let target_request = RenameRecordRequest {
        origin: &plan.origin,
        old_id: &plan.old_coordinate.id,
        new_id,
        actual_type: &plan.old_coordinate.actual_type,
    };
    let mut diagnostics = plan.writer.rename_record(&target_request)?.diagnostics;
    let mut affected_files = BTreeSet::from([plan.display_path.clone()]);
    for action in &plan.reference_actions {
        diagnostics.extend(action.execute(schema)?);
        affected_files.insert(action.display_path().to_string());
    }
    let old_id = plan.old_coordinate.id.clone();
    let new_dimension_key = new_id.clone();
    rewrite_dimension_records(
        session,
        &plan.dimension_actions,
        &old_id,
        Some(&new_dimension_key),
        &mut affected_files,
    )?;

    let new_coordinate =
        RecordCoordinate::new(plan.old_coordinate.actual_type.clone(), new_dimension_key);
    let touched = vec![plan.old_coordinate.clone(), new_coordinate.clone()];
    Ok(WriteOutcome {
        touched,
        inserted: None,
        deleted: None,
        renamed: Some((plan.old_coordinate.clone(), new_coordinate)),
        reordered: false,
        affected_files: affected_files.into_iter().collect(),
        diagnostics,
    })
}

fn stage_insert_record(
    session: &ProjectSession,
    plan: &InsertPlan,
    coordinate: &RecordCoordinate,
    fields: &std::collections::BTreeMap<String, CfdValue>,
) -> Result<WriteOutcome, DiagnosticSet> {
    let schema = session.schema();
    let request = InsertRecordRequest {
        source: &plan.source,
        record_id: &coordinate.id,
        actual_type: &coordinate.actual_type,
        fields,
        schema,
        before: None,
    };
    let writer_outcome = plan.writer.insert_record(&request)?;
    let inserted = coordinate.clone();
    Ok(WriteOutcome {
        touched: vec![inserted.clone()],
        inserted: Some(inserted),
        deleted: None,
        renamed: None,
        reordered: false,
        affected_files: vec![plan.display_path.clone()],
        diagnostics: writer_outcome.diagnostics,
    })
}

fn stage_delete_record(
    session: &ProjectSession,
    plan: &DeletePlan,
    record: &RecordCoordinate,
) -> Result<WriteOutcome, DiagnosticSet> {
    let request = DeleteRecordRequest {
        origin: &plan.origin,
        record_id: &record.id,
        actual_type: &record.actual_type,
    };
    let writer_outcome = plan.writer.delete_record(&request)?;
    let old_id = record.id.clone();
    let mut affected_files = BTreeSet::from([plan.display_path.clone()]);
    rewrite_dimension_records(
        session,
        &plan.dimension_actions,
        &old_id,
        None,
        &mut affected_files,
    )?;
    Ok(WriteOutcome {
        touched: Vec::new(),
        inserted: None,
        deleted: Some(plan.coordinate.clone()),
        renamed: None,
        reordered: false,
        affected_files: affected_files.into_iter().collect(),
        diagnostics: writer_outcome.diagnostics,
    })
}

fn stage_reorder_records(plan: &ReorderPlan) -> Result<WriteOutcome, DiagnosticSet> {
    let (operation, touched) = match &plan.operation {
        ReorderOperation::Swap { first, second } => (
            ReorderRecordsOperation::Swap {
                first: write_record_ref(first),
                second: write_record_ref(second),
            },
            vec![first.coordinate.clone(), second.coordinate.clone()],
        ),
        ReorderOperation::MoveBefore { record, before } => (
            ReorderRecordsOperation::MoveBefore {
                record: write_record_ref(record),
                before: before.as_ref().map(write_record_ref),
            },
            std::iter::once(record.coordinate.clone())
                .chain(before.iter().map(|position| position.coordinate.clone()))
                .collect(),
        ),
    };
    let writer_outcome = plan.writer.reorder_records(&ReorderRecordsRequest {
        source: &plan.source,
        operation,
    })?;
    Ok(WriteOutcome {
        touched,
        inserted: None,
        deleted: None,
        renamed: None,
        reordered: true,
        affected_files: vec![plan.display_path.clone()],
        diagnostics: writer_outcome.diagnostics,
    })
}

fn stage_transfer_record(
    _session: &ProjectSession,
    plan: &TransferPlan,
) -> Result<WriteOutcome, DiagnosticSet> {
    let before = plan.before.as_ref().map(write_record_ref);
    let outcome = plan.source_writer.transfer_record(
        &WriteRecordRef {
            origin: &plan.source_origin,
            record_id: &plan.coordinate.id,
            actual_type: &plan.coordinate.actual_type,
        },
        &plan.destination,
        before.as_ref(),
    )?;
    Ok(WriteOutcome {
        touched: vec![plan.coordinate.clone()],
        inserted: None,
        deleted: None,
        renamed: None,
        reordered: true,
        affected_files: vec![
            plan.source_display_path.clone(),
            plan.destination_display_path.clone(),
        ],
        diagnostics: outcome.diagnostics,
    })
}

fn write_record_ref(position: &super::plan::ResolvedRecordPosition) -> WriteRecordRef<'_> {
    WriteRecordRef {
        origin: &position.origin,
        record_id: &position.coordinate.id,
        actual_type: &position.coordinate.actual_type,
    }
}

fn rewrite_dimension_records(
    session: &ProjectSession,
    actions: &[DimensionRecordAction],
    old_id: &RecordId,
    new_id: Option<&RecordId>,
    affected_files: &mut BTreeSet<String>,
) -> Result<(), DiagnosticSet> {
    let schema = session.schema();
    for action in actions {
        let result = action
            .manager
            .rewrite_dimension_record(&RewriteDimensionRecordRequest {
                source: &action.source,
                schema,
                actual_type: &action.actual_type,
                old_id,
                new_id,
            })?;
        if result.changed {
            affected_files.insert(action.source.display_name.clone());
        }
    }
    Ok(())
}

fn plan_mismatch(message: &str) -> DiagnosticSet {
    DiagnosticSet::one(Diagnostic::error(
        "MUTATION-TXN-INVARIANT",
        "MUTATION",
        message,
    ))
}
