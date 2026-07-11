use coflow_api::{
    DeleteRecordRequest, Diagnostic, DiagnosticSet, InsertRecordRequest, ProviderRegistry,
    RenameRecordRequest, WriteCellRequest, WriteContext, WriteFieldPathSegment,
};
use coflow_data_model::CfdValue;

use crate::mutation::PreparedMutationOp;
use crate::{ProjectSession, RecordCoordinate, WriteOutcome};

use super::plan::prepare_write_field;
use super::refs::{reference_update_actions, source_rewrite_actions};
use super::target::{is_id_path, not_found};
use super::writer::{lookup_source_writer, source_for_file};

pub(crate) fn preflight_mutation_op(
    session: &ProjectSession,
    registry: &ProviderRegistry,
    op: &PreparedMutationOp,
) -> Result<(), DiagnosticSet> {
    let PreparedMutationOp::SetField {
        write_record,
        path,
        value,
        ..
    } = op
    else {
        return Ok(());
    };
    if is_id_path(path) {
        return Ok(());
    }
    let plan = prepare_write_field(
        session,
        registry,
        &write_record.actual_type,
        &write_record.key,
        path,
        value,
    )?;
    let compiled_schema = session.compiled_schema();
    let request = WriteCellRequest {
        origin: &plan.target.origin,
        record_key: &plan.target.coordinate.key,
        actual_type: &plan.target.coordinate.actual_type,
        field_path: &plan.target.field_path,
        new_value: value,
        schema: &compiled_schema,
        source: &plan.source,
    };
    let diagnostics = plan.writer.preflight(
        WriteContext {
            project_root: &session.project.root_dir,
            schema: &compiled_schema,
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
    registry: &ProviderRegistry,
    op: &PreparedMutationOp,
) -> Result<WriteOutcome, DiagnosticSet> {
    match op {
        PreparedMutationOp::InsertRecord {
            file,
            sheet,
            actual_type,
            key,
            fields,
        } => stage_insert_record(
            session,
            registry,
            file,
            sheet.as_deref(),
            key,
            actual_type,
            fields,
        ),
        PreparedMutationOp::SetField {
            record,
            write_record,
            path,
            value,
            ..
        } => stage_write_field(
            session,
            registry,
            record,
            write_record,
            path,
            value,
        ),
        PreparedMutationOp::RenameRecord {
            record, new_key, ..
        } => stage_rename_record_key(
            session,
            registry,
            &record.actual_type,
            &record.key,
            new_key,
        ),
        PreparedMutationOp::DeleteRecord { record, .. } => {
            stage_delete_record(session, registry, &record.actual_type, &record.key)
        }
        PreparedMutationOp::FoldedSetField { record, .. } => {
            Ok(WriteOutcome::touch(record.clone()))
        }
        PreparedMutationOp::FoldedRenameRecord {
            old_record,
            new_record,
            ..
        } => Ok(WriteOutcome {
            touched: vec![old_record.clone(), new_record.clone()],
            inserted: None,
            deleted: None,
            renamed: Some((old_record.clone(), new_record.clone())),
            diagnostics: DiagnosticSet::empty(),
        }),
        PreparedMutationOp::FoldedDeleteRecord { record, .. } => Ok(WriteOutcome {
            touched: Vec::new(),
            inserted: None,
            deleted: Some(record.clone()),
            renamed: None,
            diagnostics: DiagnosticSet::empty(),
        }),
        PreparedMutationOp::CancelledInsert { record, .. } => Ok(WriteOutcome {
            touched: vec![record.clone()],
            inserted: Some(record.clone()),
            deleted: None,
            renamed: None,
            diagnostics: DiagnosticSet::empty(),
        }),
        PreparedMutationOp::Pending { .. } => Err(DiagnosticSet::one(Diagnostic::error(
            "WRITE-TXN-PLAN",
            "WRITE",
            "mutation operation reached staging before planning completed",
        ))),
    }
}

fn stage_write_field(
    session: &ProjectSession,
    registry: &ProviderRegistry,
    host_record: &RecordCoordinate,
    write_record: &RecordCoordinate,
    path: &[WriteFieldPathSegment],
    new_value: &CfdValue,
) -> Result<WriteOutcome, DiagnosticSet> {
    if is_id_path(path) {
        let CfdValue::String(new_key) = new_value else {
            return Err(DiagnosticSet::one(Diagnostic::error(
                "WRITE-RENAME",
                "WRITE",
                "record key writes require a string value",
            )));
        };
        let mut outcome = stage_rename_record_key(
            session,
            registry,
            &write_record.actual_type,
            &write_record.key,
            new_key,
        )?;
        if host_record != write_record {
            outcome.touched.insert(0, host_record.clone());
        }
        return Ok(outcome);
    }
    let plan = prepare_write_field(
        session,
        registry,
        &write_record.actual_type,
        &write_record.key,
        path,
        new_value,
    )?;
    let compiled_schema = session.compiled_schema();
    let request = WriteCellRequest {
        origin: &plan.target.origin,
        record_key: &plan.target.coordinate.key,
        actual_type: &plan.target.coordinate.actual_type,
        field_path: &plan.target.field_path,
        new_value,
        schema: &compiled_schema,
        source: &plan.source,
    };
    let ctx = WriteContext {
        project_root: &session.project.root_dir,
        schema: &compiled_schema,
        model: Some(&session.model),
    };
    let provider_outcome = plan.writer.write_field(ctx, &request)?;
    Ok(WriteOutcome {
        touched: if host_record == write_record {
            vec![host_record.clone()]
        } else {
            vec![host_record.clone(), plan.host_coordinate]
        },
        inserted: None,
        deleted: None,
        renamed: None,
        diagnostics: provider_outcome.diagnostics,
    })
}

fn stage_rename_record_key(
    session: &ProjectSession,
    registry: &ProviderRegistry,
    actual_type: &str,
    old_key: &str,
    new_key: &str,
) -> Result<WriteOutcome, DiagnosticSet> {
    let Some(target_ref) = session.records.get_by_coordinate(actual_type, old_key) else {
        return Err(DiagnosticSet::one(not_found(actual_type, old_key)));
    };
    if old_key == new_key {
        return Ok(WriteOutcome::touch(target_ref.coordinate.clone()));
    }

    let old_coordinate = target_ref.coordinate.clone();
    let target_source = source_for_file(session, &target_ref.display_path)?;
    let target_writer = lookup_source_writer(registry, &target_source)?;
    let compiled_schema = session.compiled_schema();
    let ctx = WriteContext {
        project_root: &session.project.root_dir,
        schema: &compiled_schema,
        model: Some(&session.model),
    };
    let target_request = RenameRecordRequest {
        origin: &target_ref.origin,
        old_key,
        new_key,
        actual_type,
        source: &target_source,
        schema: &compiled_schema,
    };
    let reference_actions =
        reference_update_actions(session, registry, target_ref.id, new_key)?;
    let rewrite_actions =
        source_rewrite_actions(session, registry, target_ref.id, old_key, new_key)?;

    target_writer.rename_record(ctx, &target_request)?;
    for action in &reference_actions {
        action
            .writer
            .write_field(ctx, &action.request.as_request(&compiled_schema))?;
    }
    for action in &rewrite_actions {
        action
            .writer
            .rewrite_record_references(ctx, &action.request.as_request(&compiled_schema))?;
    }

    let new_coordinate = RecordCoordinate::new(actual_type, new_key);
    Ok(WriteOutcome {
        touched: vec![old_coordinate.clone(), new_coordinate.clone()],
        inserted: None,
        deleted: None,
        renamed: Some((old_coordinate, new_coordinate)),
        diagnostics: DiagnosticSet::empty(),
    })
}

fn stage_insert_record(
    session: &ProjectSession,
    registry: &ProviderRegistry,
    file: &str,
    sheet: Option<&str>,
    record_key: &str,
    actual_type: &str,
    fields: &std::collections::BTreeMap<String, CfdValue>,
) -> Result<WriteOutcome, DiagnosticSet> {
    let source = source_for_file(session, file)?;
    let sheet = sheet
        .map(ToOwned::to_owned)
        .or_else(|| sheet_for_file_type(session, file, actual_type));
    let writer = lookup_source_writer(registry, &source)?;
    let compiled_schema = session.compiled_schema();
    let request = InsertRecordRequest {
        source: &source,
        sheet: sheet.as_deref(),
        record_key,
        actual_type,
        fields,
        schema: &compiled_schema,
    };
    let ctx = WriteContext {
        project_root: &session.project.root_dir,
        schema: &compiled_schema,
        model: Some(&session.model),
    };
    let provider_outcome = writer.insert_record(ctx, &request)?;
    let inserted = RecordCoordinate::new(actual_type, record_key);
    Ok(WriteOutcome {
        touched: vec![inserted.clone()],
        inserted: Some(inserted),
        deleted: None,
        renamed: None,
        diagnostics: provider_outcome.diagnostics,
    })
}

fn stage_delete_record(
    session: &ProjectSession,
    registry: &ProviderRegistry,
    actual_type: &str,
    key: &str,
) -> Result<WriteOutcome, DiagnosticSet> {
    let Some(record_ref) = session.records.get_by_coordinate(actual_type, key) else {
        return Err(DiagnosticSet::one(not_found(actual_type, key)));
    };
    let Some(record) = session.model.record(record_ref.id) else {
        return Err(DiagnosticSet::one(not_found(actual_type, key)));
    };
    let coordinate = record_ref.coordinate.clone();
    let source = source_for_file(session, &record_ref.display_path)?;
    let writer = lookup_source_writer(registry, &source)?;
    let compiled_schema = session.compiled_schema();
    let request = DeleteRecordRequest {
        origin: &record.origin,
        record_key: key,
        actual_type,
        source: &source,
    };
    let ctx = WriteContext {
        project_root: &session.project.root_dir,
        schema: &compiled_schema,
        model: Some(&session.model),
    };
    let provider_outcome = writer.delete_record(ctx, &request)?;
    Ok(WriteOutcome {
        touched: Vec::new(),
        inserted: None,
        deleted: Some(coordinate),
        renamed: None,
        diagnostics: provider_outcome.diagnostics,
    })
}

fn sheet_for_file_type(session: &ProjectSession, file: &str, actual_type: &str) -> Option<String> {
    for id in session.records.ids_in_file(file) {
        let Some(record_ref) = session.records.get(*id) else {
            continue;
        };
        let coflow_data_model::RecordOrigin::Table { sheet, .. } = &record_ref.origin else {
            continue;
        };
        if record_ref.coordinate.actual_type == actual_type {
            return Some(sheet.clone());
        }
    }
    None
}
