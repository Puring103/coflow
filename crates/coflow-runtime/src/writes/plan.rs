use std::sync::Arc;

use coflow_api::{
    Diagnostic, DiagnosticSet, DimensionSourceManager, ProviderRegistry, ResolvedSource,
    SourceWriter, WriteFieldPathSegment,
};
use coflow_data_model::{CfdValue, RecordOrigin};

use crate::dimensions::DimensionField;
use crate::indexes::{RecordRef, SourceId};
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
    WriteDimension(DimensionWritePlan),
    Rename(RenamePlan),
    Delete(DeletePlan),
    Reorder(ReorderPlan),
    Transfer(TransferPlan),
    Noop { coordinate: RecordCoordinate },
    Folded,
}

pub(crate) struct InsertPlan {
    pub(super) source: ResolvedSource,
    pub(super) writer: Arc<dyn SourceWriter>,
    pub(super) sheet: Option<String>,
}

pub(crate) struct WriteFieldPlan {
    pub(super) target: WriteTarget,
    pub(super) source: ResolvedSource,
    pub(super) writer: Arc<dyn SourceWriter>,
}

pub(crate) struct DimensionWritePlan {
    pub(super) source: ResolvedSource,
    pub(super) manager: Arc<dyn DimensionSourceManager>,
}

pub(crate) struct DimensionRecordAction {
    pub(super) source: ResolvedSource,
    pub(super) manager: Arc<dyn DimensionSourceManager>,
    pub(super) field: DimensionField,
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
    pub(super) dimension_actions: Vec<DimensionRecordAction>,
}

pub(crate) struct DeletePlan {
    pub(super) coordinate: RecordCoordinate,
    pub(super) origin: RecordOrigin,
    pub(super) display_path: String,
    pub(super) source: ResolvedSource,
    pub(super) writer: Arc<dyn SourceWriter>,
    pub(super) dimension_actions: Vec<DimensionRecordAction>,
}

pub(crate) struct ReorderPlan {
    pub(super) source: ResolvedSource,
    pub(super) writer: Arc<dyn SourceWriter>,
    pub(super) operation: ReorderOperation,
    pub(super) display_path: String,
}

pub(crate) struct TransferPlan {
    pub(super) coordinate: RecordCoordinate,
    pub(super) fields: std::collections::BTreeMap<String, CfdValue>,
    pub(super) source_origin: RecordOrigin,
    pub(super) source_display_path: String,
    pub(super) source: ResolvedSource,
    pub(super) source_writer: Arc<dyn SourceWriter>,
    pub(super) destination_display_path: String,
    pub(super) destination: ResolvedSource,
    pub(super) destination_writer: Arc<dyn SourceWriter>,
    pub(super) destination_sheet: Option<String>,
    pub(super) before: Option<ResolvedRecordPosition>,
}

pub(crate) enum ReorderOperation {
    Swap {
        first: ResolvedRecordPosition,
        second: ResolvedRecordPosition,
    },
    MoveBefore {
        record: ResolvedRecordPosition,
        before: Option<ResolvedRecordPosition>,
    },
}

