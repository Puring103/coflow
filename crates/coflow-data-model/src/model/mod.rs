mod dimensions;
mod edges;
mod ids;
mod tables;
mod value;

pub use dimensions::{DimensionFieldLookupError, DimensionValueLookup};
pub use edges::{DimensionRefCoordinate, RefEdge, RefSite, SpreadEdge};
pub(crate) use edges::{RefEdgeId, SpreadEdgeId};
pub use ids::{CfdRecordId, RecordCoordinate};
pub use tables::CfdTable;
pub use value::{
    CfdDictKey, CfdDimensionFieldValues, CfdDimensionValue, CfdEnumValue, CfdObject, CfdRecord,
    CfdValue,
};

use crate::build::CfdModelBuilder;
use crate::diagnostics::CfdPath;
use coflow_cft::{CftSchema, RecordKey, TypeName};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq)]
pub struct CfdDataModel {
    pub(crate) tables: BTreeMap<TypeName, CfdTable>,
    pub(crate) record_by_type_key: BTreeMap<TypeName, BTreeMap<RecordKey, CfdRecordId>>,
    pub(crate) record_by_domain_key: BTreeMap<TypeName, BTreeMap<RecordKey, CfdRecordId>>,
    pub(crate) records: Vec<CfdRecord>,
    pub(crate) ref_edges: Vec<RefEdge>,
    pub(crate) ref_by_site: BTreeMap<RefSite, RefEdgeId>,
    pub(crate) ref_by_host: BTreeMap<CfdRecordId, Vec<RefEdgeId>>,
    pub(crate) ref_by_target: BTreeMap<CfdRecordId, Vec<RefEdgeId>>,
    pub(crate) spread_edges: Vec<SpreadEdge>,
    pub(crate) spread_by_host: BTreeMap<CfdRecordId, Vec<SpreadEdgeId>>,
    pub(crate) spread_by_source: BTreeMap<CfdRecordId, Vec<SpreadEdgeId>>,
}

impl CfdDataModel {
    #[must_use]
    pub fn builder(schema: &CftSchema) -> CfdModelBuilder<'_> {
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
    pub fn lookup_assignable(
        &self,
        schema: &CftSchema,
        expected_type: &str,
        key: &str,
    ) -> Option<CfdRecordId> {
        if let Some(inheritance_root) = schema.inheritance_root(expected_type) {
            if let Some(record_id) = self.record_by_domain_key(inheritance_root, key) {
                if self.record(record_id).is_some_and(|record| {
                    inheritance_root.as_str() == expected_type
                        || schema.is_assignable(record.actual_type(), expected_type)
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

    /// Looks up a record by its actual type and key.
    #[must_use]
    pub fn record_by_type_key(&self, type_name: &str, key: &str) -> Option<CfdRecordId> {
        self.record_by_type_key.get(type_name)?.get(key).copied()
    }

    /// Looks up a record by canonical inheritance root and key.
    #[must_use]
    pub fn record_by_domain_key(&self, inheritance_root: &str, key: &str) -> Option<CfdRecordId> {
        self.record_by_domain_key
            .get(inheritance_root)?
            .get(key)
            .copied()
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
        schema: &'a CftSchema,
        type_name: &'a str,
    ) -> impl Iterator<Item = (CfdRecordId, &'a CfdRecord)> + 'a {
        self.records()
            .filter(move |(_, record)| schema.is_assignable(record.actual_type(), type_name))
    }

    /// Look up the direct target id for the `CfdValue::Ref` at `site`.
    ///
    /// Returns `None` when no direct ref lives at that path. This does not
    /// follow spread provenance; use [`Self::resolve_ref`] for that.
    #[must_use]
    pub fn resolve_direct_ref(&self, site: &RefSite) -> Option<CfdRecordId> {
        self.ref_by_site
            .get(site)
            .and_then(|edge_id| self.ref_edges.get(edge_id.index()))
            .map(|edge| edge.target)
    }

    /// Resolves a ref at `site`, following default or dimension-overlay spread
    /// provenance when the value was inherited from another record.
    #[must_use]
    pub fn resolve_ref(&self, site: &RefSite) -> Option<CfdRecordId> {
        self.resolve_ref_inner(site, &mut BTreeSet::new())
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
            .filter_map(|id| self.ref_edges.get(id.index()))
    }

    pub fn direct_ref_edges_to_target(
        &self,
        target: CfdRecordId,
    ) -> impl Iterator<Item = &RefEdge> + '_ {
        self.ref_by_target
            .get(&target)
            .into_iter()
            .flat_map(|ids| ids.iter())
            .filter_map(|id| self.ref_edges.get(id.index()))
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
            .filter_map(|id| self.spread_edges.get(id.index()))
    }

    fn spread_edges_from_host(&self, host: CfdRecordId) -> impl Iterator<Item = &SpreadEdge> + '_ {
        self.spread_by_host
            .get(&host)
            .into_iter()
            .flat_map(|ids| ids.iter())
            .filter_map(|id| self.spread_edges.get(id.index()))
    }

    /// Returns the transitive spread-materialization closure, including the
    /// supplied source records themselves.
    #[must_use]
    pub fn materialization_dependents(
        &self,
        sources: impl IntoIterator<Item = CfdRecordId>,
    ) -> BTreeSet<CfdRecordId> {
        let mut visited = sources.into_iter().collect::<BTreeSet<_>>();
        let mut pending = visited.iter().copied().collect::<Vec<_>>();
        while let Some(source) = pending.pop() {
            for edge in self.spread_edges_from_source(source) {
                if visited.insert(edge.host) {
                    pending.push(edge.host);
                }
            }
        }
        visited
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
    fn spread_edge_at_path(&self, host: CfdRecordId, path: &CfdPath) -> Option<&SpreadEdge> {
        self.spread_edges_from_host(host)
            .find(|edge| edge.dimension.is_none() && edge.covers_path(path))
    }

    #[must_use]
    pub fn spread_source_path(
        &self,
        host: CfdRecordId,
        path: &CfdPath,
    ) -> Option<(CfdRecordId, CfdPath)> {
        self.spread_source_path_inner(host, path, &mut BTreeSet::new())
    }

    fn resolve_ref_inner(
        &self,
        site: &RefSite,
        visited: &mut BTreeSet<RefSite>,
    ) -> Option<CfdRecordId> {
        if !visited.insert(site.clone()) {
            return None;
        }
        self.resolve_direct_ref(site).or_else(|| {
            let (source, source_path) = self.spread_source_path_for_site(
                site.host,
                &site.path,
                site.dimension.as_ref(),
                &mut BTreeSet::new(),
            )?;
            self.resolve_ref_inner(&RefSite::new(source, source_path), visited)
        })
    }

    fn spread_source_path_for_site(
        &self,
        host: CfdRecordId,
        path: &CfdPath,
        dimension: Option<&DimensionRefCoordinate>,
        visited: &mut BTreeSet<(CfdRecordId, CfdPath, Option<DimensionRefCoordinate>)>,
    ) -> Option<(CfdRecordId, CfdPath)> {
        let key = (host, path.clone(), dimension.cloned());
        if !visited.insert(key) {
            return None;
        }
        let edge = self
            .spread_edges_from_host(host)
            .find(|edge| edge.dimension.as_ref() == dimension && edge.covers_path(path))?;
        let source_path = edge.source_path_for(path)?;
        self.spread_source_path_for_site(edge.source, &source_path, None, visited)
            .or(Some((edge.source, source_path)))
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
