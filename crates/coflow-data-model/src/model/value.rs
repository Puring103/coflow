use super::ids::RecordCoordinate;
use crate::diagnostics::RecordOrigin;
use crate::diagnostics::{format_cfd_dict_key, CfdPath, CfdPathSegment};
use coflow_cft::{
    CftEnumValue, CftNameError, DimensionName, EnumName, EnumVariantName, FieldName, RecordKey,
    TypeName, VariantName,
};
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
    #[cfg_attr(feature = "ts-export", ts(type = "string"))]
    pub key: RecordKey,
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
    #[serde(skip)]
    #[cfg_attr(feature = "ts-export", ts(skip))]
    pub dimension_fields: BTreeMap<FieldName, CfdDimensionFieldValues>,
}

impl CfdRecord {
    #[must_use]
    pub fn key(&self) -> &str {
        self.key.as_str()
    }

    #[must_use]
    pub const fn record_key(&self) -> &RecordKey {
        &self.key
    }

    #[must_use]
    pub fn actual_type(&self) -> &str {
        self.object.actual_type.as_str()
    }

    #[must_use]
    pub const fn actual_type_name(&self) -> &TypeName {
        &self.object.actual_type
    }

    #[must_use]
    pub fn coordinate(&self) -> RecordCoordinate {
        RecordCoordinate::new(self.actual_type_name().clone(), self.record_key().clone())
    }

    #[must_use]
    pub fn fields(&self) -> &BTreeMap<FieldName, CfdValue> {
        &self.object.fields
    }

    #[must_use]
    pub fn field(&self, name: &str) -> Option<&CfdValue> {
        self.object.field(name)
    }

    #[must_use]
    pub fn dimension_field(&self, name: &str) -> Option<&CfdDimensionFieldValues> {
        self.dimension_fields.get(name)
    }

    /// Resolves a value by its absolute path within this record.
    #[must_use]
    pub fn value_at_path(&self, path: &CfdPath) -> Option<&CfdValue> {
        let mut segments = path.segments.iter();
        let CfdPathSegment::Field(field) = segments.next()? else {
            return None;
        };
        let mut current = self.fields().get(field.as_str())?;
        for segment in segments {
            current = match (segment, current) {
                (CfdPathSegment::Field(field), CfdValue::Object(record)) => {
                    record.fields().get(field.as_str())?
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
#[derive(Debug, Clone, PartialEq)]
pub struct CfdDimensionFieldValues {
    pub dimension: DimensionName,
    pub variants: BTreeMap<VariantName, CfdDimensionValue>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CfdDimensionValue {
    pub value: CfdValue,
    pub origin: RecordOrigin,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct CfdObject {
    #[cfg_attr(feature = "ts-export", ts(type = "string"))]
    pub actual_type: TypeName,
    #[cfg_attr(feature = "ts-export", ts(type = "Record<string, CfdValue>"))]
    pub fields: BTreeMap<FieldName, CfdValue>,
}

impl CfdObject {
    #[must_use]
    pub const fn new(actual_type: TypeName, fields: BTreeMap<FieldName, CfdValue>) -> Self {
        Self {
            actual_type,
            fields,
        }
    }

    /// Validates raw object and field names before constructing a successful
    /// model object.
    ///
    /// # Errors
    ///
    /// Returns an error when the type name or any field name is invalid.
    pub fn try_new(
        actual_type: impl Into<String>,
        fields: BTreeMap<String, CfdValue>,
    ) -> Result<Self, CftNameError> {
        let fields = fields
            .into_iter()
            .map(|(name, value)| Ok((FieldName::new(name)?, value)))
            .collect::<Result<_, CftNameError>>()?;
        Ok(Self::new(TypeName::new(actual_type)?, fields))
    }

    #[must_use]
    pub fn actual_type(&self) -> &str {
        self.actual_type.as_str()
    }

    #[must_use]
    pub const fn actual_type_name(&self) -> &TypeName {
        &self.actual_type
    }

    #[must_use]
    pub fn fields(&self) -> &BTreeMap<FieldName, CfdValue> {
        &self.fields
    }

    #[must_use]
    pub fn fields_mut(&mut self) -> &mut BTreeMap<FieldName, CfdValue> {
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
    Ref(#[cfg_attr(feature = "ts-export", ts(type = "string"))] RecordKey),
    Array(Vec<CfdValue>),
    Dict(Vec<(CfdDictKey, CfdValue)>),
}

impl CfdValue {
    /// Validates a raw reference key before constructing a successful value.
    ///
    /// # Errors
    ///
    /// Returns an error when `key` is not a valid record key.
    pub fn record_ref(key: impl Into<String>) -> Result<Self, CftNameError> {
        RecordKey::new(key).map(Self::Ref)
    }
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
    #[cfg_attr(feature = "ts-export", ts(type = "string"))]
    pub enum_name: EnumName,
    #[cfg_attr(feature = "ts-export", ts(type = "string | null"))]
    pub variant: Option<EnumVariantName>,
    #[serde(with = "crate::serde_i64")]
    #[cfg_attr(feature = "ts-export", ts(type = "bigint"))]
    pub value: i64,
}

impl CfdEnumValue {
    /// Validates raw enum identity before constructing a successful value.
    ///
    /// # Errors
    ///
    /// Returns an error when the enum or optional variant name is invalid.
    pub fn try_new(
        enum_name: impl Into<String>,
        variant: Option<impl Into<String>>,
        value: i64,
    ) -> Result<Self, CftNameError> {
        Ok(Self {
            enum_name: EnumName::new(enum_name)?,
            variant: variant.map(EnumVariantName::new).transpose()?,
            value,
        })
    }
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
            enum_name: meta.enum_name,
            variant: meta.variant,
            value: meta.value,
        }
    }
}
