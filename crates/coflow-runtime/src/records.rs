//! Record views and write outcomes exposed at the engine boundary.

use coflow_api::DiagnosticSet;
use coflow_data_model::RecordOrigin;
use coflow_data_model::{
    format_cfd_dict_key, CfdDictKey, CfdPathSegment, CfdRecord, CfdRecordId, CfdValue,
};
use serde::{Deserialize, Serialize};

use super::{RecordCoordinate, SourceId};

/// Read-only view of a top-level record. Bundles the model's `CfdRecord` with
/// the engine's metadata so hosts don't have to do a second lookup.
#[derive(Debug, Clone)]
pub struct RecordView<'a> {
    pub coordinate: RecordCoordinate,
    pub display_path: &'a str,
    pub record: &'a CfdRecord,
    pub origin: &'a RecordOrigin,
    pub source_id: SourceId,
    pub provider_id: &'a str,
}

/// Outcome of an engine write transaction. Surfaces both the rebuilt
/// diagnostics and the coordinates that changed so callers can refresh local
/// caches without re-querying the full project.
///
/// `renamed` is `Some(old, new)` when the write modified a record's `id`
/// field: the engine treats this as a coordinate change so the editor can
/// update routes, undo stacks, and any other long-lived references that
/// previously pointed at `old`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct WriteOutcome {
    pub touched: Vec<RecordCoordinate>,
    pub inserted: Option<RecordCoordinate>,
    pub deleted: Option<RecordCoordinate>,
    pub renamed: Option<(RecordCoordinate, RecordCoordinate)>,
    // Skip from TS: `DiagnosticSet` references concrete `Diagnostic` types
    // whose location data isn't part of the editor's surface. Hosts that
    // care convert to `FlatDiagnostic` before wire-shipping.
    #[cfg_attr(feature = "ts-export", ts(skip))]
    pub diagnostics: DiagnosticSet,
}

impl WriteOutcome {
    #[must_use]
    pub fn touch(coordinate: RecordCoordinate) -> Self {
        Self {
            touched: vec![coordinate],
            ..Default::default()
        }
    }
}

/// Target descriptor for future host write APIs.
///
/// The editor still resolves writes through its own path. Other hosts can use
/// this descriptor to carry a coordinate and record id together.
#[derive(Debug, Clone)]
pub struct RecordTarget {
    pub id: CfdRecordId,
    pub coordinate: RecordCoordinate,
}

#[derive(Debug, Clone)]
pub struct RefTargetInfo {
    pub coordinate: RecordCoordinate,
    pub file_path: String,
}

#[derive(Debug, Clone)]
pub struct EffectiveFieldWrite {
    pub host: RecordCoordinate,
    pub target: RecordCoordinate,
    pub file_path: String,
    pub field_path: Vec<CfdPathSegment>,
    pub old_value: Option<CfdValue>,
}

#[must_use]
pub fn value_summary(value: &CfdValue) -> String {
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

#[must_use]
pub fn dict_key_path_text(key: &CfdDictKey) -> String {
    format_cfd_dict_key(key)
}

fn string_summary(value: &str) -> String {
    const TRUNCATE_AFTER_BYTES: usize = 40;
    const PREFIX_BYTES: usize = 38;
    if value.len() <= TRUNCATE_AFTER_BYTES {
        return value.to_string();
    }
    let end = previous_char_boundary(value, PREFIX_BYTES);
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

const fn dict_key_kind(key: &CfdDictKey) -> &'static str {
    match key {
        CfdDictKey::String(_) => "string",
        CfdDictKey::Int(_) => "int",
        CfdDictKey::Enum(_) => "enum",
    }
}
