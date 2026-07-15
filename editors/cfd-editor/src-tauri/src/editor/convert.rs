//! Build editor-facing `RecordRow` / `FieldCell` views over engine records.
//!
//! After spec 17, `FieldCell.value` is a `CfdValue` straight from the
//! core model — no wire-only re-encoding. Editor-derived metadata
//! (spread-source, ref target file hint, enum integer value) is
//! collected into `FieldAnnotation` on the side.

use coflow_data_model::{CfdPath, CfdRecord, CfdValue};
use coflow_runtime::{
    dict_key_path_text, value_summary, FieldShapeInfo, ProjectQueries, RecordCoordinate, RecordView,
};
use std::collections::{BTreeMap, BTreeSet};

use crate::editor::session::Diagnostics;
use crate::editor::types::{FieldAnnotation, FieldCell, FieldDiagnostic, RecordRow, SpreadInfo};

/// Lookup context the converter consults when annotating cells.
pub struct WireContext<'a> {
    pub queries: ProjectQueries<'a>,
    pub diagnostics: &'a Diagnostics,
}

impl<'a> WireContext<'a> {
    #[must_use]
    pub fn new(queries: ProjectQueries<'a>, diagnostics: &'a Diagnostics) -> Self {
        Self {
            queries,
            diagnostics,
        }
    }
}

/// Translate a [`RecordView`] into a wire [`RecordRow`].
#[must_use]
pub fn record_view_to_row(view: &RecordView<'_>, ctx: &WireContext<'_>) -> RecordRow {
    let fields = record_fields(view.record, ctx);
    let (field_index, field_summaries) = field_indexes(&fields);
    let (field_diagnostics, diagnostic_severity) =
        diagnostics_for_record(ctx.diagnostics, view.display_path, &view.coordinate);
    RecordRow {
        coordinate: view.coordinate.clone(),
        display_path: view.display_path.to_string(),
        fields,
        field_index,
        field_summaries,
        field_diagnostics,
        diagnostic_severity,
    }
}

/// Convenience: pull the [`RecordView`] from the session, then render it.
#[must_use]
pub fn record_to_row(record: &CfdRecord, display_path: &str, ctx: &WireContext<'_>) -> RecordRow {
    let fields = record_fields(record, ctx);
    let (field_index, field_summaries) = field_indexes(&fields);
    let coordinate = RecordCoordinate::new(record.actual_type(), record.key.clone());
    let (field_diagnostics, diagnostic_severity) =
        diagnostics_for_record(ctx.diagnostics, display_path, &coordinate);
    RecordRow {
        coordinate,
        display_path: display_path.to_string(),
        fields,
        field_index,
        field_summaries,
        field_diagnostics,
        diagnostic_severity,
    }
}

fn diagnostics_for_record(
    diagnostics: &Diagnostics,
    file_path: &str,
    coordinate: &RecordCoordinate,
) -> (Vec<FieldDiagnostic>, Option<String>) {
    let mut fields = Vec::new();
    let mut best = None;
    for diagnostic in diagnostics.for_record(file_path, coordinate) {
        if let Some(field_path) = &diagnostic.field_path {
            fields.push(FieldDiagnostic {
                severity: normalized_severity(&diagnostic.severity).to_string(),
                field_path: field_path.clone(),
                message: diagnostic.message.clone(),
            });
        }
        match diagnostic.severity.as_str() {
            "error" => best = Some("error"),
            "warning" if best.is_none() => best = Some("warning"),
            _ => {}
        }
    }
    (fields, best.map(str::to_string))
}

fn normalized_severity(severity: &str) -> &'static str {
    match severity {
        "error" => "error",
        "warning" => "warning",
        _ => "info",
    }
}

