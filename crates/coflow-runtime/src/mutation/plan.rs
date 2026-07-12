use std::collections::BTreeMap;

use coflow_api::{Diagnostic, DiagnosticSet};

use crate::{ProjectSession, RecordCoordinate};

use super::prepare::{
    prepare_delete_on_pending_insert, prepare_one, prepare_rename_on_pending_insert,
    prepare_set_on_pending_insert, rename_pending_insert_references,
    rename_prepared_field_references, PendingInsertSetRequest,
};
use super::types::PreparedMutationOp;
use super::{MutationFailedOp, MutationOp, MutationRequest};

#[derive(Debug)]
pub(super) struct PlannedMutationOp {
    pub(super) index: usize,
    pub(super) op: PreparedMutationOp,
}

pub(super) fn plan_mutations(
    session: &ProjectSession,
    request: MutationRequest,
) -> (Vec<PlannedMutationOp>, Vec<MutationFailedOp>, bool, bool) {
    let MutationRequest {
        stop_on_write_error,
        ops,
    } = request;
    let mut planned = Vec::<PlannedMutationOp>::new();
    let mut pending_inserts = BTreeMap::<RecordCoordinate, usize>::new();
    let mut failed = Vec::new();
    let mut write_ok = true;

    for (index, op) in ops.into_iter().enumerate() {
        let op_name = mutation_op_name(&op);
        let insert_op = matches!(op, MutationOp::InsertRecord { .. });
        let result = prepare_planned_op(session, &mut planned, &mut pending_inserts, op);

        match result {
            Ok(op) => planned.push(PlannedMutationOp { index, op }),
            Err(diagnostics) => {
                write_ok = false;
                let terminal = is_terminal_prepare_error(insert_op, &diagnostics);
                failed.push(MutationFailedOp::from_diagnostics(
                    index,
                    op_name,
                    diagnostics,
                ));
                if stop_on_write_error || terminal {
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
    op: MutationOp,
) -> Result<PreparedMutationOp, DiagnosticSet> {
    match op {
        MutationOp::SetField {
            record,
            file,
            path,
            value,
        } => prepare_set_field(
            session,
            planned,
            pending_inserts,
            MutationOp::SetField {
                record,
                file,
                path,
                value,
            },
        ),
        MutationOp::InsertRecord {
            file,
            sheet,
            actual_type,
            key,
            fields,
            materialization,
        } => prepare_insert(
            session,
            planned.len(),
            pending_inserts,
            MutationOp::InsertRecord {
                file,
                sheet,
                actual_type,
                key,
                fields,
                materialization,
            },
        ),
        MutationOp::RenameRecord {
            record,
            file,
            new_key,
        } => {
            let Some(insert_index) = pending_inserts.get(&record).copied() else {
                return prepare_one(
                    session,
                    MutationOp::RenameRecord {
                        record,
                        file,
                        new_key,
                    },
                    pending_inserts,
                );
            };
            fold_pending_insert_rename(
                session,
                planned,
                pending_inserts,
                insert_index,
                &record,
                file.as_deref(),
                &new_key,
            )
        }
        MutationOp::DeleteRecord { record, file } => {
            let Some(insert_index) = pending_inserts.get(&record).copied() else {
                return prepare_one(
                    session,
                    MutationOp::DeleteRecord { record, file },
                    pending_inserts,
                );
            };
            fold_pending_insert_delete(
                planned,
                pending_inserts,
                insert_index,
                &record,
                file.as_deref(),
            )
        }
    }
}

fn prepare_set_field(
    session: &ProjectSession,
    planned: &mut [PlannedMutationOp],
    pending_inserts: &BTreeMap<RecordCoordinate, usize>,
    op: MutationOp,
) -> Result<PreparedMutationOp, DiagnosticSet> {
    let MutationOp::SetField {
        record,
        file,
        path,
        value,
    } = op
    else {
        return Err(mutation_invariant_error(
            "set-field preparation received a different operation",
        ));
    };
    let Some(insert_index) = pending_inserts.get(&record).copied() else {
        return prepare_one(
            session,
            MutationOp::SetField {
                record,
                file,
                path,
                value,
            },
            pending_inserts,
        );
    };
    let pending_insert = planned.get_mut(insert_index).ok_or_else(|| {
        mutation_invariant_error("pending insert index is outside the planned operation list")
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
            path: &path,
            value,
            pending_records: pending_inserts,
        },
    )
}

fn prepare_insert(
    session: &ProjectSession,
    planned_len: usize,
    pending_inserts: &mut BTreeMap<RecordCoordinate, usize>,
    op: MutationOp,
) -> Result<PreparedMutationOp, DiagnosticSet> {
    let MutationOp::InsertRecord {
        ref actual_type,
        ref key,
        ..
    } = op
    else {
        return Err(mutation_invariant_error(
            "insert preparation received a non-insert operation",
        ));
    };
    let coordinate = RecordCoordinate::new(actual_type, key);
    if pending_inserts.contains_key(&coordinate) {
        return Err(DiagnosticSet::one(Diagnostic::error(
            "MUTATION-INSERT-CONFLICT",
            "MUTATION",
            format!(
                "record `{}.{}` is inserted more than once in the same mutation",
                coordinate.actual_type, coordinate.key
            ),
        )));
    }
    let prepared = prepare_one(session, op, pending_inserts)?;
    if matches!(prepared, PreparedMutationOp::InsertRecord { .. }) {
        pending_inserts.insert(coordinate, planned_len);
    }
    Ok(prepared)
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

fn is_terminal_prepare_error(insert_op: bool, diagnostics: &DiagnosticSet) -> bool {
    diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "MUTATION-TXN-INVARIANT"
            || (insert_op && diagnostic.code == "MUTATION-INSERT-CONFLICT")
    })
}
