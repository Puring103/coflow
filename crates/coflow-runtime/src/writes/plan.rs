use std::sync::Arc;

use coflow_api::{
    Diagnostic, DiagnosticSet, ProviderRegistry, ResolvedSource, SourceWriter,
    WriteFieldPathSegment,
};
use coflow_data_model::{CfdValue, RecordOrigin};

use crate::mutation::PreparedMutationOp;
use crate::{ProjectSession, RecordCoordinate};

use super::refs::{
    reference_update_actions, source_rewrite_actions, ReferenceUpdateAction, SourceRewriteAction,
};
use super::target::{is_id_path, not_found, write_target_for_path, WriteTarget};
use super::writer::{lookup_source_writer, source_for_file, source_for_id};
use crate::write_rules;

pub(crate) enum MutationExecutionPlan {
    Insert(InsertPlan),
    WriteField(WriteFieldPlan),
    Rename(RenamePlan),
    Delete(DeletePlan),
    Noop { coordinate: RecordCoordinate },
    Folded,
}

pub(crate) struct InsertPlan {
    pub(super) source: ResolvedSource,
    pub(super) writer: Arc<dyn SourceWriter>,
    pub(super) sheet: Option<String>,
}

pub(crate) struct WriteFieldPlan {
    pub(super) host_coordinate: RecordCoordinate,
    pub(super) target: WriteTarget,
    pub(super) source: ResolvedSource,
    pub(super) writer: Arc<dyn SourceWriter>,
}

pub(crate) enum RenamePlan {
    Noop { coordinate: RecordCoordinate },
    Write(Box<RenameWritePlan>),
}

pub(crate) struct RenameWritePlan {
    pub(super) old_coordinate: RecordCoordinate,
    pub(super) origin: RecordOrigin,
    pub(super) display_path: String,
    pub(super) source: ResolvedSource,
    pub(super) writer: Arc<dyn SourceWriter>,
    pub(super) reference_actions: Vec<ReferenceUpdateAction>,
    pub(super) rewrite_actions: Vec<SourceRewriteAction>,
}

pub(crate) struct DeletePlan {
    pub(super) coordinate: RecordCoordinate,
    pub(super) origin: RecordOrigin,
    pub(super) display_path: String,
    pub(super) source: ResolvedSource,
    pub(super) writer: Arc<dyn SourceWriter>,
}

impl MutationExecutionPlan {
    pub(crate) const fn changes_generation(&self) -> bool {
        !matches!(
            self,
            Self::Rename(RenamePlan::Noop { .. }) | Self::Noop { .. } | Self::Folded
        )
    }

    pub(crate) fn visit_sources<E>(
        &self,
        mut visit: impl FnMut(&ResolvedSource, &Arc<dyn SourceWriter>) -> Result<(), E>,
    ) -> Result<(), E> {
        match self {
            Self::Insert(plan) => visit(&plan.source, &plan.writer)?,
            Self::WriteField(plan) => visit(&plan.source, &plan.writer)?,
            Self::Rename(RenamePlan::Noop { .. }) | Self::Folded => {}
            Self::Rename(RenamePlan::Write(plan)) => {
                visit(&plan.source, &plan.writer)?;
                for action in &plan.reference_actions {
                    visit(action.source(), &action.writer)?;
                }
                for action in &plan.rewrite_actions {
                    visit(action.source(), &action.writer)?;
                }
            }
            Self::Delete(plan) => visit(&plan.source, &plan.writer)?,
            Self::Noop { .. } => {}
        }
        Ok(())
    }

    pub(crate) fn can_batch_field_write_with(&self, other: &Self) -> bool {
        let (Self::WriteField(left), Self::WriteField(right)) = (self, other) else {
            return false;
        };
        Arc::ptr_eq(&left.writer, &right.writer)
            && left.source.provider_id == right.source.provider_id
            && left.source.location == right.source.location
    }
}

pub(crate) fn prepare_mutation_execution(
    session: &ProjectSession,
    registry: &ProviderRegistry,
    op: &PreparedMutationOp,
) -> Result<MutationExecutionPlan, DiagnosticSet> {
    match op {
        PreparedMutationOp::InsertRecord {
            file,
            sheet,
            actual_type,
            ..
        } => {
            let source = source_for_file(session, file)?;
            let writer = lookup_source_writer(registry, &source)?;
            Ok(MutationExecutionPlan::Insert(InsertPlan {
                source,
                writer,
                sheet: sheet
                    .clone()
                    .or_else(|| sheet_for_file_type(session, file, actual_type)),
            }))
        }
        PreparedMutationOp::SetField {
            write_record,
            path,
            value,
            ..
        } if is_id_path(path) => {
            let CfdValue::String(new_key) = value else {
                return Err(DiagnosticSet::one(Diagnostic::error(
                    "WRITE-RENAME",
                    "WRITE",
                    "record key writes require a string value",
                )));
            };
            prepare_rename(session, registry, write_record, new_key)
                .map(MutationExecutionPlan::Rename)
        }
        PreparedMutationOp::SetField {
            write_record,
            path,
            value,
            ..
        } => prepare_write_field(
            session,
            registry,
            &write_record.actual_type,
            &write_record.key,
            path,
            value,
        )
        .map(|plan| {
            plan.map_or_else(
                || MutationExecutionPlan::Noop {
                    coordinate: write_record.clone(),
                },
                MutationExecutionPlan::WriteField,
            )
        }),
        PreparedMutationOp::RenameRecord {
            record, new_key, ..
        } => prepare_rename(session, registry, record, new_key).map(MutationExecutionPlan::Rename),
        PreparedMutationOp::DeleteRecord { record, .. } => {
            prepare_delete(session, registry, record).map(MutationExecutionPlan::Delete)
        }
        PreparedMutationOp::FoldedSetField { .. }
        | PreparedMutationOp::FoldedRenameRecord { .. }
        | PreparedMutationOp::FoldedDeleteRecord { .. }
        | PreparedMutationOp::CancelledInsert { .. } => Ok(MutationExecutionPlan::Folded),
    }
}

