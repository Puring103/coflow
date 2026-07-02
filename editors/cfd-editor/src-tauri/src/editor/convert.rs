//! Build editor-facing `RecordRow` / `FieldCell` views over engine records.
//!
//! After spec 17, `FieldCell.value` is a `CfdValue` straight from the
//! core model — no wire-only re-encoding. Editor-derived metadata
//! (spread-source, ref target file hint, enum integer value) is
//! collected into `FieldAnnotation` on the side. Conversion is a single
//! walk of the record so the annotation tree mirrors the value tree.

use coflow_data_model::{CfdPath, CfdRecord, CfdRecordId, CfdValue, RefSite};
use coflow_engine::{ProjectSession, RecordCoordinate, RecordView};
use std::collections::BTreeMap;

use crate::editor::types::{FieldAnnotation, FieldCell, RecordRow, SpreadInfo};

const STRING_SUMMARY_TRUNCATE_AFTER_BYTES: usize = 40;
const STRING_SUMMARY_PREFIX_BYTES: usize = 38;

/// Lookup context the converter consults when annotating cells.
pub struct WireContext<'a> {
    pub session: &'a ProjectSession,
}

/// Translate a [`RecordView`] into a wire [`RecordRow`].
#[must_use]
pub fn record_view_to_row(view: &RecordView<'_>, ctx: &WireContext<'_>) -> RecordRow {
    let fields = record_fields(view.record, ctx);
    let (field_index, field_summaries) = field_indexes(&fields);
    RecordRow {
        coordinate: view.coordinate.clone(),
        display_path: view.display_path.to_string(),
        fields,
        field_index,
        field_summaries,
    }
}

/// Convenience: pull the [`RecordView`] from the session, then render it.
#[must_use]
pub fn record_to_row(record: &CfdRecord, display_path: &str, ctx: &WireContext<'_>) -> RecordRow {
    let fields = record_fields(record, ctx);
    let (field_index, field_summaries) = field_indexes(&fields);
    RecordRow {
        coordinate: RecordCoordinate::new(record.actual_type(), record.key.clone()),
        display_path: display_path.to_string(),
        fields,
        field_index,
        field_summaries,
    }
}

fn record_fields(record: &CfdRecord, ctx: &WireContext<'_>) -> Vec<FieldCell> {
    record
        .fields()
        .iter()
        .map(|(name, value)| FieldCell {
            name: name.clone(),
            value: value.clone(),
            annotation: build_annotation(record, name, value, ctx, &[]),
        })
        .collect()
}

fn field_indexes(fields: &[FieldCell]) -> (BTreeMap<String, usize>, BTreeMap<String, String>) {
    let mut index = BTreeMap::new();
    let mut summaries = BTreeMap::new();
    for (idx, field) in fields.iter().enumerate() {
        index.insert(field.name.clone(), idx);
        summaries.insert(field.name.clone(), value_summary(&field.value));
    }
    (index, summaries)
}

fn value_summary(value: &CfdValue) -> String {
    match value {
        CfdValue::Null => "-".to_string(),
        CfdValue::Bool(value) => value.to_string(),
        CfdValue::Int(value) => value.to_string(),
        CfdValue::Float(value) => value.to_string(),
        CfdValue::String(value) => string_summary(value),
        CfdValue::Enum(value) => value
            .variant
            .clone()
            .unwrap_or_else(|| value.value.to_string()),
        CfdValue::Ref(target_key) => target_key.clone(),
        CfdValue::Object(value) => value.actual_type().to_string(),
        CfdValue::Array(items) => {
            if items.is_empty() {
                "[]".to_string()
            } else {
                format!("{}[{}]", value_kind(&items[0]), items.len())
            }
        }
        CfdValue::Dict(entries) => {
            if entries.is_empty() {
                "{}".to_string()
            } else {
                format!(
                    "{}->{}  ({})",
                    dict_key_kind(&entries[0].0),
                    value_kind(&entries[0].1),
                    entries.len()
                )
            }
        }
    }
}

