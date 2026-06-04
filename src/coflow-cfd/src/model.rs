use crate::{check::CheckRunner, compiler::ModelCompiler, CfdDiagnostics};
use coflow_cft::CftContainer;
use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct CfdDataModel {
    pub tables: BTreeMap<String, CfdTable>,
    pub inheritance_index: BTreeMap<String, CfdPolymorphicIndex>,
    pub(crate) records: Vec<CfdRecord>,
}

impl CfdDataModel {
    #[must_use]
    pub fn builder(schema: &CftContainer) -> CfdModelBuilder<'_> {
        CfdModelBuilder::new(schema)
    }

    /// Executes CFT `check` blocks against this already-built data model.
    ///
    /// # Errors
    ///
    /// Returns runtime check diagnostics for false conditions or evaluation
    /// errors. This method is source-neutral and never inspects Excel/JSON
    /// loader state.
    pub fn run_checks(&self, schema: &CftContainer) -> Result<(), CfdDiagnostics> {
        CheckRunner::new(schema, self).run()
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

#[derive(Debug, Clone, PartialEq)]
pub struct CfdPolymorphicIndex {
    pub root_type: String,
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CfdEnumValue {
    pub enum_name: String,
    pub variant: String,
    pub value: i64,
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
