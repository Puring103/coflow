//! Build editor-facing `RecordRow` / `FieldCell` views over engine records.
//!
//! After spec 17, `FieldCell.value` is a `CfdValue` straight from the
//! core model — no wire-only re-encoding. Editor-derived metadata
//! (spread-source, ref target file hint, enum integer value) is
//! collected into `FieldAnnotation` on the side.

use coflow_api::FlatDiagnostic;
use coflow_cft::{CftSchemaTypeRef, CftSchemaView};
use coflow_data_model::{CfdPath, CfdRecord, CfdRecordId, CfdValue, RefSite};
use coflow_runtime::{
    dict_key_path_text, value_summary, ProjectSession, RecordCoordinate, RecordView,
};
use std::collections::{BTreeMap, BTreeSet};

use crate::editor::types::{FieldAnnotation, FieldCell, FieldDiagnostic, RecordRow, SpreadInfo};

/// Lookup context the converter consults when annotating cells.
pub struct WireContext<'a> {
    pub session: &'a ProjectSession,
    pub schema: CftSchemaView,
    pub diagnostics: Vec<FlatDiagnostic>,
    /// Set of dimension-synthesized type names (e.g. `Item_nameVariants`).
    /// Passed in once per snapshot so the annotator can flag the derived
    /// `default` slot as read-only without recomputing per record.
    pub dimension_synth_types: BTreeSet<String>,
}

impl<'a> WireContext<'a> {
    /// Build a `WireContext` and eagerly compute the dimension-synthesized
    /// type set. Callers that build many rows in a row should reuse the
    /// same context to avoid re-walking the dimension list.
    #[must_use]
    pub fn new(session: &'a ProjectSession, diagnostics: Vec<FlatDiagnostic>) -> Self {
        Self {
            session,
            schema: CftSchemaView::new(session.schema()),
            diagnostics,
            dimension_synth_types: session.dimension_synthesized_types(),
        }
    }
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
        field_diagnostics: field_diagnostics_for_record(
            &ctx.diagnostics,
            view.display_path,
            &view.coordinate,
        ),
        diagnostic_severity: diagnostic_severity_for_record(
            &ctx.diagnostics,
            view.display_path,
            &view.coordinate,
        ),
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
        field_diagnostics: field_diagnostics_for_record(
            &ctx.diagnostics,
            display_path,
            &RecordCoordinate::new(record.actual_type(), record.key.clone()),
        ),
        diagnostic_severity: diagnostic_severity_for_record(
            &ctx.diagnostics,
            display_path,
            &RecordCoordinate::new(record.actual_type(), record.key.clone()),
        ),
    }
}

fn field_diagnostics_for_record(
    diagnostics: &[FlatDiagnostic],
    file_path: &str,
    coordinate: &RecordCoordinate,
) -> Vec<FieldDiagnostic> {
    diagnostics
        .iter()
        .filter(|diagnostic| diagnostic_matches_record(diagnostic, file_path, coordinate))
        .filter_map(|diagnostic| {
            diagnostic
                .field_path
                .as_ref()
                .map(|field_path| FieldDiagnostic {
                    severity: normalized_severity(&diagnostic.severity).to_string(),
                    field_path: field_path.clone(),
                    message: diagnostic.message.clone(),
                })
        })
        .collect()
}

fn diagnostic_severity_for_record(
    diagnostics: &[FlatDiagnostic],
    file_path: &str,
    coordinate: &RecordCoordinate,
) -> Option<String> {
    let mut best = None;
    for diagnostic in diagnostics {
        if !diagnostic_matches_record(diagnostic, file_path, coordinate) {
            continue;
        }
        match diagnostic.severity.as_str() {
            "error" => return Some("error".to_string()),
            "warning" => best = Some("warning".to_string()),
            _ => {}
        }
    }
    best
}

fn diagnostic_matches_record(
    diagnostic: &FlatDiagnostic,
    file_path: &str,
    coordinate: &RecordCoordinate,
) -> bool {
    diagnostic.file_path.as_deref() == Some(file_path)
        && diagnostic.record_key.as_deref() == Some(coordinate.key.as_str())
        && diagnostic
            .actual_type
            .as_deref()
            .is_none_or(|actual_type| actual_type == coordinate.actual_type)
}