fn prepare_write_field(
    session: &ProjectSession,
    registry: &ProviderRegistry,
    actual_type: &str,
    key: &str,
    path: &[WriteFieldPathSegment],
    new_value: &CfdValue,
) -> Result<Option<WriteFieldPlan>, DiagnosticSet> {
    let Some(record_ref) = session.records.get_by_coordinate(actual_type, key) else {
        return Err(DiagnosticSet::one(not_found(actual_type, key)));
    };
    let Some(_record) = session.model.record(record_ref.id) else {
        return Err(DiagnosticSet::one(not_found(actual_type, key)));
    };
    let target = write_target_for_path(session, record_ref, path)?;
    write_rules::validate_value_at_write_path(
        session,
        &target.coordinate.actual_type,
        &target.field_path,
        new_value,
        "WRITE-SHAPE",
        "WRITE",
    )?;
    if session.field_value(
        &target.coordinate.actual_type,
        &target.coordinate.key,
        &target.field_path,
    ) == Some(new_value)
    {
        return Ok(None);
    }
    let source = source_for_id(session, target.source_id)?;
    let writer = lookup_source_writer(registry, &source)?;
    Ok(Some(WriteFieldPlan {
        host_coordinate: record_ref.coordinate.clone(),
        target,
        source,
        writer,
    }))
}

fn prepare_rename(
    session: &ProjectSession,
    registry: &ProviderRegistry,
    record: &RecordCoordinate,
    new_key: &str,
) -> Result<RenamePlan, DiagnosticSet> {
    let Some(target_ref) = session
        .records
        .get_by_coordinate(&record.actual_type, &record.key)
    else {
        return Err(DiagnosticSet::one(not_found(
            &record.actual_type,
            &record.key,
        )));
    };
    if record.key == new_key {
        return Ok(RenamePlan::Noop {
            coordinate: target_ref.coordinate.clone(),
        });
    }
    let source = source_for_id(session, target_ref.source_id)?;
    let writer = lookup_source_writer(registry, &source)?;
    let reference_actions = reference_update_actions(session, registry, target_ref.id, new_key)?;
    let rewrite_actions =
        source_rewrite_actions(session, registry, target_ref.id, &record.key, new_key)?;
    Ok(RenamePlan::Write(Box::new(RenameWritePlan {
        old_coordinate: target_ref.coordinate.clone(),
        origin: target_ref.origin.clone(),
        display_path: target_ref.display_path.clone(),
        source,
        writer,
        reference_actions,
        rewrite_actions,
    })))
}

fn prepare_delete(
    session: &ProjectSession,
    registry: &ProviderRegistry,
    record: &RecordCoordinate,
) -> Result<DeletePlan, DiagnosticSet> {
    let Some(record_ref) = session
        .records
        .get_by_coordinate(&record.actual_type, &record.key)
    else {
        return Err(DiagnosticSet::one(not_found(
            &record.actual_type,
            &record.key,
        )));
    };
    let Some(model_record) = session.model.record(record_ref.id) else {
        return Err(DiagnosticSet::one(not_found(
            &record.actual_type,
            &record.key,
        )));
    };
    let source = source_for_id(session, record_ref.source_id)?;
    let writer = lookup_source_writer(registry, &source)?;
    Ok(DeletePlan {
        coordinate: record_ref.coordinate.clone(),
        origin: model_record.origin.clone(),
        display_path: record_ref.display_path.clone(),
        source,
        writer,
    })
}

fn sheet_for_file_type(session: &ProjectSession, file: &str, actual_type: &str) -> Option<String> {
    for id in session.records.ids_in_file(file) {
        let Some(record_ref) = session.records.get(*id) else {
            continue;
        };
        let RecordOrigin::Table { sheet, .. } = &record_ref.origin else {
            continue;
        };
        if record_ref.coordinate.actual_type == actual_type {
            return Some(sheet.clone());
        }
    }
    None
}
