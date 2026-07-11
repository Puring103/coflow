use std::collections::{btree_map::Entry, BTreeMap};

use coflow_api::{Diagnostic, DiagnosticSet};

use crate::{ProjectSession, RecordCoordinate};

use super::prepare::{
    prepare_delete_on_pending_insert, prepare_one, prepare_rename_on_pending_insert,
    prepare_set_on_pending_insert, rename_pending_insert_references,
    rename_prepared_field_references, PendingInsertSetRequest,
};
use super::types::{PreparedMutation, PreparedMutationOp};
use super::{MutationFailedOp, MutationOp};

#[derive(Debug)]
pub(super) struct PlannedMutationOp {
    pub(super) index: usize,
    pub(super) op: PreparedMutationOp,
}

pub(super) fn plan_mutations(
    session: &ProjectSession,
    prepared: PreparedMutation,
) -> (Vec<PlannedMutationOp>, Vec<MutationFailedOp>, bool, bool) {
    let PreparedMutation {
        stop_on_write_error,
        ops,
    } = prepared;
    let mut planned = Vec::<PlannedMutationOp>::new();
    let mut pending_inserts = BTreeMap::<RecordCoordinate, usize>::new();
    let mut failed = Vec::new();
    let mut write_ok = true;

    for (index, pending) in ops.into_iter().enumerate() {
        let PreparedMutationOp::Pending { op } = pending else {
            continue;
        };
        let pending_records = pending_inserts.keys().cloned().collect::<Vec<_>>();
        let result = prepare_planned_op(
            session,
            &mut planned,
            &mut pending_inserts,
            &op,
            &pending_records,
        );

        match result {
            Ok(op) => planned.push(PlannedMutationOp { index, op }),
            Err(diagnostics) => {
                write_ok = false;
                failed.push(MutationFailedOp {
                    index,
                    op: mutation_op_name(&op).to_string(),
                    diagnostics: diagnostics.flat_diagnostics(),
                });
                if stop_on_write_error || is_terminal_prepare_error(&op, &diagnostics) {
                    return (Vec::new(), failed, false, true);
                }
            }
        }
    }
    (planned, failed, write_ok, false)
}

fn prepare_planned_op(
    session: &ProjectSession,
    planned: &mut [PlannedMutationOp],
    pending_inserts: &mut BTreeMap<RecordCoordinate, usize>,
    op: &MutationOp,
    pending_records: &[RecordCoordinate],
) -> Result<PreparedMutationOp, DiagnosticSet> {
    match op {
        MutationOp::SetField {
            record,
            file,
            path,
            value,
        } => {
            let Some(insert_index) = pending_inserts.get(record).copied() else {
                return prepare_one(session, op.clone(), pending_records);
            };
            let pending_insert = planned.get_mut(insert_index).ok_or_else(|| {
                mutation_invariant_error(
                    "pending insert index is outside the planned operation list",
                )
            })?;
            let PreparedMutationOp::InsertRecord {
                file: insert_file,
                actual_type,
                key,
                fields,
                ..
            } = &mut pending_insert.op
            else {
                return Err(mutation_invariant_error(
                    "pending insert index does not identify an insert operation",
                ));
            };
            prepare_set_on_pending_insert(
                session,
                PendingInsertSetRequest {
                    insert_file,
                    actual_type,
                    key,
                    fields,
                    file_guard: file.as_deref(),
                    path,
                    value: value.clone(),
                    pending_records,
                },
            )
        }
        MutationOp::InsertRecord {
            actual_type, key, ..
        } => {
            let coordinate = RecordCoordinate::new(actual_type, key);
            match pending_inserts.entry(coordinate) {
                Entry::Occupied(entry) => Err(DiagnosticSet::one(Diagnostic::error(
                    "MUTATION-INSERT-CONFLICT",
                    "MUTATION",
                    format!(
                        "record `{}.{}` is inserted more than once in the same mutation",
                        entry.key().actual_type,
                        entry.key().key
                    ),
                ))),
                Entry::Vacant(entry) => {
                    prepare_one(session, op.clone(), pending_records).inspect(|prepared_op| {
                        if matches!(prepared_op, PreparedMutationOp::InsertRecord { .. }) {
                            entry.insert(planned.len());
                        }
                    })
                }
            }
        }
        MutationOp::RenameRecord {
            record,
            file,
            new_key,
        } => {
            let Some(insert_index) = pending_inserts.get(record).copied() else {
                return prepare_one(session, op.clone(), pending_records);
            };
            fold_pending_insert_rename(
                session,
                planned,
                pending_inserts,
                insert_index,
                record,
                file.as_deref(),
                new_key,
            )
        }
        MutationOp::DeleteRecord { record, file } => {
            let Some(insert_index) = pending_inserts.get(record).copied() else {
                return prepare_one(session, op.clone(), pending_records);
            };
            fold_pending_insert_delete(
                planned,
                pending_inserts,
                insert_index,
                record,
                file.as_deref(),
            )
        }
    }
}

