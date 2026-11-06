use std::sync::Arc;

use crate::api::{CfdSource, CfdSourceCatalog, Diagnostic, DiagnosticSet};
use crate::cfd_loader::CfdWriter;
use crate::data_model::{CfdObject, CfdValue, RecordOrigin};
use coflow_language::cft::RecordId;
use std::collections::BTreeMap;

use crate::indexes::{RecordRef, SourceId};
use crate::mutation::{DimensionSourceCoordinate, PreparedMutationOp, RecordRestoreSnapshot};
use crate::{ProjectSession, RecordCoordinate};

use super::refs::{reference_update_actions, ReferenceUpdateAction};
use super::target::{not_found, WriteTarget};
use super::writer::{
    dimension_source_for_file, lookup_source_writer, source_for_file, source_for_id,
};

/// 准备后的值与已解析的写入目标共同组成唯一执行表示，不再维护两套配对枚举。
pub(crate) enum ExecutableMutationOp {
    Restore {
        snapshot: RecordRestoreSnapshot,
        writer: Arc<CfdWriter>,
        sources: Vec<CfdSource>,
    },
    Insert {
        plan: InsertPlan,
        coordinate: RecordCoordinate,
        fields: BTreeMap<String, CfdValue>,
    },
    WriteField(FieldWrite),
    UnsetField(WriteFieldPlan),
    WriteDimension {
        plan: DimensionWritePlan,
        record: RecordCoordinate,
        coordinate: DimensionSourceCoordinate,
        new_value: Option<CfdValue>,
    },
    Rename {
        plan: RenamePlan,
        new_id: RecordId,
    },
    Delete(DeletePlan),
    Reorder(ReorderPlan),
    Transfer(TransferPlan),
    Noop {
        coordinate: RecordCoordinate,
    },
    Folded(crate::WriteOutcome),
}

pub(crate) struct FieldWrite {
    pub(super) plan: WriteFieldPlan,
    pub(super) value: CfdValue,
    pub(super) materialized_top_level: Option<Arc<CfdObject>>,
}

impl FieldWrite {
    pub(crate) fn batch_key(&self) -> (&std::path::Path, *const CfdWriter) {
        (
            self.plan.source.location.path(),
            Arc::as_ptr(&self.plan.writer),
        )
    }
}

pub(crate) struct InsertPlan {
    pub(super) display_path: String,
    pub(super) source: CfdSource,
    pub(super) writer: Arc<CfdWriter>,
}

pub(crate) struct WriteFieldPlan {
    pub(super) target: WriteTarget,
    pub(super) source: CfdSource,
    pub(super) writer: Arc<CfdWriter>,
}

pub(crate) struct DimensionWritePlan {
    pub(super) display_path: String,
    pub(super) source: CfdSource,
    pub(super) manager: Arc<CfdWriter>,
}

pub(crate) struct DimensionRecordAction {
    pub(super) source: CfdSource,
    pub(super) manager: Arc<CfdWriter>,
    pub(super) actual_type: String,
}

pub(crate) enum RenamePlan {
    Noop { coordinate: RecordCoordinate },
    Write(Box<RenameWritePlan>),
}

pub(crate) struct RenameWritePlan {
    pub(super) old_coordinate: RecordCoordinate,
    pub(super) origin: RecordOrigin,
    pub(super) display_path: String,
    pub(super) writer: Arc<CfdWriter>,
    pub(super) reference_actions: Vec<ReferenceUpdateAction>,
    pub(super) dimension_actions: Vec<DimensionRecordAction>,
}

pub(crate) struct DeletePlan {
    pub(super) coordinate: RecordCoordinate,
    pub(super) origin: RecordOrigin,
    pub(super) display_path: String,
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
    pub(super) source_origin: RecordOrigin,
    pub(super) source_display_path: String,
    pub(super) source_writer: Arc<CfdWriter>,
    pub(super) destination_display_path: String,
    pub(super) destination: CfdSource,
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

impl ExecutableMutationOp {
    /// 只有纯字段写入可以跨文件重组；结构、维度及引用改名操作形成顺序屏障。
    pub(crate) const fn field_write(&self) -> Option<&FieldWrite> {
        match self {
            Self::WriteField(write) => Some(write),
            _ => None,
        }
    }

