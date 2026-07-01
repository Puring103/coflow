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

use crate::editor::types::{
    FieldAnnotation, FieldCell, FieldMode, FieldModeIndex, RecordRow, SpreadInfo,
};

/// Lookup context the converter consults when annotating cells.
pub struct WireContext<'a> {
    pub session: &'a ProjectSession,
}

/// Translate a [`RecordView`] into a wire [`RecordRow`].
#[must_use]
pub fn record_view_to_row(view: &RecordView<'_>, ctx: &WireContext<'_>) -> RecordRow {
    let fields = view
        .record
        .fields
        .iter()
        .map(|(name, value)| FieldCell {
            name: name.clone(),
            value: value.clone(),
            annotation: build_annotation(
                view.record,
                name,
                value,
                ctx,
                &[],
                &view.record.actual_type,
            ),
        })
        .collect();
    RecordRow {
        coordinate: view.coordinate.clone(),
        display_path: view.display_path.to_string(),
        fields,
    }
}

/// Convenience: pull the [`RecordView`] from the session, then render it.
#[must_use]
pub fn record_to_row(record: &CfdRecord, display_path: &str, ctx: &WireContext<'_>) -> RecordRow {
    let fields = record
        .fields
        .iter()
        .map(|(name, value)| FieldCell {
            name: name.clone(),
            value: value.clone(),
            annotation: build_annotation(record, name, value, ctx, &[], &record.actual_type),
        })
        .collect();
    RecordRow {
        coordinate: RecordCoordinate::new(record.actual_type.clone(), record.key.clone()),
        display_path: display_path.to_string(),
        fields,
    }
}

fn build_annotation(
    host: &CfdRecord,
    field_name: &str,
    value: &CfdValue,
    ctx: &WireContext<'_>,
    parent_path: &[String],
    owner_type: &str,
) -> Option<FieldAnnotation> {
    let mut annotation = FieldAnnotation::default();
    if let Some(source_id) = host.spread_field_sources.get(field_name).copied() {
        annotation.spread_info = spread_info_for_source(ctx, source_id, parent_path, field_name);
    }
    annotation.field_mode = field_mode_for_schema_field(ctx, owner_type, field_name);
    let host_id = ctx
        .session
        .records
        .id_for_coordinate(&host.actual_type, &host.key);
    let path = CfdPath::root().field(field_name.to_string());
    annotation_for_value(value, ctx, host_id, &path, &mut annotation);
    if annotation.is_empty() {
        None
    } else {
        Some(annotation)
    }
}

fn field_mode_for_schema_field(
    ctx: &WireContext<'_>,
    owner_type: &str,
    field_name: &str,
) -> Option<FieldMode> {
    let field = ctx
        .session
        .schema
        .resolve_type(owner_type)?
        .all_fields
        .iter()
        .find(|field| field.name == field_name)?;
    if field
        .annotations
        .iter()
        .any(|annotation| annotation.name == "ref")
    {
        Some(FieldMode::Ref)
    } else if field
        .annotations
        .iter()
        .any(|annotation| annotation.name == "inline")
    {
        Some(FieldMode::Inline)
    } else {
        None
    }
}

#[must_use]
pub fn field_mode_index(session: &ProjectSession) -> FieldModeIndex {
    session
        .schema
        .all_types()
        .filter_map(|schema_type| {
            let fields = schema_type
                .all_fields
                .iter()
                .filter_map(|field| {
                    field_mode_for_annotations(&field.annotations)
                        .map(|mode| (field.name.clone(), mode))
                })
                .collect::<BTreeMap<_, _>>();
            (!fields.is_empty()).then(|| (schema_type.name.clone(), fields))
        })
        .collect()
}

fn field_mode_for_annotations(annotations: &[coflow_cft::CftAnnotation]) -> Option<FieldMode> {
    if annotations
        .iter()
        .any(|annotation| annotation.name == "ref")
    {
        Some(FieldMode::Ref)
    } else if annotations
        .iter()
        .any(|annotation| annotation.name == "inline")
    {
        Some(FieldMode::Inline)
    } else {
        None
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
                        .resolve_ref(&RefSite::new(host, path.clone()))
                })
                .and_then(|target| ctx.session.model.record(target))
                .and_then(|record| {
                    ctx.session
                        .file_for_record(&record.actual_type, &record.key)
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
        .file_for_record(&source.actual_type, &source.key)
        .map(str::to_string);
    Some(SpreadInfo {
        source: RecordCoordinate::new(source.actual_type.clone(), source.key.clone()),
        source_record_file,
        source_field_path,
    })
}
