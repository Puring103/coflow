//! 会话 mutation 应用层：字段写回、集合编辑、草稿转 wire、mutation 收尾。
//!
//! 引擎拥有校验与落盘，本层只做 `MutationReport` 到编辑器 DTO 的包装。

use coflow_project::CfdValue;

use super::errors::{api_diagnostics_to_editor_error, mutation_report_to_editor_error};
use super::EditorSession;
use crate::editor::convert::{annotation_for_draft_field, WireContext};
use crate::editor::types::{
    CreateRecordDraft, CreateRecordFieldDraft, EditorError, WriteFieldOutcome,
};

/// 字段写回：先取 `old_value` 供 undo 使用，再走引擎 `SetField`。
pub(crate) fn write_field_in_session(
    session: &mut EditorSession,
    coordinate: &coflow_project::RecordCoordinate,
    field_path: &[coflow_project::CfdPathSegment],
    new_value: &CfdValue,
) -> Result<WriteFieldOutcome, EditorError> {
    let old_value = session
        .queries()
        .effective_field_write(coordinate, field_path)
        .and_then(|preview| preview.old_value);
    let report = coflow_project::commands::apply_project_mutation(
        &mut session.engine,
        coflow_project::MutationRequest {
            stop_on_write_error: true,
            ops: vec![coflow_project::MutationOp::SetField {
                record: coordinate.clone(),
                file: None,
                path: field_path.to_vec(),
                value: coflow_project::MutationValue::Cfd(new_value.clone()),
            }],
        },
    )
    .map_err(api_diagnostics_to_editor_error)?;
    let (report, changes) = finalize_mutation(session, report, "write field failed")?;
    let outcome = report
        .applied
        .first()
        .map(|applied| &applied.outcome)
        .ok_or_else(|| EditorError::write("write field did not apply"))?;
    let renamed = outcome
        .renamed
        .as_ref()
        .and_then(|(old, new)| (old == coordinate).then_some(new.clone()));
    let final_coordinate = renamed.as_ref().unwrap_or(coordinate);
    let queries = session.queries();
    let current_value = queries
        .field_value(
            &final_coordinate.actual_type,
            &final_coordinate.key,
            field_path,
        )
        .cloned();
    Ok(WriteFieldOutcome {
        changes,
        revision: session.revisions.current(),
        diagnostics: report.diagnostics,
        old_value,
        new_value: current_value,
        affected_files: report.affected_files,
        renamed,
    })
}

/// mutation 收尾：失败转错误，成功则刷新诊断并提交内部写版本。
pub(crate) fn finalize_mutation(
    session: &mut EditorSession,
    report: coflow_project::MutationReport,
    fallback: &str,
) -> Result<
    (
        coflow_project::MutationReport,
        crate::editor::types::records::EditorChangeSet,
    ),
    EditorError,
> {
    if !report.write_ok {
        return Err(mutation_report_to_editor_error(fallback, &report));
    }
    let base_revision = session.revisions.current();
    if report.generation_changed {
        let affected = report.changed_records.keys().cloned().collect();
        session.publish_commit(&report.written_files, Some(&affected), false)?;
    }
    let files = report
        .changed_records
        .iter()
        .map(|(file, changed)| {
            let order = session
                .queries()
                .record_views_in_file(file)
                .map(|view| view.coordinate)
                .collect();
            let changed = changed.iter().collect::<std::collections::BTreeSet<_>>();
            let data = super::row_build::file_records_selection(session, file, Some(&changed));
            crate::editor::types::records::FileRecordsPatch { data, order }
        })
        .collect();
    let changes = crate::editor::types::records::EditorChangeSet {
        base_revision,
        revision: session.revisions.current(),
        files,
    };
    Ok((report, changes))
}

pub(crate) fn create_record_draft_to_wire(
    draft: &coflow_project::CreateRecordDraft,
    ctx: &WireContext<'_>,
) -> CreateRecordDraft {
    CreateRecordDraft {
        actual_type: draft.actual_type.clone(),
        fields: draft
            .fields
            .iter()
            .map(|field| create_record_field_draft_to_wire(&draft.actual_type, field, ctx))
            .collect(),
    }
}

fn create_record_field_draft_to_wire(
    actual_type: &str,
    field: &coflow_project::CreateRecordFieldDraft,
    ctx: &WireContext<'_>,
) -> CreateRecordFieldDraft {
    let annotation = field
        .value
        .as_ref()
        .and_then(|value| annotation_for_draft_field(actual_type, &field.name, value, ctx));
    CreateRecordFieldDraft {
        name: field.name.clone(),
        value: field.value.clone(),
        source: field.source,
        required: field.required.clone(),
        annotation,
    }
}
