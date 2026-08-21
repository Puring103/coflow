mod dimensions;
mod edges;
mod ids;
mod tables;
mod value;

pub use dimensions::{DimensionFieldLookupError, DimensionValueLookup};
pub(crate) use edges::RefEdgeId;
pub use edges::{DimensionRefCoordinate, RefEdge, RefSite};
pub use ids::{CfdRecordId, RecordCoordinate};
pub use tables::CfdTable;
pub use value::{
    CfdDictKey, CfdDimensionFieldValues, CfdDimensionValue, CfdEnumValue, CfdFormattedString,
    CfdObject, CfdRecord, CfdValue,
};

use crate::build::CfdModelBuilder;
use crate::indexes::RefIndexes;
use coflow_cft::{CftSchema, RecordKey, TypeName};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub struct CfdDataModel {
    pub(crate) tables: BTreeMap<TypeName, CfdTable>,
    pub(crate) record_by_domain_key: BTreeMap<TypeName, BTreeMap<RecordKey, CfdRecordId>>,
    pub(crate) records: Vec<CfdRecord>,
    pub(crate) refs: RefIndexes,
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

    /// Returns top-level records assignable to `expected_type` in stable
    /// `(actual_type, record_key)` order.
    #[must_use]
    pub fn assignable_records(&self, schema: &CftSchema, expected_type: &str) -> Vec<CfdRecordId> {
        self.tables
            .iter()
            .filter(|(actual_type, _)| schema.is_assignable(actual_type, expected_type))
            .flat_map(|(_, table)| table.primary_index.values().copied())
            .collect()
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
        self.tables.get(type_name)?.primary_index.get(key).copied()
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

    /// Look up the target id for the `CfdValue::Ref` at `site`.
    ///
    /// Returns `None` when no ref lives at that path.
    #[must_use]
    pub fn resolve_ref(&self, site: &RefSite) -> Option<CfdRecordId> {
        self.refs
            .by_site
            .get(site)
            .and_then(|edge_id| self.refs.edges.get(edge_id.index()))
            .map(|edge| edge.target)
    }

    pub fn ref_edges(&self) -> impl Iterator<Item = &RefEdge> {
        self.refs.edges.iter()
    }

    pub fn ref_edges_from_host(&self, host: CfdRecordId) -> impl Iterator<Item = &RefEdge> + '_ {
        self.refs
            .by_host
            .get(&host)
            .into_iter()
            .flat_map(|ids| ids.iter())
            .filter_map(|id| self.refs.edges.get(id.index()))
    }

    pub fn ref_edges_to_target(&self, target: CfdRecordId) -> impl Iterator<Item = &RefEdge> + '_ {
        self.refs
            .by_target
            .get(&target)
            .into_iter()
            .flat_map(|ids| ids.iter())
            .filter_map(|id| self.refs.edges.get(id.index()))
    }
}
