use std::sync::Arc;

use crate::api::{CfdSource, CfdSourceCatalog, Diagnostic, DiagnosticSet, WriteFieldPathSegment};
use crate::cfd_loader::CfdWriter;
use crate::data_model::{CfdValue, RecordOrigin};

use crate::dimensions::DimensionField;
use crate::indexes::{RecordRef, SourceId};
use crate::mutation::PreparedMutationOp;
use crate::{ProjectSession, RecordCoordinate};

use super::refs::{reference_update_actions, ReferenceUpdateAction};
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
    pub(super) source: CfdSource,
    pub(super) writer: Arc<CfdWriter>,
}

pub(crate) struct WriteFieldPlan {
    pub(super) target: WriteTarget,
    pub(super) source: CfdSource,
    pub(super) writer: Arc<CfdWriter>,
}

pub(crate) struct DimensionWritePlan {
    pub(super) source: CfdSource,
    pub(super) manager: Arc<CfdWriter>,
}

pub(crate) struct DimensionRecordAction {
    pub(super) source: CfdSource,
    pub(super) manager: Arc<CfdWriter>,
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
    pub(super) source: CfdSource,
    pub(super) writer: Arc<CfdWriter>,
    pub(super) reference_actions: Vec<ReferenceUpdateAction>,
    pub(super) dimension_actions: Vec<DimensionRecordAction>,
}

pub(crate) struct DeletePlan {
    pub(super) coordinate: RecordCoordinate,
    pub(super) origin: RecordOrigin,
    pub(super) display_path: String,
    pub(super) source: CfdSource,
    pub(super) writer: Arc<CfdWriter>,
    pub(super) dimension_actions: Vec<DimensionRecordAction>,
}

pub(crate) struct ReorderPlan {
    pub(super) source: CfdSource,
    pub(super) writer: Arc<CfdWriter>,
    pub(super) operation: ReorderOperation,
    pub(super) display_path: String,
}

pub(crate) struct TransferPlan {
    pub(super) coordinate: RecordCoordinate,
    pub(super) fields: std::collections::BTreeMap<String, CfdValue>,
    pub(super) source_origin: RecordOrigin,
    pub(super) source_display_path: String,
    pub(super) source: CfdSource,
    pub(super) source_writer: Arc<CfdWriter>,
    pub(super) destination_display_path: String,
    pub(super) destination: CfdSource,
    pub(super) destination_writer: Arc<CfdWriter>,
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
        mut visit: impl FnMut(&CfdSource) -> Result<(), E>,
    ) -> Result<(), E> {
        match self {
            Self::Insert(plan) => visit(&plan.source)?,
            Self::WriteField(plan) => visit(&plan.source)?,
            Self::WriteDimension(plan) => visit(&plan.source)?,
            Self::Rename(RenamePlan::Noop { .. }) | Self::Folded | Self::Noop { .. } => {}
            Self::Rename(RenamePlan::Write(plan)) => {
                visit(&plan.source)?;
                for action in &plan.reference_actions {
                    visit(action.source())?;
                }
                for action in &plan.dimension_actions {
                    visit(&action.source)?;
                }
            }
            Self::Delete(plan) => {
                visit(&plan.source)?;
                for action in &plan.dimension_actions {
                    visit(&action.source)?;
                }
            }
            Self::Reorder(plan) => visit(&plan.source)?,
            Self::Transfer(plan) => {
                visit(&plan.source)?;
                visit(&plan.destination)?;
            }
        }
        Ok(())
    }

    pub(crate) fn can_batch_field_write_with(&self, other: &Self) -> bool {
        let (Self::WriteField(left), Self::WriteField(right)) = (self, other) else {
            return false;
        };
        Arc::ptr_eq(&left.writer, &right.writer) && left.source.location == right.source.location
    }
}

