//! 会话 mutation 应用层：字段写回、集合编辑、草稿转 wire、mutation 收尾。
//!
//! 引擎拥有校验与落盘，本层只做 `MutationReport` 到编辑器 DTO 的包装。

use coflow_runtime::CfdValue;

use super::errors::{api_diagnostics_to_editor_error, mutation_report_to_editor_error};
use super::{Diagnostics, EditorSession};
use crate::editor::convert::{annotation_for_draft_field, record_view_to_row, WireContext};
use crate::editor::types::{
    CollectionEdit, CreateRecordDraft, CreateRecordFieldDraft, EditorError, WriteFieldOutcome,
};

/// 字段写回：先取 `old_value` 供 undo 使用，再走引擎 `SetField`。
pub(crate) fn write_field_in_session(
    session: &mut EditorSession,
    coordinate: &coflow_runtime::RecordCoordinate,
    field_path: &[coflow_runtime::CfdPathSegment],
    new_value: &CfdValue,
) -> Result<WriteFieldOutcome, EditorError> {
    let old_value = session
        .queries()
        .effective_field_write(coordinate, field_path)
        .and_then(|preview| preview.old_value);
    let report = coflow_runtime::commands::apply_project_mutation(
        &mut session.engine,
        coflow_runtime::MutationRequest {
            stop_on_write_error: true,
            ops: vec![coflow_runtime::MutationOp::SetField {
                record: coordinate.clone(),
                file: None,
                path: field_path.to_vec(),
                value: coflow_runtime::MutationValue::Cfd(new_value.clone()),
            }],
        },
    )
    .map_err(api_diagnostics_to_editor_error)?;
    let report = finalize_mutation(session, report, "write field failed")?;
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
    let view = queries
        .record_view(&final_coordinate.actual_type, &final_coordinate.key)
        .ok_or_else(|| {
            EditorError::not_found(format!(
                "record `{}.{}` not found after write",
                final_coordinate.actual_type, final_coordinate.key
            ))
        })?;
    let current_value = queries
        .field_value(
            &final_coordinate.actual_type,
            &final_coordinate.key,
            field_path,
        )
        .cloned();
    let ctx = WireContext::new(queries, &session.diagnostics, &session.shape_cache);
    Ok(WriteFieldOutcome {
        revision: session.revisions.current(),
        row: record_view_to_row(&view, &ctx),
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
    report: coflow_runtime::MutationReport,
    fallback: &str,
) -> Result<coflow_runtime::MutationReport, EditorError> {
    if !report.write_ok {
        return Err(mutation_report_to_editor_error(fallback, &report));
    }
    session.diagnostics = Diagnostics::from_queries(session.queries(), &session.project_root);
    if report.generation_changed {
        session.commit_internal_write(&report.written_files);
        session.ref_target_cache.clear();
    }
    Ok(report)
}

/// 集合字段的纯函数编辑：`Option` 包装自动展开/回包，数组/字典分支各自处理。
pub(crate) fn apply_collection_edit(
    value: CfdValue,
    edit: CollectionEdit,
    default_item: Option<CfdValue>,
) -> Result<CfdValue, EditorError> {
    match (value, edit) {
        (CfdValue::OptionSome(inner), edit) => apply_collection_edit(*inner, edit, default_item)
            .map(|value| CfdValue::OptionSome(Box::new(value))),
        (CfdValue::OptionNone, edit @ CollectionEdit::ArrayAppend { .. }) => {
            apply_collection_edit(CfdValue::Array(Vec::new()), edit, default_item)
                .map(|value| CfdValue::OptionSome(Box::new(value)))
        }
        (CfdValue::OptionNone, edit @ CollectionEdit::DictInsert { .. }) => {
            apply_collection_edit(CfdValue::Dict(Vec::new()), edit, default_item)
                .map(|value| CfdValue::OptionSome(Box::new(value)))
        }
        (CfdValue::Array(mut items), CollectionEdit::ArrayAppend { value }) => {
            let seed = value
                .or(default_item)
                .ok_or_else(|| EditorError::write("array item requires an explicit value"))?;
            items.push(seed);
            Ok(CfdValue::Array(items))
        }
        (CfdValue::Array(mut items), CollectionEdit::ArrayRemove { index }) => {
            if index >= items.len() {
                return Err(EditorError::write("array index out of range"));
            }
            items.remove(index);
            Ok(CfdValue::Array(items))
        }
        (CfdValue::Array(mut items), CollectionEdit::ArrayMove { from, to }) => {
            if from >= items.len() || to >= items.len() {
                return Err(EditorError::write("array index out of range"));
            }
            if from != to {
                let moved = items.remove(from);
                items.insert(to, moved);
            }
            Ok(CfdValue::Array(items))
        }
        (CfdValue::Dict(mut entries), CollectionEdit::DictInsert { key, value }) => {
            if entries.iter().any(|(entry_key, _)| entry_key == &key) {
                return Err(EditorError::write("dict key already exists"));
            }
            let seed = value
                .or(default_item)
                .ok_or_else(|| EditorError::write("dict value requires an explicit value"))?;
            entries.push((key, seed));
            Ok(CfdValue::Dict(entries))
        }
        (CfdValue::Dict(entries), CollectionEdit::DictRemove { key }) => {
            let original_len = entries.len();
            let entries = entries
                .into_iter()
                .filter(|(entry_key, _)| entry_key != &key)
                .collect::<Vec<_>>();
            if entries.len() == original_len {
                return Err(EditorError::write("dict key not found"));
            }
            Ok(CfdValue::Dict(entries))
        }
        _ => Err(EditorError::write(
            "collection edit target is not a collection",
        )),
    }
}

pub(crate) fn create_record_draft_to_wire(
    draft: &coflow_runtime::CreateRecordDraft,
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
    field: &coflow_runtime::CreateRecordFieldDraft,
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

#[cfg(test)]
mod collection_edit_tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn array_append_materializes_an_optional_collection() {
        let next = apply_collection_edit(
            CfdValue::OptionNone,
            CollectionEdit::ArrayAppend {
                value: Some(CfdValue::Int(1)),
            },
            None,
        )
        .expect("optional array edit");

        assert_eq!(
            next,
            CfdValue::OptionSome(Box::new(CfdValue::Array(vec![CfdValue::Int(1)])))
        );
    }

    #[test]
    fn dict_insert_preserves_nested_option_layers() {
        let next = apply_collection_edit(
            CfdValue::OptionSome(Box::new(CfdValue::OptionNone)),
            CollectionEdit::DictInsert {
                key: coflow_runtime::CfdDictKey::String("key".to_string()),
                value: Some(CfdValue::Bool(true)),
            },
            None,
        )
        .expect("nested optional dict edit");

        assert_eq!(
            next,
            CfdValue::OptionSome(Box::new(CfdValue::OptionSome(Box::new(CfdValue::Dict(
                vec![(
                    coflow_runtime::CfdDictKey::String("key".to_string()),
                    CfdValue::Bool(true),
                )]
            ),))))
        );
    }
}
