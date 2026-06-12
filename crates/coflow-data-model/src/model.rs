use crate::{compiler::ModelCompiler, CfdDiagnostics};
use coflow_cft::CftContainer;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, PartialEq)]
pub struct CfdDataModel {
    pub(crate) tables: BTreeMap<String, CfdTable>,
    pub(crate) inheritance_index: BTreeMap<String, CfdPolymorphicIndex>,
    pub(crate) records: Vec<CfdRecord>,
}

impl CfdDataModel {
    #[must_use]
    pub fn builder(schema: &CftContainer) -> CfdModelBuilder<'_> {
        CfdModelBuilder::new(schema)
    }

    #[must_use]
    pub fn record(&self, id: CfdRecordId) -> Option<&CfdRecord> {
        self.records.get(id.0)
    }

    pub fn records(&self) -> impl Iterator<Item = (CfdRecordId, &CfdRecord)> {
        self.records
            .iter()
            .enumerate()
            .map(|(index, record)| (CfdRecordId::new(index), record))
    }

    #[must_use]
    pub fn table(&self, type_name: &str) -> Option<&CfdTable> {
        self.tables.get(type_name)
    }

    /// Looks up a record by type name and id.
    /// Works for both concrete tables and polymorphic (abstract/inherited) ranges.
    #[must_use]
    pub fn lookup(&self, type_name: &str, id: &CfdIdValue) -> Option<CfdRecordId> {
        if let Some(index) = self.inheritance_index.get(type_name) {
            return index.records.get(id).copied();
        }
        self.tables
            .get(type_name)
            .and_then(|table| table.primary_index.get(id))
            .copied()
    }

    /// Returns the polymorphic index for a type, if one exists.
    #[must_use]
    pub fn polymorphic_index(&self, type_name: &str) -> Option<&CfdPolymorphicIndex> {
        self.inheritance_index.get(type_name)
    }

    pub fn tables(&self) -> impl Iterator<Item = (&str, &CfdTable)> {
        self.tables.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Total number of top-level records in the model.
    #[must_use]
    pub fn record_count(&self) -> usize {
        self.records.len()
    }

    /// Returns true when the model contains no top-level records.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Iterates over the records of a specific concrete type, in insertion order.
    pub fn records_of_type<'a>(
        &'a self,
        type_name: &str,
    ) -> impl Iterator<Item = (CfdRecordId, &'a CfdRecord)> + 'a {
        let ids = self
            .tables
            .get(type_name)
            .map_or(&[] as &[CfdRecordId], |table| table.records.as_slice());
        ids.iter()
            .filter_map(move |id| self.records.get(id.0).map(|record| (*id, record)))
    }
}

#[derive(Debug)]
pub struct CfdModelBuilder<'a> {
    schema: &'a CftContainer,
    records: Vec<CfdInputRecord>,
}

impl<'a> CfdModelBuilder<'a> {
    #[must_use]
    pub fn new(schema: &'a CftContainer) -> Self {
        Self {
            schema,
            records: Vec::new(),
        }
    }

    pub fn add_record(
        &mut self,
        actual_type: impl Into<String>,
        fields: impl IntoIterator<Item = (impl Into<String>, CfdInputValue)>,
    ) -> &mut Self {
        self.records.push(CfdInputRecord::new(actual_type, fields));
        self
    }

    pub fn add_input_record(&mut self, record: CfdInputRecord) -> &mut Self {
        self.records.push(record);
        self
    }