pub(crate) struct ResolvedRecordPosition {
    pub(super) coordinate: RecordCoordinate,
    pub(super) origin: RecordOrigin,
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
        mut visit: impl FnMut(&ResolvedSource, Option<&Arc<dyn SourceWriter>>) -> Result<(), E>,
    ) -> Result<(), E> {
        match self {
            Self::Insert(plan) => visit(&plan.source, Some(&plan.writer))?,
            Self::WriteField(plan) => visit(&plan.source, Some(&plan.writer))?,
            Self::WriteDimension(plan) => visit(&plan.source, None)?,
            Self::Rename(RenamePlan::Noop { .. }) | Self::Folded | Self::Noop { .. } => {}
            Self::Rename(RenamePlan::Write(plan)) => {
                visit(&plan.source, Some(&plan.writer))?;
                for action in &plan.reference_actions {
                    visit(action.source(), action.writer())?;
                }
                for action in &plan.rewrite_actions {
                    visit(action.source(), action.writer())?;
                }
                for action in &plan.dimension_actions {
                    visit(&action.source, None)?;
                }
            }
            Self::Delete(plan) => {
                visit(&plan.source, Some(&plan.writer))?;
                for action in &plan.dimension_actions {
                    visit(&action.source, None)?;
                }
            }
            Self::Reorder(plan) => visit(&plan.source, Some(&plan.writer))?,
            Self::Transfer(plan) => {
                visit(&plan.source, Some(&plan.source_writer))?;
                visit(&plan.destination, Some(&plan.destination_writer))?;
            }
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
    allow_noop: bool,
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
            allow_noop,
        )
        .map(|plan| {
            plan.map_or_else(
                || MutationExecutionPlan::Noop {
                    coordinate: write_record.clone(),
                },
                MutationExecutionPlan::WriteField,
            )
        }),
        PreparedMutationOp::WriteDimensionValue { write_file, .. } => {
            let source = source_for_file(session, write_file)?;
            let manager = registry
                .dimension_source_manager(&source.provider_id)
                .ok_or_else(|| {
                    transaction_invariant(format!(
                        "dimension source provider `{}` disappeared before mutation planning",
                        source.provider_id
                    ))
                })?;
            Ok(MutationExecutionPlan::WriteDimension(DimensionWritePlan {
                source,
                manager,
            }))
        }
        PreparedMutationOp::RenameRecord {
            record, new_key, ..
        } => prepare_rename(session, registry, record, new_key).map(MutationExecutionPlan::Rename),
        PreparedMutationOp::DeleteRecord { record, .. } => {
            prepare_delete(session, registry, record).map(MutationExecutionPlan::Delete)
        }
        PreparedMutationOp::SwapRecords { first, second, .. } => {
            prepare_swap_records(session, registry, first, second)
        }
        PreparedMutationOp::MoveRecord {
            record,
            target_index,
            ..
        } => prepare_move_record(session, registry, record, *target_index),
        PreparedMutationOp::TransferRecord {
            record,
            destination_file,
            destination_sheet,
            target_index,
            ..
        } => prepare_transfer_record(
            session,
            registry,
            record,
            destination_file,
            destination_sheet.as_deref(),
            *target_index,
        ),
        PreparedMutationOp::FoldedSetField { .. }
        | PreparedMutationOp::FoldedRenameRecord { .. }
        | PreparedMutationOp::FoldedDeleteRecord { .. }
        | PreparedMutationOp::CancelledInsert { .. } => Ok(MutationExecutionPlan::Folded),
    }
}

