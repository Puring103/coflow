use crate::diagnostic::CfdPath;
use crate::origin::RecordOrigin;
use crate::{compiler::ModelCompiler, CfdDiagnostics};
use coflow_cft::CftContainer;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, PartialEq)]
pub struct CfdDataModel {
    pub(crate) tables: BTreeMap<String, CfdTable>,
    pub(crate) inheritance_index: BTreeMap<String, CfdPolymorphicIndex>,
    pub(crate) domain_index: CfdDomainIndex,
    pub(crate) record_by_type_key: BTreeMap<(CfdTypeId, String), CfdRecordId>,
    pub(crate) record_by_domain_key: BTreeMap<(CfdDomainId, String), CfdRecordId>,
    pub(crate) records: Vec<CfdRecord>,
    pub(crate) ref_edges: Vec<RefEdge>,
    pub(crate) ref_by_site: BTreeMap<RefSite, RefEdgeId>,
    pub(crate) ref_by_host: BTreeMap<CfdRecordId, Vec<RefEdgeId>>,
    pub(crate) ref_by_target: BTreeMap<CfdRecordId, Vec<RefEdgeId>>,
}

/// Logical address of a `CfdValue::Ref` instance inside the model: the host
/// record and the `CfdPath` to the ref.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RefSite {
    pub host: CfdRecordId,
    pub path: CfdPath,
}