fn string_summary(value: &str) -> String {
    if value.len() <= STRING_SUMMARY_TRUNCATE_AFTER_BYTES {
        return value.to_string();
    }
    let end = previous_char_boundary(value, STRING_SUMMARY_PREFIX_BYTES);
    format!("{}...", &value[..end])
}

fn previous_char_boundary(value: &str, preferred_end: usize) -> usize {
    let mut end = preferred_end.min(value.len());
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    end
}

const fn value_kind(value: &CfdValue) -> &'static str {
    match value {
        CfdValue::Null => "null",
        CfdValue::Bool(_) => "bool",
        CfdValue::Int(_) => "int",
        CfdValue::Float(_) => "float",
        CfdValue::String(_) => "string",
        CfdValue::Enum(_) => "enum",
        CfdValue::Ref(_) => "&",
        CfdValue::Object(_) => "object",
        CfdValue::Array(_) => "[]",
        CfdValue::Dict(_) => "{}",
    }
}

const fn dict_key_kind(key: &coflow_data_model::CfdDictKey) -> &'static str {
    match key {
        coflow_data_model::CfdDictKey::String(_) => "string",
        coflow_data_model::CfdDictKey::Int(_) => "int",
        coflow_data_model::CfdDictKey::Enum(_) => "enum",
    }
}

fn build_annotation(
    host: &CfdRecord,
    field_name: &str,
    value: &CfdValue,
    ctx: &WireContext<'_>,
    parent_path: &[String],
) -> Option<FieldAnnotation> {
    let mut annotation = FieldAnnotation::default();
    let host_id = ctx
        .session
        .records
        .id_for_coordinate(host.actual_type(), &host.key);
    let path = CfdPath::root().field(field_name.to_string());
    if let Some(source_id) =
        host_id.and_then(|host| ctx.session.model.spread_source_at_path(host, &path))
    {
        annotation.spread_info = spread_info_for_source(ctx, source_id, parent_path, field_name);
    }
    annotation_for_value(value, ctx, host_id, &path, &mut annotation);
    if annotation.is_empty() {
        None
    } else {
        Some(annotation)
    }
}

fn annotation_for_value(
    value: &CfdValue,
    ctx: &WireContext<'_>,
    host_id: Option<CfdRecordId>,
    path: &CfdPath,
    annotation: &mut FieldAnnotation,
) {
    match value {
        CfdValue::Ref(_) => {
            annotation.ref_target_file = host_id
                .and_then(|host| {
                    ctx.session
                        .model
                        .resolve_ref_effective(&RefSite::new(host, path.clone()))
                })
                .and_then(|target| ctx.session.model.record(target))
                .and_then(|record| {
                    ctx.session
                        .file_for_record(record.actual_type(), &record.key)
                        .map(str::to_string)
                });
        }
        CfdValue::Enum(enum_value) => {
            annotation.enum_int_value = Some(enum_value.value);
        }
        _ => {}
    }
}

fn spread_info_for_source(
    ctx: &WireContext<'_>,
    source_id: CfdRecordId,
    parent_path: &[String],
    field_name: &str,
) -> Option<SpreadInfo> {
    let source = ctx.session.model.record(source_id)?;
    let mut source_field_path = parent_path.to_vec();
    source_field_path.push(field_name.to_string());
    let source_record_file = ctx
        .session
        .file_for_record(source.actual_type(), &source.key)
        .map(str::to_string);
    Some(SpreadInfo {
        source: RecordCoordinate::new(source.actual_type(), source.key.clone()),
        source_record_file,
        source_field_path,
    })
}

#[cfg(test)]
mod tests {
    use super::{string_summary, value_summary};
    use coflow_data_model::CfdValue;

    #[test]
    fn string_summary_preserves_ascii_truncation_behavior() {
        let value = "abcdefghijklmnopqrstuvwxyz0123456789ABCDE";

        assert_eq!(
            value_summary(&CfdValue::String(value.to_string())),
            "abcdefghijklmnopqrstuvwxyz0123456789AB..."
        );
    }

    #[test]
    fn string_summary_truncates_at_utf8_boundary() {
        let value = "婆".repeat(20);
        let expected = format!("{}...", "婆".repeat(12));

        assert_eq!(string_summary(&value), expected);
    }
}