fn prepare_transfer_record(
    session: &ProjectSession,
    registry: &ProviderRegistry,
    record: &RecordCoordinate,
    destination_file: &str,
    requested_sheet: Option<&str>,
    target_index: usize,
) -> Result<MutationExecutionPlan, DiagnosticSet> {
    let record_ref = required_record_ref(session, record)?;
    if record_ref.display_path == destination_file {
        return Err(DiagnosticSet::one(Diagnostic::error(
            "WRITE-TRANSFER-FILE",
            "WRITE",
            "record transfer requires different source and destination files",
        )));
    }
    if matches!(record_ref.origin, RecordOrigin::None) {
        return Err(DiagnosticSet::one(Diagnostic::error(
            "WRITE-TRANSFER-ORIGIN",
            "WRITE",
            "record has no writable source origin",
        )));
    }
    let model_record = session
        .model
        .record(record_ref.id)
        .ok_or_else(|| reorder_invariant("record is missing from the data model"))?;
    let source = source_for_id(session, record_ref.source_id)?;
    let source_writer = lookup_source_writer(registry, &source)?;
    if !source_writer.capabilities(&source).can_delete_record {
        return Err(DiagnosticSet::one(Diagnostic::error(
            "WRITE-UNSUPPORTED",
            "WRITE",
            "source writer does not support transferring records out",
        )));
    }
    let destination = source_for_file(session, destination_file)?;
    let destination_writer = lookup_source_writer(registry, &destination)?;
    if !destination_writer
        .capabilities(&destination)
        .can_insert_record
    {
        return Err(DiagnosticSet::one(Diagnostic::error(
            "WRITE-UNSUPPORTED",
            "WRITE",
            "destination writer does not support transferring records in",
        )));
    }

    let destination_sheet = resolve_transfer_sheet(
        session,
        destination_file,
        &record.actual_type,
        requested_sheet,
    )?;
    let order = session
        .records
        .ids_in_file(destination_file)
        .iter()
        .filter_map(|id| session.records.get(*id))
        .filter(|candidate| {
            candidate.coordinate.actual_type == record.actual_type
                && record_matches_sheet(candidate, destination_sheet.as_deref())
        })
        .collect::<Vec<_>>();
    if target_index > order.len() {
        return Err(DiagnosticSet::one(Diagnostic::error(
            "WRITE-TRANSFER-INDEX",
            "WRITE",
            format!(
                "target index {target_index} is outside destination type length {}",
                order.len()
            ),
        )));
    }
    let before = order.get(target_index).copied().map(resolved_position);
    let fields = model_record
        .fields()
        .iter()
        .map(|(name, value)| (name.as_str().to_string(), value.clone()))
        .collect();
    Ok(MutationExecutionPlan::Transfer(TransferPlan {
        coordinate: record_ref.coordinate.clone(),
        fields,
        source_origin: record_ref.origin.clone(),
        source_display_path: record_ref.display_path.clone(),
        source,
        source_writer,
        destination_display_path: destination_file.to_string(),
        destination,
        destination_writer,
        destination_sheet,
        before,
    }))
}

fn resolve_transfer_sheet(
    session: &ProjectSession,
    file: &str,
    actual_type: &str,
    requested: Option<&str>,
) -> Result<Option<String>, DiagnosticSet> {
    if let Some(sheet) = requested {
        return Ok(Some(sheet.to_string()));
    }
    let sheets = session
        .records
        .ids_in_file(file)
        .iter()
        .filter_map(|id| session.records.get(*id))
        .filter(|record| record.coordinate.actual_type.as_str() == actual_type)
        .filter_map(|record| match &record.origin {
            RecordOrigin::Table { sheet, .. } => Some(sheet.clone()),
            RecordOrigin::File { .. } | RecordOrigin::None => None,
        })
        .collect::<std::collections::BTreeSet<_>>();
    match sheets.len() {
        0 => Ok(None),
        1 => Ok(sheets.into_iter().next()),
        _ => Err(DiagnosticSet::one(Diagnostic::error(
            "WRITE-TRANSFER-SHEET",
            "WRITE",
            "destination file maps this type to multiple sheets; specify destination_sheet",
        ))),
    }
}

fn record_matches_sheet(record: &RecordRef, sheet: Option<&str>) -> bool {
    match (&record.origin, sheet) {
        (RecordOrigin::Table { sheet: actual, .. }, Some(expected)) => actual == expected,
        (RecordOrigin::Table { .. }, None) => true,
        (RecordOrigin::File { .. }, None) => true,
        (RecordOrigin::File { .. } | RecordOrigin::None, Some(_)) | (RecordOrigin::None, None) => {
            false
        }
    }
}

fn prepare_swap_records(
    session: &ProjectSession,
    registry: &ProviderRegistry,
    first: &RecordCoordinate,
    second: &RecordCoordinate,
) -> Result<MutationExecutionPlan, DiagnosticSet> {
    let first_ref = required_record_ref(session, first)?;
    let second_ref = required_record_ref(session, second)?;
    if first_ref.coordinate.actual_type != second_ref.coordinate.actual_type {
        return Err(DiagnosticSet::one(Diagnostic::error(
            "WRITE-REORDER-TYPE",
            "WRITE",
            "records must have the same actual type to exchange positions",
        )));
    }
    ensure_same_container(first_ref, second_ref)?;
    if first_ref.id == second_ref.id {
        return Ok(MutationExecutionPlan::Noop {
            coordinate: first_ref.coordinate.clone(),
        });
    }
    let (source, writer) = reorder_writer(session, registry, first_ref)?;
    Ok(MutationExecutionPlan::Reorder(ReorderPlan {
        source,
        writer,
        operation: ReorderOperation::Swap {
            first: resolved_position(first_ref),
            second: resolved_position(second_ref),
        },
        display_path: first_ref.display_path.clone(),
    }))
}

