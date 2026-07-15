use super::ids::CfdRecordId;
use crate::diagnostic::{format_cfd_dict_key, CfdPath, CfdPathSegment};
use crate::origin::RecordOrigin;
use coflow_cft::CftEnumValue;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct CfdRecord {
    pub key: String,
    pub object: CfdObject,
    /// Where this record came from in its original source. Used by writers to
    /// dispatch edits back to the right source and by diagnostics to map
    /// record-anchored labels to file/cell locations. Defaults to
    /// [`RecordOrigin::None`] for synthetic records.
    ///
    /// Not exported to wire - origin metadata is internal to the engine and
    /// not consumed by editor frontends (which route by stable coordinate).
    #[serde(skip)]
    #[cfg_attr(feature = "ts-export", ts(skip))]
    pub origin: RecordOrigin,
}

impl CfdRecord {
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    #[must_use]
    pub fn actual_type(&self) -> &str {
        &self.object.actual_type
    }

    #[must_use]
    pub fn fields(&self) -> &BTreeMap<String, CfdValue> {
        &self.object.fields
    }

    #[must_use]
    pub fn fields_mut(&mut self) -> &mut BTreeMap<String, CfdValue> {
        &mut self.object.fields
    }

    #[must_use]
    pub fn field(&self, name: &str) -> Option<&CfdValue> {
        self.object.field(name)
    }

    /// Resolves a value by its absolute path within this record.
    #[must_use]
    pub fn value_at_path(&self, path: &CfdPath) -> Option<&CfdValue> {
        let mut segments = path.segments.iter();
        let CfdPathSegment::Field(field) = segments.next()? else {
            return None;
        };
        let mut current = self.fields().get(field)?;
        for segment in segments {
            current = match (segment, current) {
                (CfdPathSegment::Field(field), CfdValue::Object(record)) => {
                    record.fields().get(field)?
                }
                (CfdPathSegment::Index(index), CfdValue::Array(items)) => items.get(*index)?,
                (CfdPathSegment::DictKey(key), CfdValue::Dict(entries)) => entries
                    .iter()
                    .find(|(entry_key, _)| format_cfd_dict_key(entry_key) == *key)
                    .map(|(_, value)| value)?,
                _ => return None,
            };
        }
        Some(current)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct CfdObject {
    pub actual_type: String,
    pub fields: BTreeMap<String, CfdValue>,
}

impl CfdObject {
    #[must_use]
    pub fn new(actual_type: impl Into<String>, fields: BTreeMap<String, CfdValue>) -> Self {
        Self {
            actual_type: actual_type.into(),
            fields,
        }
    }

    #[must_use]
    pub fn actual_type(&self) -> &str {
        &self.actual_type
    }

    #[must_use]
    pub fn fields(&self) -> &BTreeMap<String, CfdValue> {
        &self.fields
    }

    #[must_use]
    pub fn fields_mut(&mut self) -> &mut BTreeMap<String, CfdValue> {
        &mut self.fields
    }

    #[must_use]
    pub fn field(&self, name: &str) -> Option<&CfdValue> {
        self.fields.get(name)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum CfdValue {
    Null,
    Bool(bool),
    Int(
        #[serde(with = "crate::serde_i64")]
        #[cfg_attr(feature = "ts-export", ts(type = "bigint"))]
        i64,
    ),
    Float(f64),
    String(String),
    Enum(CfdEnumValue),
    Object(Box<CfdObject>),
    Ref(String),
    Array(Vec<CfdValue>),
    Dict(Vec<(CfdDictKey, CfdValue)>),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum CfdDictKey {
    String(String),
    Int(
        #[serde(with = "crate::serde_i64")]
        #[cfg_attr(feature = "ts-export", ts(type = "bigint"))]
        i64,
    ),
    Enum(CfdEnumValue),
}

/// A resolved enum value.
///
/// `variant` holds the variant identifier when the value matches a defined
/// variant. For `@flag` enums, runtime bitwise operations (`flags | other`,
/// `~flags`) can produce composite integer values that don't correspond to a
/// single declared variant; in that case `variant` is `None` and the value is
/// identified by `enum_name + value` only. Codegen and JSON serialization
/// should therefore prefer `value` (always meaningful) and treat `variant` as
/// a presentation hint that may be missing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct CfdEnumValue {
    pub enum_name: String,
    pub variant: Option<String>,
    #[serde(with = "crate::serde_i64")]
    #[cfg_attr(feature = "ts-export", ts(type = "bigint"))]
    pub value: i64,
}

impl PartialEq for CfdEnumValue {
    fn eq(&self, other: &Self) -> bool {
        self.enum_name == other.enum_name && self.value == other.value
    }
}

impl Eq for CfdEnumValue {}

impl PartialOrd for CfdEnumValue {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CfdEnumValue {
    fn cmp(&self, other: &Self) -> Ordering {
        self.enum_name
            .cmp(&other.enum_name)
            .then_with(|| self.value.cmp(&other.value))
    }
}

impl Hash for CfdEnumValue {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.enum_name.hash(state);
        self.value.hash(state);
    }
}

impl From<CftEnumValue> for CfdEnumValue {
    fn from(meta: CftEnumValue) -> Self {
        Self {
            enum_name: meta.enum_name.to_string(),
            variant: meta.variant.map(|variant| variant.to_string()),
            value: meta.value,
        }
    }
}

#[allow(dead_code)]
fn _record_id_is_part_of_public_value_model(_: CfdRecordId) {}