    pub(crate) const fn deleted_record(&self) -> Option<&RecordCoordinate> {
        match self {
            Self::Delete(plan) => Some(&plan.coordinate),
            _ => None,
        }
    }

    pub(crate) const fn changes_generation(&self) -> bool {
        !matches!(
            self,
            Self::Rename {
                plan: RenamePlan::Noop { .. },
                ..
            } | Self::Noop { .. }
                | Self::Folded(_)
        )
    }
}

#[allow(clippy::too_many_lines)]
pub(crate) fn prepare_mutation_execution(
    session: &ProjectSession,
    catalog: &CfdSourceCatalog,
    op: PreparedMutationOp,
    allow_noop: bool,
) -> Result<ExecutableMutationOp, DiagnosticSet> {
    match op {
        PreparedMutationOp::RestoreRecord { snapshot } => {
            let sources = snapshot
                .patches
                .iter()
                .map(|patch| source_for_file(session, &patch.file))
                .collect::<Result<_, _>>()?;
            Ok(ExecutableMutationOp::Restore {
                snapshot,
                writer: catalog.writer(),
                sources,
            })
        }
        PreparedMutationOp::InsertRecord {
            file,
            actual_type,
            id,
            fields,
        } => {
            let source = source_for_file(session, &file)?;
            let writer = lookup_source_writer(catalog);
            Ok(ExecutableMutationOp::Insert {
                plan: InsertPlan {
                    display_path: file,
                    source,
                    writer,
                },
                coordinate: RecordCoordinate::new(actual_type, id),
                fields,
            })
        }
        PreparedMutationOp::SetField {
            target,
            value,
            materialized_top_level,
        } => {
            if allow_noop
                && session.field_value(
                    &target.coordinate.actual_type,
                    &target.coordinate.id,
                    &target.field_path,
                ) == Some(&value)
            {
                return Ok(ExecutableMutationOp::Noop {
                    coordinate: target.coordinate,
                });
            }
            let source = source_for_id(session, target.source_id)?;
            Ok(ExecutableMutationOp::WriteField(FieldWrite {
                plan: WriteFieldPlan {
                    target,
                    source,
                    writer: catalog.writer(),
                },
                value,
                materialized_top_level,
            }))
        }
        PreparedMutationOp::WriteDimensionValue {
            write_file,
            record,
            coordinate,
            new_value,
        } => {
            let source = dimension_source_for_file(session, &write_file)?;
            Ok(ExecutableMutationOp::WriteDimension {
                plan: DimensionWritePlan {
                    display_path: write_file,
                    source,
                    manager: catalog.dimension_source_manager(),
                },
                record,
                coordinate,
                new_value,
            })
        }
        PreparedMutationOp::UnsetField { target } => {
            let source = source_for_id(session, target.source_id)?;
            Ok(ExecutableMutationOp::UnsetField(WriteFieldPlan {
                target,
                source,
                writer: catalog.writer(),
            }))
        }
        PreparedMutationOp::RenameRecord { record, new_id, .. } => {
            let plan = prepare_rename(session, catalog, &record, &new_id)?;
            Ok(ExecutableMutationOp::Rename { plan, new_id })
        }
        PreparedMutationOp::DeleteRecord { record, .. } => {
            prepare_delete(session, catalog, &record).map(ExecutableMutationOp::Delete)
        }
        PreparedMutationOp::SwapRecords { first, second, .. } => {
            prepare_swap_records(session, catalog, &first, &second)
        }
        PreparedMutationOp::MoveRecord {
            record,
            target_index,
            ..
        } => prepare_move_record(session, catalog, &record, target_index),
        PreparedMutationOp::TransferRecord {
            record,
            destination_file,
            target_index,
            ..
        } => prepare_transfer_record(session, catalog, &record, &destination_file, target_index),
        PreparedMutationOp::FoldedSetField { record, .. } => Ok(ExecutableMutationOp::Folded(
            crate::WriteOutcome::touch(record),
        )),
        PreparedMutationOp::FoldedDeleteRecord { record, .. } => {
            Ok(ExecutableMutationOp::Folded(crate::WriteOutcome {
                deleted: Some(record),
                ..crate::WriteOutcome::default()
            }))
        }
        PreparedMutationOp::CancelledInsert { record, .. } => {
            Ok(ExecutableMutationOp::Folded(crate::WriteOutcome {
                touched: vec![record.clone()],
                inserted: Some(record),
                ..crate::WriteOutcome::default()
            }))
        }
    }
}

fn prepare_transfer_record(
    session: &ProjectSession,
    catalog: &CfdSourceCatalog,
    record: &RecordCoordinate,
    destination_file: &str,
    target_index: usize,
) -> Result<ExecutableMutationOp, DiagnosticSet> {
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
    let source_writer = lookup_source_writer(catalog);
    let destination = source_for_file(session, destination_file)?;

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
    Ok(ExecutableMutationOp::Transfer(TransferPlan {
        coordinate: record_ref.coordinate.clone(),
        source_origin: record_ref.origin.clone(),
        source_display_path: record_ref.display_path.clone(),
        source_writer,
        destination_display_path: destination_file.to_string(),
        destination,
        before,
    }))
}

fn prepare_swap_records(
    session: &ProjectSession,
    catalog: &CfdSourceCatalog,
    first: &RecordCoordinate,
    second: &RecordCoordinate,
) -> Result<ExecutableMutationOp, DiagnosticSet> {
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
        return Ok(ExecutableMutationOp::Noop {
            coordinate: first_ref.coordinate.clone(),
        });
    }
    let (source, writer) = reorder_writer(session, catalog, first_ref)?;
    Ok(ExecutableMutationOp::Reorder(ReorderPlan {
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
) -> Result<ExecutableMutationOp, DiagnosticSet> {
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
        return Ok(ExecutableMutationOp::Noop {
            coordinate: record_ref.coordinate.clone(),
        });
    }
    order.remove(old_index);
    let before = order.get(target_index).copied().map(resolved_position);
    let (source, writer) = reorder_writer(session, catalog, record_ref)?;
    Ok(ExecutableMutationOp::Reorder(ReorderPlan {
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
        .get_by_coordinate(&coordinate.actual_type, &coordinate.id)
        .ok_or_else(|| DiagnosticSet::one(not_found(&coordinate.actual_type, &coordinate.id)))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RecordContainer {
    File(SourceId),
    None(SourceId),
}

const fn record_container(record: &RecordRef) -> RecordContainer {
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

fn prepare_rename(
    session: &ProjectSession,
    catalog: &CfdSourceCatalog,
    record: &RecordCoordinate,
    new_id: &RecordId,
) -> Result<RenamePlan, DiagnosticSet> {
    let Some(target_ref) = session
        .records
        .get_by_coordinate(&record.actual_type, &record.id)
    else {
        return Err(DiagnosticSet::one(not_found(
            &record.actual_type,
            &record.id,
        )));
    };
    if record.id == *new_id {
        return Ok(RenamePlan::Noop {
            coordinate: target_ref.coordinate.clone(),
        });
    }
    let writer = lookup_source_writer(catalog);
    let reference_actions = reference_update_actions(session, catalog, target_ref.id, new_id)?;
    let dimension_actions = dimension_record_actions(session, catalog, &record.actual_type);
    Ok(RenamePlan::Write(Box::new(RenameWritePlan {
        old_coordinate: target_ref.coordinate.clone(),
        origin: target_ref.origin.clone(),
        display_path: target_ref.display_path.clone(),
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
        .get_by_coordinate(&record.actual_type, &record.id)
    else {
        return Err(DiagnosticSet::one(not_found(
            &record.actual_type,
            &record.id,
        )));
    };
    let Some(model_record) = session.model.record(record_ref.id) else {
        return Err(DiagnosticSet::one(not_found(
            &record.actual_type,
            &record.id,
        )));
    };
    let writer = lookup_source_writer(catalog);
    let dimension_actions = dimension_record_actions(session, catalog, &record.actual_type);
    Ok(DeletePlan {
        coordinate: record_ref.coordinate.clone(),
        origin: model_record.origin.clone(),
        display_path: record_ref.display_path.clone(),
        writer,
        dimension_actions,
    })
}

fn dimension_record_actions(
    session: &ProjectSession,
    catalog: &CfdSourceCatalog,
    actual_type: &str,
) -> Vec<DimensionRecordAction> {
    session
        .source_data
        .overlay_sources()
        .map(|entry| DimensionRecordAction {
            source: CfdSource {
                location: entry.source.location.clone(),
                display_name: entry.display_path.clone(),
            },
            manager: catalog.dimension_source_manager(),
            actual_type: actual_type.to_string(),
        })
        .collect()
}
