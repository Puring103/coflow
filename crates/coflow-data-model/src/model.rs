mod dimensions;
mod domain;
mod edges;
mod ids;
mod input;
mod tables;
mod value;

pub use dimensions::{DimensionFieldLookupError, DimensionFieldValue};
pub use domain::CfdDomainIndex;
pub use edges::{RefEdge, RefEdgeId, RefSite, SpreadEdge, SpreadEdgeId, SpreadSite};
pub use ids::{CfdDomainId, CfdRecordId, CfdTypeId};
pub use input::{CfdInputDictKey, CfdInputRecord, CfdInputValue};
pub use tables::{CfdPolymorphicIndex, CfdTable};
pub use value::{CfdDictKey, CfdEnumValue, CfdObject, CfdRecord, CfdValue};

use crate::diagnostic::CfdPath;
use crate::{compiler::ModelCompiler, CfdDiagnostics};
use coflow_cft::CftContainer;
use std::collections::{BTreeMap, BTreeSet};

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
    pub(crate) spread_edges: Vec<SpreadEdge>,
    pub(crate) spread_by_site: BTreeMap<SpreadSite, Vec<SpreadEdgeId>>,
    pub(crate) spread_by_source: BTreeMap<CfdRecordId, Vec<SpreadEdgeId>>,
}

