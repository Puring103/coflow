use crate::data_model::cell_value::{parse_cell, render_cell_value, ParsedCell};
use crate::data_model::{CfdPathSegment, CfdValue, LoadedValueDraft};

use crate::{write_rules, ProjectSession};

use super::one_value_error;

pub(crate) fn parse_cell_text_value(
    session: &ProjectSession,
    actual_type: &str,
    path: &[CfdPathSegment],
    text: &str,
) -> Result<CfdValue, crate::api::DiagnosticSet> {
    let expected = write_rules::expected_type_for_cfd_path(
        session.schema(),
        actual_type,
        path,
        "MUTATION-PATH",
        "MUTATION",
    )?;
    let parsed =
        parse_cell(session.schema(), &expected.display_label(), text).map_err(|error| {
            one_value_error(
                error
                    .diagnostics
                    .into_iter()
                    .map(|diagnostic| diagnostic.message)
                    .collect::<Vec<_>>()
                    .join("; "),
            )
        })?;
    let ParsedCell::Value(input) = parsed else {
        return Err(one_value_error(
            "empty cell text omits a value; use `None` for an Option field",
        ));
    };
    validate_explicit_references(session, &input)?;
    // 输入阶段只验证模板并保存源码；写入后统一由模型按实际所属对象求值，避免旧记录值污染预览。
    crate::value_drafts::convert(session.schema(), &expected, input).map_err(one_value_error)
}

pub(crate) fn render_cell_text_value(
    value: &CfdValue,
) -> Result<String, crate::api::DiagnosticSet> {
    render_cell_value(value).map_err(|error| one_value_error(error.to_string()))
}

fn validate_explicit_references(
    session: &ProjectSession,
    value: &LoadedValueDraft,
) -> Result<(), crate::api::DiagnosticSet> {
    match value {
        LoadedValueDraft::TypedRecordRef { type_name, id: key } => {
            if session
                .model()
                .lookup_assignable(session.schema(), type_name, key)
                .is_none()
            {
                return Err(one_value_error(format!("explicit reference `&{type_name}::{key}` was not found or has a different actual type")));
            }
        }
        LoadedValueDraft::OptionSome(inner) => validate_explicit_references(session, inner)?,
        LoadedValueDraft::Array(items) => {
            for item in items {
                validate_explicit_references(session, item)?;
            }
        }
        LoadedValueDraft::Dict(entries) => {
            for (_, item) in entries {
                validate_explicit_references(session, item)?;
            }
        }
        LoadedValueDraft::Object { fields, .. } => {
            for item in fields.values() {
                validate_explicit_references(session, item)?;
            }
        }
        _ => {}
    }
    Ok(())
}