impl RefSite {
    #[must_use]
    pub const fn new(host: CfdRecordId, path: CfdPath) -> Self {
        Self { host, path }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RefEdgeId(usize);

impl RefEdgeId {
    #[must_use]
    pub(crate) const fn new(index: usize) -> Self {
        Self(index)
    }

    #[must_use]
    pub fn index(self) -> usize {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefEdge {
    pub id: RefEdgeId,
    pub site: RefSite,
    pub host: CfdRecordId,
    pub path: CfdPath,
    pub expected_type: CfdTypeId,
    pub domain: CfdDomainId,
    pub key: String,
    pub target: CfdRecordId,
    pub target_type: CfdTypeId,
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

    /// Looks up a record by type name and record key.
    /// Works for both concrete tables and polymorphic (abstract/inherited) ranges.
    #[must_use]
    pub fn lookup(&self, type_name: &str, key: &str) -> Option<CfdRecordId> {
        if let Some(domain_id) = self.type_domain_id(type_name) {
            if let Some(record_id) = self.record_by_domain_key(domain_id, key) {
                if self.record(record_id).is_some_and(|record| {
                    self.type_is_assignable_by_name(record.actual_type(), type_name)
                }) {
                    return Some(record_id);
                }
            }
        }
        self.tables
            .get(type_name)
            .and_then(|table| table.primary_index.get(key))
            .copied()
    }

    /// Returns the polymorphic index for a type, if one exists.
    #[must_use]
    pub fn polymorphic_index(&self, type_name: &str) -> Option<&CfdPolymorphicIndex> {
        self.inheritance_index.get(type_name)
    }

    /// Returns the stable runtime id for a schema object type name.
    #[must_use]
    pub fn type_id(&self, type_name: &str) -> Option<CfdTypeId> {
        self.domain_index.type_id(type_name)
    }

    /// Returns the schema object type name for a runtime type id.
    #[must_use]
    pub fn type_name(&self, type_id: CfdTypeId) -> Option<&str> {
        self.domain_index.type_name(type_id)
    }

    /// Returns the inheritance connected-component domain for a type id.
    #[must_use]
    pub fn type_domain(&self, type_id: CfdTypeId) -> Option<CfdDomainId> {
        self.domain_index.type_domain(type_id)
    }

    /// Returns ancestors from nearest parent to root for a schema object type.
    #[must_use]
    pub fn type_ancestors(&self, type_id: CfdTypeId) -> Option<&[CfdTypeId]> {
        self.domain_index.type_ancestors(type_id)
    }

    /// Returns the inheritance connected-component domain for a type name.
    #[must_use]
    pub fn type_domain_id(&self, type_name: &str) -> Option<CfdDomainId> {
        self.domain_index.type_domain_by_name(type_name)
    }

    /// Returns all schema object types in an inheritance connected component.
    #[must_use]
    pub fn domain_members(&self, domain_id: CfdDomainId) -> Option<&[CfdTypeId]> {
        self.domain_index.domain_members(domain_id)
    }

    /// Looks up a record by its actual type and key.
    #[must_use]
    pub fn record_by_type_key(&self, type_name: &str, key: &str) -> Option<CfdRecordId> {
        let type_id = self.type_id(type_name)?;
        self.record_by_type_key
            .get(&(type_id, key.to_string()))
            .copied()
    }

    /// Looks up a record by inheritance connected-component domain and key.
    #[must_use]
    pub fn record_by_domain_key(&self, domain_id: CfdDomainId, key: &str) -> Option<CfdRecordId> {
        self.record_by_domain_key
            .get(&(domain_id, key.to_string()))
            .copied()
    }

    fn type_is_assignable_by_name(&self, actual_type: &str, expected_type: &str) -> bool {
        let Some(actual_type_id) = self.type_id(actual_type) else {
            return false;
        };
        let Some(expected_type_id) = self.type_id(expected_type) else {
            return false;
        };
        actual_type_id == expected_type_id
            || self
                .type_ancestors(actual_type_id)
                .is_some_and(|ancestors| ancestors.contains(&expected_type_id))
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

    /// Look up the resolved target id for the `CfdValue::Ref` at `site`.
    ///
    /// Returns `None` when no ref lives at that path.
    #[must_use]
    pub fn resolve_ref(&self, site: &RefSite) -> Option<CfdRecordId> {
        self.ref_edge_at(site)
            .and_then(|edge_id| self.ref_edge(edge_id))
            .map(|edge| edge.target)
    }

    /// Convenience for the common case "I have a host id and a path; tell me
    /// where the Ref at that path resolves to". Equivalent to
    /// [`CfdDataModel::resolve_ref`] with a freshly constructed `RefSite`.
    #[must_use]
    pub fn resolve_ref_at(&self, host: CfdRecordId, path: &CfdPath) -> Option<CfdRecordId> {
        self.resolve_ref(&RefSite::new(host, path.clone()))
    }

    /// Iterate every resolved `CfdValue::Ref` site in the model.
    pub fn ref_sites(&self) -> impl Iterator<Item = (&RefSite, CfdRecordId)> {
        self.ref_edges.iter().map(|edge| (&edge.site, edge.target))
    }

    #[must_use]
    pub fn ref_edge(&self, id: RefEdgeId) -> Option<&RefEdge> {
        self.ref_edges.get(id.index())
    }

    #[must_use]
    pub fn ref_edge_at(&self, site: &RefSite) -> Option<RefEdgeId> {
        self.ref_by_site.get(site).copied()
    }

    pub fn ref_edges(&self) -> impl Iterator<Item = &RefEdge> {
        self.ref_edges.iter()
    }

    pub fn ref_edges_from_host(&self, host: CfdRecordId) -> impl Iterator<Item = &RefEdge> + '_ {
        self.ref_by_host
            .get(&host)
            .into_iter()
            .flat_map(|ids| ids.iter())
            .filter_map(|id| self.ref_edge(*id))
    }

    pub fn ref_edges_to_target(&self, target: CfdRecordId) -> impl Iterator<Item = &RefEdge> + '_ {
        self.ref_by_target
            .get(&target)
            .into_iter()
            .flat_map(|ids| ids.iter())
            .filter_map(|id| self.ref_edge(*id))
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
        key: impl Into<String>,
        actual_type: impl Into<String>,
        fields: impl IntoIterator<Item = (impl Into<String>, CfdInputValue)>,
    ) -> &mut Self {
        self.records
            .push(CfdInputRecord::new(key, actual_type, fields));
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
    /// fields, duplicate keys, duplicate dict keys, or unresolved references.
    pub fn build(self) -> Result<CfdDataModel, CfdDiagnostics> {
        ModelCompiler::new(self.schema, self.records).build()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CfdTable {
    pub type_name: String,
    pub records: Vec<CfdRecordId>,
    pub primary_index: BTreeMap<String, CfdRecordId>,
}

/// Index of records assignable to a given root type (`abstract type` or any
/// concrete type with subclasses), keyed by record key.
///
/// The owning `CfdDataModel.inheritance_index` map keys identify the root
/// type — readers obtain it from the lookup, so it is not duplicated here.
#[derive(Debug, Clone, PartialEq)]
pub struct CfdPolymorphicIndex {
    pub records: BTreeMap<String, CfdRecordId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct CfdTypeId(usize);

impl CfdTypeId {
    #[must_use]
    pub(crate) const fn new(index: usize) -> Self {
        Self(index)
    }

    #[must_use]
    pub fn index(self) -> usize {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct CfdDomainId(usize);

impl CfdDomainId {
    #[must_use]
    pub(crate) const fn new(index: usize) -> Self {
        Self(index)
    }

    #[must_use]
    pub fn index(self) -> usize {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CfdDomainIndex {
    pub(crate) type_id_by_name: BTreeMap<String, CfdTypeId>,
    pub(crate) type_names: Vec<String>,
    pub(crate) type_domain: Vec<CfdDomainId>,
    pub(crate) domain_members: Vec<Vec<CfdTypeId>>,
    pub(crate) ancestors_by_type: Vec<Vec<CfdTypeId>>,
}

impl CfdDomainIndex {
    #[must_use]
    pub(crate) fn new(
        type_id_by_name: BTreeMap<String, CfdTypeId>,
        type_names: Vec<String>,
        type_domain: Vec<CfdDomainId>,
        domain_members: Vec<Vec<CfdTypeId>>,
        ancestors_by_type: Vec<Vec<CfdTypeId>>,
    ) -> Self {
        Self {
            type_id_by_name,
            type_names,
            type_domain,
            domain_members,
            ancestors_by_type,
        }
    }

    #[must_use]
    pub(crate) fn type_id(&self, type_name: &str) -> Option<CfdTypeId> {
        self.type_id_by_name.get(type_name).copied()
    }

    #[must_use]
    pub(crate) fn type_name(&self, type_id: CfdTypeId) -> Option<&str> {
        self.type_names.get(type_id.index()).map(String::as_str)
    }

    #[must_use]
    pub(crate) fn type_domain(&self, type_id: CfdTypeId) -> Option<CfdDomainId> {
        self.type_domain.get(type_id.index()).copied()
    }

    #[must_use]
    pub(crate) fn type_domain_by_name(&self, type_name: &str) -> Option<CfdDomainId> {
        self.type_domain(self.type_id(type_name)?)
    }

    #[must_use]
    pub(crate) fn domain_members(&self, domain_id: CfdDomainId) -> Option<&[CfdTypeId]> {
        self.domain_members
            .get(domain_id.index())
            .map(Vec::as_slice)
    }

    #[must_use]
    pub(crate) fn type_ancestors(&self, type_id: CfdTypeId) -> Option<&[CfdTypeId]> {
        self.ancestors_by_type
            .get(type_id.index())
            .map(Vec::as_slice)
    }
}

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
    /// Not exported to wire — origin metadata is internal to the engine and
    /// not consumed by editor frontends (which route by stable coordinate).
    #[serde(skip)]
    #[cfg_attr(feature = "ts-export", ts(skip))]
    pub origin: RecordOrigin,
    /// For top-level records only: maps a field name that was imported via a
    /// `...spread` expansion to the source record id whose origin should be
    /// used when writing the field back. Direct fields are not present in
    /// this map.
    ///
    /// `#[serde(skip)]` because `CfdRecordId` is an internal index; wire
    /// consumers receive `SpreadInfo` derived via [`crate::CfdDataModel`]
    /// look-ups instead. Round-trip through serde leaves this empty, which
    /// `model.build()` repopulates correctly.
    #[serde(skip)]
    #[cfg_attr(feature = "ts-export", ts(skip))]
    pub spread_field_sources: BTreeMap<String, CfdRecordId>,
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

    /// Effective origin used to write a top-level field. If the field was
    /// imported via `...spread`, returns the spread source's record id (the
    /// caller resolves it to a real `RecordOrigin` via the model). Otherwise
    /// returns `None` and the caller uses `self.origin`.
    #[must_use]
    pub fn spread_source_for_field(&self, field: &str) -> Option<CfdRecordId> {
        self.spread_field_sources.get(field).copied()
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct CfdRecordId(usize);

impl CfdRecordId {
    #[must_use]
    pub(crate) fn new(index: usize) -> Self {
        Self(index)
    }

    /// Build a record id from its raw index. Mostly useful for diagnostic
    /// rewriting where the caller knows the absolute index in a flattened
    /// record stream. Construction does not validate that any record exists
    /// at that index.
    #[must_use]
    pub fn from_index(index: usize) -> Self {
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

#[derive(Debug, Clone, PartialEq)]
pub struct CfdInputRecord {
    pub key: String,
    pub actual_type: String,
    pub spreads: Vec<CfdInputValue>,
    pub fields: BTreeMap<String, CfdInputValue>,
    /// Where this top-level record originated. Loaders set this when parsing;
    /// synthetic records (tests, ad-hoc construction) leave it as
    /// [`RecordOrigin::None`]. The compiler moves this onto the resulting
    /// [`CfdRecord`].
    pub origin: RecordOrigin,
}

impl CfdInputRecord {
    #[must_use]
    pub fn new(
        key: impl Into<String>,
        actual_type: impl Into<String>,
        fields: impl IntoIterator<Item = (impl Into<String>, CfdInputValue)>,
    ) -> Self {
        Self {
            key: key.into(),
            actual_type: actual_type.into(),
            spreads: Vec::new(),
            fields: fields
                .into_iter()
                .map(|(name, value)| (name.into(), value))
                .collect(),
            origin: RecordOrigin::None,
        }
    }

    #[must_use]
    pub fn with_spreads(
        key: impl Into<String>,
        actual_type: impl Into<String>,
        spreads: impl IntoIterator<Item = CfdInputValue>,
        fields: impl IntoIterator<Item = (impl Into<String>, CfdInputValue)>,
    ) -> Self {
        Self {
            key: key.into(),
            actual_type: actual_type.into(),
            spreads: spreads.into_iter().collect(),
            fields: fields
                .into_iter()
                .map(|(name, value)| (name.into(), value))
                .collect(),
            origin: RecordOrigin::None,
        }
    }

    /// Builder-style: attach an origin to this input record.
    #[must_use]
    pub fn with_origin(mut self, origin: RecordOrigin) -> Self {
        self.origin = origin;
        self
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
    ObjectSpread {
        actual_type: Option<String>,
        spreads: Vec<CfdInputValue>,
        fields: BTreeMap<String, CfdInputValue>,
    },
    RecordRef(String),
    Array(Vec<CfdInputValue>),
    Dict(Vec<(CfdInputDictKey, CfdInputValue)>),
    DictSpread {
        spreads: Vec<CfdInputValue>,
        entries: Vec<(CfdInputDictKey, CfdInputValue)>,
    },
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
    pub fn object_spread(
        spreads: impl IntoIterator<Item = CfdInputValue>,
        fields: impl IntoIterator<Item = (impl Into<String>, CfdInputValue)>,
    ) -> Self {
        Self::ObjectSpread {
            actual_type: None,
            spreads: spreads.into_iter().collect(),
            fields: fields
                .into_iter()
                .map(|(name, value)| (name.into(), value))
                .collect(),
        }
    }

    #[must_use]
    pub fn object_spread_with_actual_type(
        actual_type: impl Into<String>,
        spreads: impl IntoIterator<Item = CfdInputValue>,
        fields: impl IntoIterator<Item = (impl Into<String>, CfdInputValue)>,
    ) -> Self {
        Self::ObjectSpread {
            actual_type: Some(actual_type.into()),
            spreads: spreads.into_iter().collect(),
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

    #[must_use]
    pub fn dict_spread(
        spreads: impl IntoIterator<Item = CfdInputValue>,
        entries: impl IntoIterator<Item = (CfdInputDictKey, CfdInputValue)>,
    ) -> Self {
        Self::DictSpread {
            spreads: spreads.into_iter().collect(),
            entries: entries.into_iter().collect(),
        }
    }

    #[must_use]
    pub fn record_ref(key: impl Into<String>) -> Self {
        Self::RecordRef(key.into())
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