impl CfdDataModel {
    #[must_use]
    pub fn builder(schema: &CftContainer) -> CfdModelBuilder<'_> {
        CfdModelBuilder::new(schema)
    }

    #[must_use]
    pub fn record(&self, id: CfdRecordId) -> Option<&CfdRecord> {
        self.records.get(id.index())
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

    /// Looks up a record assignable to `expected_type` by key.
    ///
    /// This is intentionally not an exact `(actual_type, key)` lookup:
    /// inherited ranges resolve through the type's domain and then verify
    /// assignability. Use [`CfdDataModel::record_by_type_key`] when callers
    /// need the record's actual type to match exactly.
    #[must_use]
    pub fn lookup_assignable(&self, expected_type: &str, key: &str) -> Option<CfdRecordId> {
        if let Some(domain_id) = self.type_domain_id(expected_type) {
            if let Some(record_id) = self.record_by_domain_key(domain_id, key) {
                if self.record(record_id).is_some_and(|record| {
                    self.type_is_assignable_by_name(record.actual_type(), expected_type)
                }) {
                    return Some(record_id);
                }
            }
        }
        self.tables
            .get(expected_type)
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
            .filter_map(move |id| self.records.get(id.index()).map(|record| (*id, record)))
    }

    /// Iterates over records whose actual type is assignable to `type_name`.
    ///
    /// Unlike [`Self::records_of_type`], this includes records of every
    /// descendant type and preserves insertion order.
    pub fn records_assignable_to<'a>(
        &'a self,
        type_name: &'a str,
    ) -> impl Iterator<Item = (CfdRecordId, &'a CfdRecord)> + 'a {
        self.records().filter(move |(_, record)| {
            self.type_is_assignable_by_name(record.actual_type(), type_name)
        })
    }

    /// Look up the direct target id for the `CfdValue::Ref` at `site`.
    ///
    /// Returns `None` when no direct ref lives at that path. This does not
    /// follow spread provenance; use [`Self::resolve_effective_ref`] for that.
    #[must_use]
    pub fn resolve_direct_ref(&self, site: &RefSite) -> Option<CfdRecordId> {
        self.direct_ref_edge_at(site)
            .and_then(|edge_id| self.direct_ref_edge(edge_id))
            .map(|edge| edge.target)
    }

    /// Convenience for the common case "I have a host id and a path; tell me
    /// where the direct Ref at that path resolves to". Equivalent to
    /// [`CfdDataModel::resolve_direct_ref`] with a freshly constructed
    /// `RefSite`.
    #[must_use]
    pub fn resolve_direct_ref_at(&self, host: CfdRecordId, path: &CfdPath) -> Option<CfdRecordId> {
        self.resolve_direct_ref(&RefSite::new(host, path.clone()))
    }

    /// Resolves a ref at `site`, following spread provenance when the value at
    /// that site was inherited from another record.
    #[must_use]
    pub fn resolve_effective_ref(&self, site: &RefSite) -> Option<CfdRecordId> {
        self.resolve_effective_ref_inner(site, &mut BTreeSet::new())
    }

    #[must_use]
    pub fn resolve_effective_ref_at(
        &self,
        host: CfdRecordId,
        path: &CfdPath,
    ) -> Option<CfdRecordId> {
        self.resolve_effective_ref(&RefSite::new(host, path.clone()))
    }

    /// Iterate every resolved `CfdValue::Ref` site in the model.
    pub fn direct_ref_sites(&self) -> impl Iterator<Item = (&RefSite, CfdRecordId)> {
        self.ref_edges.iter().map(|edge| (&edge.site, edge.target))
    }

    #[must_use]
    pub fn direct_ref_edge(&self, id: RefEdgeId) -> Option<&RefEdge> {
        self.ref_edges.get(id.index())
    }

    #[must_use]
    pub fn direct_ref_edge_at(&self, site: &RefSite) -> Option<RefEdgeId> {
        self.ref_by_site.get(site).copied()
    }

    pub fn direct_ref_edges(&self) -> impl Iterator<Item = &RefEdge> {
        self.ref_edges.iter()
    }

    pub fn direct_ref_edges_from_host(
        &self,
        host: CfdRecordId,
    ) -> impl Iterator<Item = &RefEdge> + '_ {
        self.ref_by_host
            .get(&host)
            .into_iter()
            .flat_map(|ids| ids.iter())
            .filter_map(|id| self.direct_ref_edge(*id))
    }

    pub fn direct_ref_edges_to_target(
        &self,
        target: CfdRecordId,
    ) -> impl Iterator<Item = &RefEdge> + '_ {
        self.ref_by_target
            .get(&target)
            .into_iter()
            .flat_map(|ids| ids.iter())
            .filter_map(|id| self.direct_ref_edge(*id))
    }

    #[must_use]
    pub fn spread_edge(&self, id: SpreadEdgeId) -> Option<&SpreadEdge> {
        self.spread_edges.get(id.index())
    }

    #[must_use]
    pub fn spread_edge_at(&self, site: &SpreadSite) -> Option<SpreadEdgeId> {
        self.spread_by_site
            .get(site)
            .and_then(|ids| ids.first())
            .copied()
    }

    pub fn spread_edges_at(&self, site: &SpreadSite) -> impl Iterator<Item = &SpreadEdge> + '_ {
        self.spread_by_site
            .get(site)
            .into_iter()
            .flat_map(|ids| ids.iter())
            .filter_map(|id| self.spread_edge(*id))
    }

    pub fn spread_edges(&self) -> impl Iterator<Item = &SpreadEdge> {
        self.spread_edges.iter()
    }

    pub fn spread_edges_from_source(
        &self,
        source: CfdRecordId,
    ) -> impl Iterator<Item = &SpreadEdge> + '_ {
        self.spread_by_source
            .get(&source)
            .into_iter()
            .flat_map(|ids| ids.iter())
            .filter_map(|id| self.spread_edge(*id))
    }

    /// Returns the source record whose spread supplied the value at `site`.
    #[must_use]
    pub fn spread_source_at(&self, site: &SpreadSite) -> Option<CfdRecordId> {
        self.spread_edge_at(site)
            .and_then(|edge_id| self.spread_edge(edge_id))
            .map(|edge| edge.source)
    }

    /// Returns the source record whose spread supplied the value at `path`.
    ///
    /// `SpreadEdge` sites are object-level. A field is inherited from a spread
    /// when its path is at least one segment below the object site and the first
    /// relative segment is one of that edge's inherited fields.
    #[must_use]
    pub fn spread_source_at_path(&self, host: CfdRecordId, path: &CfdPath) -> Option<CfdRecordId> {
        self.spread_edge_at_path(host, path).map(|edge| edge.source)
    }

    #[must_use]
    pub fn spread_edge_at_path(&self, host: CfdRecordId, path: &CfdPath) -> Option<&SpreadEdge> {
        self.spread_edges
            .iter()
            .find(|edge| edge.host == host && edge.covers_path(path))
    }

    #[must_use]
    pub fn spread_source_path(
        &self,
        host: CfdRecordId,
        path: &CfdPath,
    ) -> Option<(CfdRecordId, CfdPath)> {
        self.spread_source_path_inner(host, path, &mut BTreeSet::new())
    }

    fn resolve_effective_ref_inner(
        &self,
        site: &RefSite,
        visited: &mut BTreeSet<RefSite>,
    ) -> Option<CfdRecordId> {
        if !visited.insert(site.clone()) {
            return None;
        }
        self.resolve_direct_ref(site).or_else(|| {
            let (source, source_path) =
                self.spread_source_path_inner(site.host, &site.path, &mut BTreeSet::new())?;
            self.resolve_effective_ref_inner(&RefSite::new(source, source_path), visited)
        })
    }

    fn spread_source_path_inner(
        &self,
        host: CfdRecordId,
        path: &CfdPath,
        visited: &mut BTreeSet<(CfdRecordId, CfdPath)>,
    ) -> Option<(CfdRecordId, CfdPath)> {
        if !visited.insert((host, path.clone())) {
            return None;
        }
        let edge = self.spread_edge_at_path(host, path)?;
        let source_path = edge.source_path_for(path)?;
        self.spread_source_path_inner(edge.source, &source_path, visited)
            .or(Some((edge.source, source_path)))
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