    /// Builds a validated in-memory data model from source-neutral records.
    ///
    /// # Errors
    ///
    /// Returns data-model diagnostics for schema/type mismatches, missing
    /// fields, duplicate ids, duplicate dict keys, or unresolved references.
    pub fn build(self) -> Result<CfdDataModel, CfdDiagnostics> {
        ModelCompiler::new(self.schema, self.records).build()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CfdTable {
    pub type_name: String,
    pub records: Vec<CfdRecordId>,
    pub primary_index: BTreeMap<CfdIdValue, CfdRecordId>,
    pub secondary_indexes: BTreeMap<String, BTreeMap<CfdIndexKey, Vec<CfdRecordId>>>,
}

/// Index of records assignable to a given root type (`abstract type` or any
/// concrete type with subclasses), keyed by `@id` value.
///
/// The owning `CfdDataModel.inheritance_index` map keys identify the root
/// type — readers obtain it from the lookup, so it is not duplicated here.
#[derive(Debug, Clone, PartialEq)]
pub struct CfdPolymorphicIndex {
    pub records: BTreeMap<CfdIdValue, CfdRecordId>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CfdRecord {
    pub actual_type: String,
    pub fields: BTreeMap<String, CfdValue>,
}

impl CfdRecord {
    #[must_use]
    pub fn field(&self, name: &str) -> Option<&CfdValue> {
        self.fields.get(name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CfdRecordId(usize);

impl CfdRecordId {
    #[must_use]
    pub(crate) fn new(index: usize) -> Self {
        Self(index)
    }

    #[must_use]
    pub fn index(self) -> usize {
        self.0
    }
}

impl fmt::Display for CfdRecordId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CfdValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Enum(CfdEnumValue),
    Object(Box<CfdRecord>),
    Ref { id: CfdIdValue, target: CfdRecordId },
    Array(Vec<CfdValue>),
    Dict(Vec<(CfdDictKey, CfdValue)>),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CfdIdValue {
    String(String),
    Int(i64),
    Enum(CfdEnumValue),
}

impl From<&str> for CfdIdValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_string())
    }
}

impl From<String> for CfdIdValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<i64> for CfdIdValue {
    fn from(value: i64) -> Self {
        Self::Int(value)
    }
}

impl From<CfdEnumValue> for CfdIdValue {
    fn from(value: CfdEnumValue) -> Self {
        Self::Enum(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CfdDictKey {
    String(String),
    Int(i64),
    Enum(CfdEnumValue),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CfdIndexKey {
    String(String),
    Int(i64),
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
#[derive(Debug, Clone)]
pub struct CfdEnumValue {
    pub enum_name: String,
    pub variant: Option<String>,
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

#[derive(Debug, Clone, PartialEq)]
pub struct CfdInputRecord {
    pub actual_type: String,
    pub fields: BTreeMap<String, CfdInputValue>,
}

impl CfdInputRecord {
    #[must_use]
    pub fn new(
        actual_type: impl Into<String>,
        fields: impl IntoIterator<Item = (impl Into<String>, CfdInputValue)>,
    ) -> Self {
        Self {
            actual_type: actual_type.into(),
            fields: fields
                .into_iter()
                .map(|(name, value)| (name.into(), value))
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CfdInputValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    EnumVariant {
        enum_name: String,
        variant: String,
    },
    Object {
        actual_type: Option<String>,
        fields: BTreeMap<String, CfdInputValue>,
    },
    Ref(CfdIdValue),
    Array(Vec<CfdInputValue>),
    Dict(Vec<(CfdInputDictKey, CfdInputValue)>),
}

impl CfdInputValue {
    #[must_use]
    pub fn enum_variant(enum_name: impl Into<String>, variant: impl Into<String>) -> Self {
        Self::EnumVariant {
            enum_name: enum_name.into(),
            variant: variant.into(),
        }
    }

    #[must_use]
    pub fn object(
        actual_type: impl Into<String>,
        fields: impl IntoIterator<Item = (impl Into<String>, CfdInputValue)>,
    ) -> Self {
        Self::Object {
            actual_type: Some(actual_type.into()),
            fields: fields
                .into_iter()
                .map(|(name, value)| (name.into(), value))
                .collect(),
        }
    }

    #[must_use]
    pub fn object_with_declared_type(
        fields: impl IntoIterator<Item = (impl Into<String>, CfdInputValue)>,
    ) -> Self {
        Self::Object {
            actual_type: None,
            fields: fields
                .into_iter()
                .map(|(name, value)| (name.into(), value))
                .collect(),
        }
    }

    #[must_use]
    pub fn dict(entries: impl IntoIterator<Item = (CfdInputDictKey, CfdInputValue)>) -> Self {
        Self::Dict(entries.into_iter().collect())
    }
}

impl From<bool> for CfdInputValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<i64> for CfdInputValue {
    fn from(value: i64) -> Self {
        Self::Int(value)
    }
}

impl From<f64> for CfdInputValue {
    fn from(value: f64) -> Self {
        Self::Float(value)
    }
}

impl From<&str> for CfdInputValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_string())
    }
}

impl From<String> for CfdInputValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CfdInputDictKey {
    String(String),
    Int(i64),
    EnumVariant { enum_name: String, variant: String },
}

impl CfdInputDictKey {
    #[must_use]
    pub fn enum_variant(enum_name: impl Into<String>, variant: impl Into<String>) -> Self {
        Self::EnumVariant {
            enum_name: enum_name.into(),
            variant: variant.into(),
        }
    }
}

impl From<&str> for CfdInputDictKey {
    fn from(value: &str) -> Self {
        Self::String(value.to_string())
    }
}

impl From<String> for CfdInputDictKey {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<i64> for CfdInputDictKey {
    fn from(value: i64) -> Self {
        Self::Int(value)
    }
}