fn fold_pending_insert_rename(
    session: &ProjectSession,
    planned: &mut [PlannedMutationOp],
    pending_inserts: &mut BTreeMap<RecordCoordinate, usize>,
    insert_index: usize,
    record: &RecordCoordinate,
    file_guard: Option<&str>,
    new_key: &str,
) -> Result<PreparedMutationOp, DiagnosticSet> {
    let Some(pending_insert) = planned.get(insert_index) else {
        return Err(mutation_invariant_error(
            "pending rename index is outside the planned operation list",
        ));
    };
    let PreparedMutationOp::InsertRecord { file, .. } = &pending_insert.op else {
        return Err(mutation_invariant_error(
            "pending rename index does not identify an insert operation",
        ));
    };
    let insert_file = file.clone();
    let folded =
        prepare_rename_on_pending_insert(session, &insert_file, record, file_guard, new_key)?;
    let new_record = RecordCoordinate::new(&record.actual_type, new_key);
    if new_record != *record && pending_inserts.contains_key(&new_record) {
        return Err(DiagnosticSet::one(Diagnostic::error(
            "MUTATION-RENAME-CONFLICT",
            "MUTATION",
            format!(
                "record `{}.{}` is already pending insertion in the same mutation",
                new_record.actual_type, new_record.key
            ),
        )));
    }
    for planned_op in planned.iter_mut() {
        match &mut planned_op.op {
            PreparedMutationOp::InsertRecord {
                actual_type,
                fields,
                ..
            } => rename_pending_insert_references(
                session,
                &record.actual_type,
                actual_type,
                fields,
                &record.key,
                new_key,
            )?,
            PreparedMutationOp::SetField {
                write_record,
                path,
                value,
                ..
            } => rename_prepared_field_references(
                session,
                &record.actual_type,
                &write_record.actual_type,
                path,
                value,
                &record.key,
                new_key,
            )?,
            _ => {}
        }
    }
    let Some(pending_insert) = planned.get_mut(insert_index) else {
        return Err(mutation_invariant_error(
            "pending rename index disappeared during planning",
        ));
    };
    let PreparedMutationOp::InsertRecord { key, .. } = &mut pending_insert.op else {
        return Err(mutation_invariant_error(
            "pending rename target changed during planning",
        ));
    };
    *key = new_key.to_string();
    pending_inserts.remove(record);
    pending_inserts.insert(new_record, insert_index);
    Ok(folded)
}

fn fold_pending_insert_delete(
    planned: &mut [PlannedMutationOp],
    pending_inserts: &mut BTreeMap<RecordCoordinate, usize>,
    insert_index: usize,
    record: &RecordCoordinate,
    file_guard: Option<&str>,
) -> Result<PreparedMutationOp, DiagnosticSet> {
    let Some(pending_insert) = planned.get(insert_index) else {
        return Err(mutation_invariant_error(
            "pending delete index is outside the planned operation list",
        ));
    };
    let PreparedMutationOp::InsertRecord { file, .. } = &pending_insert.op else {
        return Err(mutation_invariant_error(
            "pending delete index does not identify an insert operation",
        ));
    };
    let insert_file = file.clone();
    let folded = prepare_delete_on_pending_insert(&insert_file, record, file_guard)?;
    let Some(pending_insert) = planned.get_mut(insert_index) else {
        return Err(mutation_invariant_error(
            "pending delete index disappeared during planning",
        ));
    };
    pending_insert.op = PreparedMutationOp::CancelledInsert {
        record: record.clone(),
        write_file: insert_file,
    };
    pending_inserts.remove(record);
    Ok(folded)
}

fn mutation_invariant_error(message: &str) -> DiagnosticSet {
    DiagnosticSet::one(Diagnostic::error(
        "MUTATION-TXN-INVARIANT",
        "MUTATION",
        message,
    ))
}

pub(super) const fn mutation_op_name(op: &MutationOp) -> &'static str {
    match op {
        MutationOp::InsertRecord { .. } => "insert_record",
        MutationOp::SetField { .. } => "set_field",
        MutationOp::RenameRecord { .. } => "rename_record",
        MutationOp::DeleteRecord { .. } => "delete_record",
    }
}

fn is_terminal_prepare_error(op: &MutationOp, diagnostics: &DiagnosticSet) -> bool {
    diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "MUTATION-TXN-INVARIANT"
            || (matches!(op, MutationOp::InsertRecord { .. })
                && diagnostic.code == "MUTATION-INSERT-CONFLICT")
    })
}