fn normalized_severity(severity: &str) -> &'static str {
    match severity {
        "error" => "error",
        "warning" => "warning",
        _ => "info",
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

fn build_annotation(
    host: &CfdRecord,
    field_name: &str,
    value: &CfdValue,
    ctx: &WireContext<'_>,
    parent_path: &[String],
) -> Option<FieldAnnotation> {
    let host_id = ctx
        .session
        .records()
        .id_for_coordinate(host.actual_type(), &host.key);
    let path = CfdPath::root().field(field_name.to_string());
    let declared_type = declared_field_type(ctx, host.actual_type(), field_name);
    let mut annotation = annotation_for_value(value, ctx, host_id, &path, declared_type);
    if let Some(source_id) =
        host_id.and_then(|host| ctx.session.model().spread_source_at_path(host, &path))
    {
        annotation.spread_info = spread_info_for_source(ctx, source_id, parent_path, field_name);
    }
    // Synthesized dimension records expose a `default` slot that mirrors the
    // source record's value. Writing into it isn't blocked at the engine
    // layer, but the editor renders it as read-only to steer users to the
    // source record instead.
    if field_name == "default" && ctx.dimension_synth_types.contains(host.actual_type()) {
        annotation.read_only = true;
    }
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
    let declared_type = declared_field_type(ctx, actual_type, field_name);
    let annotation = annotation_for_value(value, ctx, None, &path, declared_type);
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
    declared_type: Option<&CftSchemaTypeRef>,
) -> FieldAnnotation {
    let mut annotation = FieldAnnotation::default();
    if let Some(ty) = declared_type {
        annotation.declared_type = Some(ty.display_label());
        annotation.ref_target_type = ref_target_type(ty).map(str::to_string);
        annotation.enum_type = enum_type_name(ty, ctx.session.schema()).map(str::to_string);
        annotation.nullable = matches!(ty, CftSchemaTypeRef::Nullable(_));
        annotation.polymorphic_types = polymorphic_types_for(ty, &ctx.schema);
        // Preload the element template when the declared type is a
        // collection. Filled here (not only in the Array/Dict arms below) so
        // a nullable / empty collection still carries the template the
        // editor needs to add its first element.
        if let Some(item_ty) = array_item_type(Some(ty)).or_else(|| dict_item_type(Some(ty))) {
            annotation.item_annotation = element_template(Some(item_ty), ctx);
        }
    }
    match value {
        CfdValue::Ref(_) => {
            annotation.ref_target_file = host_id
                .and_then(|host| {
                    ctx.session
                        .model()
                        .resolve_effective_ref(&RefSite::new(host, path.clone()))
                })
                .and_then(|target| ctx.session.model().record(target))
                .and_then(|record| {
                    ctx.session
                        .file_for_record(record.actual_type(), &record.key)
                        .map(str::to_string)
                });
        }
        CfdValue::Enum(enum_value) => {
            annotation.enum_int_value = Some(enum_value.value);
        }
        CfdValue::Object(object) => {
            let object_type = object_type_for_value(value, declared_type);
            for (name, child) in object.fields() {
                let child_type =
                    object_type.and_then(|actual_type| declared_field_type(ctx, actual_type, name));
                let child_path = path.clone().field(name.clone());
                let child_annotation =
                    annotation_for_value(child, ctx, host_id, &child_path, child_type);
                if !child_annotation.is_empty() {
                    annotation.children.insert(name.clone(), child_annotation);
                }
            }
        }
        CfdValue::Array(items) => {
            let item_type = array_item_type(declared_type);
            for (idx, child) in items.iter().enumerate() {
                let child_path = path.clone().index(idx);
                let child_annotation =
                    annotation_for_value(child, ctx, host_id, &child_path, item_type);
                if !child_annotation.is_empty() {
                    annotation
                        .children
                        .insert(idx.to_string(), child_annotation);
                }
            }
        }
        CfdValue::Dict(entries) => {
            let item_type = dict_item_type(declared_type);
            for (key, child) in entries {
                let key_text = dict_key_path_text(key);
                let child_path = path.clone().dict_key_value(key);
                let child_annotation =
                    annotation_for_value(child, ctx, host_id, &child_path, item_type);
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
fn element_template(
    item_type: Option<&CftSchemaTypeRef>,
    ctx: &WireContext<'_>,
) -> Option<Box<FieldAnnotation>> {
    let item_type = item_type?;
    let mut ann = FieldAnnotation {
        declared_type: Some(item_type.display_label()),
        ref_target_type: ref_target_type(item_type).map(str::to_string),
        enum_type: enum_type_name(item_type, ctx.session.schema()).map(str::to_string),
        nullable: matches!(item_type, CftSchemaTypeRef::Nullable(_)),
        polymorphic_types: polymorphic_types_for(item_type, &ctx.schema),
        ..FieldAnnotation::default()
    };
    if let Some(inner) =
        array_item_type(Some(item_type)).or_else(|| dict_item_type(Some(item_type)))
    {
        ann.item_annotation = element_template(Some(inner), ctx);
    }
    Some(Box::new(ann))
}

fn declared_field_type<'a>(
    ctx: &'a WireContext<'_>,
    actual_type: &str,
    field_name: &str,
) -> Option<&'a CftSchemaTypeRef> {
    ctx.schema.field_type(actual_type, field_name)
}

fn ref_target_type(ty: &CftSchemaTypeRef) -> Option<&str> {
    match ty {
        CftSchemaTypeRef::Ref(name) => Some(name),
        CftSchemaTypeRef::Nullable(inner) => ref_target_type(inner),
        _ => None,
    }
}

fn enum_type_name<'a>(
    ty: &'a CftSchemaTypeRef,
    schema: &coflow_cft::CftContainer,
) -> Option<&'a str> {
    match ty {
        CftSchemaTypeRef::Named(name) if schema.has_enum(name) => Some(name),
        CftSchemaTypeRef::Nullable(inner) => enum_type_name(inner, schema),
        _ => None,
    }
}

fn object_type_for_value<'a>(
    value: &'a CfdValue,
    declared_type: Option<&'a CftSchemaTypeRef>,
) -> Option<&'a str> {
    if let CfdValue::Object(object) = value {
        return Some(object.actual_type());
    }
    match non_nullable(declared_type?) {
        CftSchemaTypeRef::Named(name) => Some(name),
        _ => None,
    }
}