fn prepare_move_record(
    session: &ProjectSession,
    registry: &ProviderRegistry,
    record: &RecordCoordinate,
    target_index: usize,
) -> Result<MutationExecutionPlan, DiagnosticSet> {
    let record_ref = required_record_ref(session, record)?;
    let container = record_container(record_ref);
    let mut order = session
        .records
        .ids_in_file(&record_ref.display_path)
        .iter()
        .filter_map(|id| session.records.get(*id))
        .filter(|candidate| record_container(candidate) == container)
        .collect::<Vec<_>>();
    let old_index = order
        .iter()
        .position(|candidate| candidate.id == record_ref.id)
        .ok_or_else(|| reorder_invariant("record is missing from its source order index"))?;
    if target_index >= order.len() {
        return Err(DiagnosticSet::one(Diagnostic::error(
            "WRITE-REORDER-INDEX",
            "WRITE",
            format!(
                "target index {target_index} is outside record container length {}",
                order.len()
            ),
        )));
    }
    if target_index == old_index {
        return Ok(MutationExecutionPlan::Noop {
            coordinate: record_ref.coordinate.clone(),
        });
    }
    order.remove(old_index);
    let before = order.get(target_index).copied().map(resolved_position);
    let (source, writer) = reorder_writer(session, registry, record_ref)?;
    Ok(MutationExecutionPlan::Reorder(ReorderPlan {
        source,
        writer,
        operation: ReorderOperation::MoveBefore {
            record: resolved_position(record_ref),
            before,
        },
        display_path: record_ref.display_path.clone(),
    }))
}

fn required_record_ref<'a>(
    session: &'a ProjectSession,
    coordinate: &RecordCoordinate,
) -> Result<&'a RecordRef, DiagnosticSet> {
    session
        .records
        .get_by_coordinate(&coordinate.actual_type, &coordinate.key)
        .ok_or_else(|| DiagnosticSet::one(not_found(&coordinate.actual_type, &coordinate.key)))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RecordContainer {
    File(SourceId),
    Table(SourceId, String),
    None(SourceId),
}

fn record_container(record: &RecordRef) -> RecordContainer {
    match &record.origin {
        RecordOrigin::File { .. } => RecordContainer::File(record.source_id),
        RecordOrigin::Table { sheet, .. } => {
            RecordContainer::Table(record.source_id, sheet.clone())
        }
        RecordOrigin::None => RecordContainer::None(record.source_id),
    }
}

fn ensure_same_container(left: &RecordRef, right: &RecordRef) -> Result<(), DiagnosticSet> {
    if record_container(left) == record_container(right)
        && !matches!(record_container(left), RecordContainer::None(_))
    {
        return Ok(());
    }
    Err(DiagnosticSet::one(Diagnostic::error(
        "WRITE-REORDER-CONTAINER",
        "WRITE",
        "records must belong to the same writable file or table sheet",
    )))
}

fn reorder_writer(
    session: &ProjectSession,
    registry: &ProviderRegistry,
    record: &RecordRef,
) -> Result<(ResolvedSource, Arc<dyn SourceWriter>), DiagnosticSet> {
    if matches!(record.origin, RecordOrigin::None) {
        return Err(DiagnosticSet::one(Diagnostic::error(
            "WRITE-REORDER-ORIGIN",
            "WRITE",
            "record has no writable source origin",
        )));
    }
    let source = source_for_id(session, record.source_id)?;
    let writer = lookup_source_writer(registry, &source)?;
    if !writer.capabilities(&source).can_reorder_records {
        return Err(DiagnosticSet::one(Diagnostic::error(
            "WRITE-UNSUPPORTED",
            "WRITE",
            "writer does not support reordering records",
        )));
    }
    Ok((source, writer))
}