#[allow(clippy::too_many_lines)]
pub(crate) fn prepare_mutation_execution(
    session: &ProjectSession,
    catalog: &CfdSourceCatalog,
    op: &PreparedMutationOp,
    allow_noop: bool,
) -> Result<MutationExecutionPlan, DiagnosticSet> {
    match op {
        PreparedMutationOp::InsertRecord { file, .. } => {
            let source = source_for_file(session, file)?;
            let writer = lookup_source_writer(catalog);
            Ok(MutationExecutionPlan::Insert(InsertPlan { source, writer }))
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
            prepare_rename(session, catalog, write_record, new_key)
                .map(MutationExecutionPlan::Rename)
        }
        PreparedMutationOp::SetField {
            write_record,
            path,
            value,
            ..
        } => prepare_write_field(
            session,
            catalog,
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
            let manager = catalog.dimension_source_manager();
            Ok(MutationExecutionPlan::WriteDimension(DimensionWritePlan {
                source,
                manager,
            }))
        }
        PreparedMutationOp::RenameRecord {
            record, new_key, ..
        } => prepare_rename(session, catalog, record, new_key).map(MutationExecutionPlan::Rename),
        PreparedMutationOp::DeleteRecord { record, .. } => {
            prepare_delete(session, catalog, record).map(MutationExecutionPlan::Delete)
        }
        PreparedMutationOp::SwapRecords { first, second, .. } => {
            prepare_swap_records(session, catalog, first, second)
        }
        PreparedMutationOp::MoveRecord {
            record,
            target_index,
            ..
        } => prepare_move_record(session, catalog, record, *target_index),
        PreparedMutationOp::TransferRecord {
            record,
            destination_file,
            target_index,
            ..
        } => prepare_transfer_record(session, catalog, record, destination_file, *target_index),
        PreparedMutationOp::FoldedSetField { .. }
        | PreparedMutationOp::FoldedRenameRecord { .. }
        | PreparedMutationOp::FoldedDeleteRecord { .. }
        | PreparedMutationOp::CancelledInsert { .. } => Ok(MutationExecutionPlan::Folded),
    }
}

fn prepare_transfer_record(
    session: &ProjectSession,
    catalog: &CfdSourceCatalog,
    record: &RecordCoordinate,
    destination_file: &str,
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
    let source_writer = lookup_source_writer(catalog);
    let destination = source_for_file(session, destination_file)?;
    let destination_writer = lookup_source_writer(catalog);

    let order = session
        .records
        .ids_in_file(destination_file)
        .iter()
        .filter_map(|id| session.records.get(*id))
        .filter(|candidate| candidate.coordinate.actual_type == record.actual_type)
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
        before,
    }))
}

fn prepare_swap_records(
    session: &ProjectSession,
    catalog: &CfdSourceCatalog,
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
    let (source, writer) = reorder_writer(session, catalog, first_ref)?;
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
    catalog: &CfdSourceCatalog,
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
    let (source, writer) = reorder_writer(session, catalog, record_ref)?;
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
    None(SourceId),
}

fn record_container(record: &RecordRef) -> RecordContainer {
    match &record.origin {
        RecordOrigin::File { .. } => RecordContainer::File(record.source_id),
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
        "records must belong to the same writable CFD file",
    )))
}

fn reorder_writer(
    session: &ProjectSession,
    catalog: &CfdSourceCatalog,
    record: &RecordRef,
) -> Result<(CfdSource, Arc<CfdWriter>), DiagnosticSet> {
    if matches!(record.origin, RecordOrigin::None) {
        return Err(DiagnosticSet::one(Diagnostic::error(
            "WRITE-REORDER-ORIGIN",
            "WRITE",
            "record has no writable source origin",
        )));
    }
    let source = source_for_id(session, record.source_id)?;
    let writer = lookup_source_writer(catalog);
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
    catalog: &CfdSourceCatalog,
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
    let target_record = session
        .model
        .record(record_ref.id)
        .ok_or_else(|| DiagnosticSet::one(not_found(actual_type, key)))?;
    let expected = write_rules::expected_type_for_record_path(
        session.schema(),
        target_record,
        &target.field_path,
        "WRITE-SHAPE",
        "WRITE",
    )?;
    write_rules::validate_value_for_write(
        session,
        session.schema(),
        &expected,
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
    let writer = lookup_source_writer(catalog);
    Ok(Some(WriteFieldPlan {
        target,
        source,
        writer,
    }))
}

fn prepare_rename(
    session: &ProjectSession,
    catalog: &CfdSourceCatalog,
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
    let writer = lookup_source_writer(catalog);
    let reference_actions = reference_update_actions(session, catalog, target_ref.id, new_key)?;
    let dimension_actions = dimension_record_actions(session, catalog, &record.actual_type)?;
    Ok(RenamePlan::Write(Box::new(RenameWritePlan {
        old_coordinate: target_ref.coordinate.clone(),
        origin: target_ref.origin.clone(),
        display_path: target_ref.display_path.clone(),
        source,
        writer,
        reference_actions,
        dimension_actions,
    })))
}

fn prepare_delete(
    session: &ProjectSession,
    catalog: &CfdSourceCatalog,
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
    let writer = lookup_source_writer(catalog);
    let dimension_actions = dimension_record_actions(session, catalog, &record.actual_type)?;
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
    catalog: &CfdSourceCatalog,
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
        let manager = catalog.dimension_source_manager();
        actions.push(DimensionRecordAction {
            source: entry.source.clone(),
            manager,
            field: field.clone(),
        });
    }
    Ok(actions)
}