fn record_fields(record: &CfdRecord, ctx: &WireContext<'_>) -> Vec<FieldCell> {
    // `CfdRecord` stores fields in a BTreeMap for deterministic lookup, not
    // presentation. The schema retains the declared (including inherited)
    // field order, which is what users expect in the editor.
    let declared_names = ctx
        .queries
        .schema_type_fields(record.actual_type())
        .into_iter()
        .map(|(name, _)| name)
        .collect::<Vec<_>>();
    let declared_name_set = declared_names.iter().cloned().collect::<BTreeSet<_>>();
    let remaining_names = record
        .fields()
        .keys()
        .filter(|name| !declared_name_set.contains(*name))
        .cloned();

    declared_names
        .into_iter()
        .chain(remaining_names)
        .filter_map(|name| {
            let value = record.fields().get(&name)?;
            Some(FieldCell {
                name: name.clone(),
                value: value.clone(),
                annotation: build_annotation(record, &name, value, ctx, &[]),
            })
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

fn build_annotation(
    host: &CfdRecord,
    field_name: &str,
    value: &CfdValue,
    ctx: &WireContext<'_>,
    parent_path: &[String],
) -> Option<FieldAnnotation> {
    let host_coordinate = RecordCoordinate::new(host.actual_type(), host.key.clone());
    let path = CfdPath::root().field(field_name.to_string());
    let declared_shape = ctx.queries.field_shape(host.actual_type(), field_name);
    let mut annotation = annotation_for_value(
        value,
        ctx,
        Some(&host_coordinate),
        &path,
        declared_shape.as_ref(),
    );
    if let Some(source) = ctx.queries.spread_source(&host_coordinate, &path) {
        annotation.spread_info = Some(spread_info_for_source(
            ctx,
            &source,
            parent_path,
            field_name,
        ));
    }
    // Synthesized dimension records expose a `default` slot that mirrors the
    // source record's value. Writing into it isn't blocked at the engine
    // layer, but the editor renders it as read-only to steer users to the
    // source record instead.
    if annotation.is_empty() {
        None
    } else {
        Some(annotation)
    }
}

#[must_use]
pub fn annotation_for_draft_field(
    actual_type: &str,
    field_name: &str,
    value: &CfdValue,
    ctx: &WireContext<'_>,
) -> Option<FieldAnnotation> {
    let path = CfdPath::root().field(field_name.to_string());
    let declared_shape = ctx.queries.field_shape(actual_type, field_name);
    let annotation = annotation_for_value(value, ctx, None, &path, declared_shape.as_ref());
    if annotation.is_empty() {
        None
    } else {
        Some(annotation)
    }
}

fn annotation_for_value(
    value: &CfdValue,
    ctx: &WireContext<'_>,
    host: Option<&RecordCoordinate>,
    path: &CfdPath,
    declared_shape: Option<&FieldShapeInfo>,
) -> FieldAnnotation {
    let mut annotation = FieldAnnotation::default();
    if let Some(shape) = declared_shape {
        annotation.declared_type = Some(shape.display_label.clone());
        annotation
            .ref_target_type
            .clone_from(&shape.ref_target_type);
        annotation.enum_type.clone_from(&shape.enum_type);
        annotation.nullable = shape.nullable;
        annotation
            .polymorphic_types
            .clone_from(&shape.polymorphic_types);
        // Preload the element template when the declared type is a
        // collection. Filled here (not only in the Array/Dict arms below) so
        // a nullable / empty collection still carries the template the
        // editor needs to add its first element.
        if let Some(item_shape) = shape.collection_item.as_deref() {
            annotation.item_annotation = Some(Box::new(element_template(item_shape)));
        }
    }
    match value {
        CfdValue::Ref(_) => {
            annotation.ref_target_file = host
                .and_then(|host| ctx.queries.resolved_ref_target(host, path))
                .and_then(|target| {
                    ctx.queries
                        .file_for_record(&target.actual_type, &target.key)
                        .map(str::to_string)
                });
        }
        CfdValue::Enum(enum_value) => {
            annotation.enum_int_value = Some(enum_value.value);
        }
        CfdValue::Object(object) => {
            for (name, child) in object.fields() {
                let child_shape = ctx.queries.field_shape(object.actual_type(), name);
                let child_path = path.clone().field(name.clone());
                let child_annotation =
                    annotation_for_value(child, ctx, host, &child_path, child_shape.as_ref());
                if !child_annotation.is_empty() {
                    annotation.children.insert(name.clone(), child_annotation);
                }
            }
        }
        CfdValue::Array(items) => {
            let item_shape = declared_shape.and_then(|shape| shape.collection_item.as_deref());
            for (idx, child) in items.iter().enumerate() {
                let child_path = path.clone().index(idx);
                let child_annotation =
                    annotation_for_value(child, ctx, host, &child_path, item_shape);
                if !child_annotation.is_empty() {
                    annotation
                        .children
                        .insert(idx.to_string(), child_annotation);
                }
            }
        }
        CfdValue::Dict(entries) => {
            let item_shape = declared_shape.and_then(|shape| shape.collection_item.as_deref());
            for (key, child) in entries {
                let key_text = dict_key_path_text(key);
                let child_path = path.clone().dict_key_value(key);
                let child_annotation =
                    annotation_for_value(child, ctx, host, &child_path, item_shape);
                if !child_annotation.is_empty() {
                    annotation.children.insert(key_text, child_annotation);
                }
            }
        }
        _ => {}
    }
    annotation
}

/// Produce a minimal template `FieldAnnotation` describing the elements of
/// a collection (array item / dict value). The editor consumes this when it
/// needs the element's declared type / ref target / enum type to add a new
/// entry into an empty or nullable collection.
fn element_template(item_shape: &FieldShapeInfo) -> FieldAnnotation {
    let mut ann = FieldAnnotation {
        declared_type: Some(item_shape.display_label.clone()),
        ref_target_type: item_shape.ref_target_type.clone(),
        enum_type: item_shape.enum_type.clone(),
        nullable: item_shape.nullable,
        polymorphic_types: item_shape.polymorphic_types.clone(),
        ..FieldAnnotation::default()
    };
    if let Some(inner) = item_shape.collection_item.as_deref() {
        ann.item_annotation = Some(Box::new(element_template(inner)));
    }
    ann
}

fn spread_info_for_source(
    ctx: &WireContext<'_>,
    source: &RecordCoordinate,
    parent_path: &[String],
    field_name: &str,
) -> SpreadInfo {
    let mut source_field_path = parent_path.to_vec();
    source_field_path.push(field_name.to_string());
    let source_record_file = ctx
        .queries
        .file_for_record(&source.actual_type, &source.key)
        .map(str::to_string);
    SpreadInfo {
        source: source.clone(),
        source_record_file,
        source_field_path,
    }
}

#[cfg(test)]
mod tests {
    use coflow_data_model::CfdValue;
    use coflow_runtime::value_summary;

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

        assert_eq!(value_summary(&CfdValue::String(value)), expected);
    }
}