fn resolved_position(record: &RecordRef) -> ResolvedRecordPosition {
    ResolvedRecordPosition {
        coordinate: record.coordinate.clone(),
        origin: record.origin.clone(),
    }
}

fn reorder_invariant(message: &str) -> DiagnosticSet {
    DiagnosticSet::one(Diagnostic::error(
        "MUTATION-TXN-INVARIANT",
        "MUTATION",
        message,
    ))
}

fn prepare_write_field(
    session: &ProjectSession,
    registry: &ProviderRegistry,
    actual_type: &str,
    key: &str,
    path: &[WriteFieldPathSegment],
    new_value: &CfdValue,
    allow_noop: bool,
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
    if allow_noop
        && session.field_value(
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
    if record.key() == new_key {
        return Ok(RenamePlan::Noop {
            coordinate: target_ref.coordinate.clone(),
        });
    }
    let source = source_for_id(session, target_ref.source_id)?;
    let writer = lookup_source_writer(registry, &source)?;
    let reference_actions = reference_update_actions(session, registry, target_ref.id, new_key)?;
    let rewrite_actions =
        source_rewrite_actions(session, registry, target_ref.id, &record.key, new_key)?;
    let dimension_actions = dimension_record_actions(session, registry, &record.actual_type)?;
    Ok(RenamePlan::Write(Box::new(RenameWritePlan {
        old_coordinate: target_ref.coordinate.clone(),
        origin: target_ref.origin.clone(),
        display_path: target_ref.display_path.clone(),
        source,
        writer,
        reference_actions,
        rewrite_actions,
        dimension_actions,
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
    let dimension_actions = dimension_record_actions(session, registry, &record.actual_type)?;
    Ok(DeletePlan {
        coordinate: record_ref.coordinate.clone(),
        origin: model_record.origin.clone(),
        display_path: record_ref.display_path.clone(),
        source,
        writer,
        dimension_actions,
    })
}

fn dimension_record_actions(
    session: &ProjectSession,
    registry: &ProviderRegistry,
    actual_type: &str,
) -> Result<Vec<DimensionRecordAction>, DiagnosticSet> {
    let schema = session.schema();
    let mut actions = Vec::new();
    for (entry, field) in session.source_data.dimension_sources() {
        if field.is_singleton {
            continue;
        }
        let applies = schema
            .field(actual_type, &field.source_field)
            .is_some_and(|schema_field| {
                schema_field.declaring_type == field.source_type
                    && schema_field
                        .dimension
                        .as_ref()
                        .is_some_and(|binding| binding.dimension == field.dimension)
            });
        if !applies {
            continue;
        }
        let manager = registry
            .dimension_source_manager(&entry.source.provider_id)
            .ok_or_else(|| {
                transaction_invariant(format!(
                    "dimension source provider `{}` disappeared before record mutation planning",
                    entry.source.provider_id
                ))
            })?;
        actions.push(DimensionRecordAction {
            source: entry.source.clone(),
            manager,
            field: field.clone(),
        });
    }
    Ok(actions)
}

fn sheet_for_file_type(session: &ProjectSession, file: &str, actual_type: &str) -> Option<String> {
    for id in session.records.ids_in_file(file) {
        let Some(record_ref) = session.records.get(*id) else {
            continue;
        };
        let RecordOrigin::Table { sheet, .. } = &record_ref.origin else {
            continue;
        };
        if record_ref.coordinate.actual_type.as_str() == actual_type {
            return Some(sheet.clone());
        }
    }
    None
}

fn transaction_invariant(message: impl Into<String>) -> DiagnosticSet {
    DiagnosticSet::one(Diagnostic::error(
        "MUTATION-TXN-INVARIANT",
        "MUTATION",
        message,
    ))
}