fn array_item_type(ty: Option<&CftSchemaTypeRef>) -> Option<&CftSchemaTypeRef> {
    match non_nullable(ty?) {
        CftSchemaTypeRef::Array(inner) => Some(inner),
        _ => None,
    }
}

fn dict_item_type(ty: Option<&CftSchemaTypeRef>) -> Option<&CftSchemaTypeRef> {
    match non_nullable(ty?) {
        CftSchemaTypeRef::Dict(_, value) => Some(value),
        _ => None,
    }
}

fn non_nullable(ty: &CftSchemaTypeRef) -> &CftSchemaTypeRef {
    match ty {
        CftSchemaTypeRef::Nullable(inner) => non_nullable(inner),
        _ => ty,
    }
}

/// Concrete types the editor may materialize into a polymorphic field.
///
/// Non-empty only when the declared type resolves to an abstract Named type
/// with at least two concrete descendants — a single-concrete case can't be
/// "switched" so we save the wire bytes and skip. Ref / enum / collection
/// / non-abstract object all return empty.
fn polymorphic_types_for(ty: &CftSchemaTypeRef, schema: &CftSchemaView) -> Vec<String> {
    let CftSchemaTypeRef::Named(name) = non_nullable(ty) else {
        return Vec::new();
    };
    let Some(meta) = schema.type_meta(name) else {
        return Vec::new();
    };
    if !meta.is_abstract {
        return Vec::new();
    }
    let concrete = schema
        .concrete_assignable_types(name)
        .unwrap_or_default();
    if concrete.len() < 2 {
        return Vec::new();
    }
    concrete
}

fn spread_info_for_source(
    ctx: &WireContext<'_>,
    source_id: CfdRecordId,
    parent_path: &[String],
    field_name: &str,
) -> Option<SpreadInfo> {
    let source = ctx.session.model().record(source_id)?;
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
