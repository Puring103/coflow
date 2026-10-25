mod dimensions;
mod edges;
mod ids;
mod projection;
mod tables;
mod value;

pub use dimensions::{DimensionFieldLookupError, DimensionValueLookup};
pub(crate) use edges::RefEdgeId;
pub use edges::{DimensionRefCoordinate, RefEdge, RefSite};
pub use ids::{CfdRecordHandle, RecordCoordinate};
pub use tables::CfdTable;
pub use value::{
    CfdDictKey, CfdDimensionFieldValues, CfdDimensionValue, CfdEnumValue, CfdFormattedString,
    CfdObject, CfdRecord, CfdValue,
};

use crate::build::CfdModelBuilder;
use crate::indexes::RefIndexes;
use coflow_language::cft::{CftSchema, DimensionName, RecordId, TypeName, VariantName};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DimensionProjection {
    // 视图始终共享最初的基础模型，后续选择不能把前一视图的覆盖误当作基础来源。
    source: Arc<CfdDataModel>,
    selections: BTreeMap<String, String>,
}

/// 不可变模型共享记录与索引；克隆可用于代次快照和投影来源，无需复制数据树。
#[derive(Debug, Clone)]
pub struct CfdDataModel {
    pub(crate) tables: Arc<BTreeMap<TypeName, CfdTable>>,
    pub(crate) record_by_domain_id: Arc<BTreeMap<TypeName, BTreeMap<RecordId, CfdRecordHandle>>>,
    pub(crate) records: Arc<Vec<Arc<CfdRecord>>>,
    pub(crate) refs: Arc<RefIndexes>,
    pub(crate) dimension_variants: Arc<BTreeMap<DimensionName, BTreeSet<VariantName>>>,
    pub(crate) projection: Option<DimensionProjection>,
    pub(crate) template_dependencies: Arc<crate::build::TemplateDependencies>,
    pub(crate) incremental_ready: bool,
    pub(crate) structural_limits: coflow_language::limits::StructuralLimits,
    pub(crate) schema_compilation: coflow_language::cft::CftSchemaIdentity,
}

impl PartialEq for CfdDataModel {
    fn eq(&self, other: &Self) -> bool {
        // 编译身份与增量依赖属于缓存元数据，不改变模型原有值语义。
        self.tables == other.tables
            && self.record_by_domain_id == other.record_by_domain_id
            && self.records == other.records
            && self.refs == other.refs
            && self.dimension_variants == other.dimension_variants
            && self.projection == other.projection
    }
}

impl CfdDataModel {
    pub fn dimension_variants(&self, dimension: &str) -> impl Iterator<Item = &VariantName> {
        self.dimension_variants
            .get(dimension)
            .into_iter()
            .flat_map(|variants| variants.iter())
    }

    #[must_use]
    pub fn builder(schema: &CftSchema) -> CfdModelBuilder<'_> {
        CfdModelBuilder::new(schema)
    }

    #[must_use]
    pub fn record(&self, id: CfdRecordHandle) -> Option<&CfdRecord> {
        self.records.get(id.index()).map(AsRef::as_ref)
    }

    pub fn records(&self) -> impl Iterator<Item = (CfdRecordHandle, &CfdRecord)> {
        self.records
            .iter()
            .enumerate()
            .map(|(index, record)| (CfdRecordHandle::new(index), record.as_ref()))
    }

    /// Returns top-level records assignable to `expected_type` in stable
    /// `(actual_type, record_id)` order.
    #[must_use]
    pub fn assignable_records(
        &self,
        schema: &CftSchema,
        expected_type: &str,
    ) -> Vec<CfdRecordHandle> {
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

    /// Looks up a record assignable to `expected_type` by id.
    ///
    /// This is intentionally not an exact `(actual_type, id)` lookup:
    /// inherited ranges resolve through the type's domain and then verify
    /// assignability. Use [`CfdDataModel::record_by_type_id`] when callers
    /// need the record's actual type to match exactly.
    #[must_use]
    pub fn lookup_assignable(
        &self,
        schema: &CftSchema,
        expected_type: &str,
        id: &str,
    ) -> Option<CfdRecordHandle> {
        if let Some(inheritance_root) = schema.inheritance_root(expected_type) {
            if let Some(record_id) = self.record_by_domain_id(inheritance_root, id) {
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
            .and_then(|table| table.primary_index.get(id))
            .copied()
    }

    /// Looks up a record by its actual type and id.
    #[must_use]
    pub fn record_by_type_id(&self, type_name: &str, id: &str) -> Option<CfdRecordHandle> {
        self.tables.get(type_name)?.primary_index.get(id).copied()
    }

    /// Looks up a record by canonical inheritance root and id.
    #[must_use]
    pub fn record_by_domain_id(&self, inheritance_root: &str, id: &str) -> Option<CfdRecordHandle> {
        self.record_by_domain_id
            .get(inheritance_root)?
            .get(id)
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
    ) -> impl Iterator<Item = (CfdRecordHandle, &'a CfdRecord)> + 'a {
        let ids = self
            .tables
            .get(type_name)
            .map_or(&[] as &[CfdRecordHandle], |table| table.records.as_slice());
        ids.iter()
            .filter_map(move |id| self.record(*id).map(|record| (*id, record)))
    }

    /// Iterates over records whose actual type is assignable to `type_name`.
    ///
    /// Unlike [`Self::records_of_type`], this includes records of every
    /// descendant type and preserves insertion order.
    pub fn records_assignable_to<'a>(
        &'a self,
        schema: &'a CftSchema,
        type_name: &'a str,
    ) -> impl Iterator<Item = (CfdRecordHandle, &'a CfdRecord)> + 'a {
        self.records()
            .filter(move |(_, record)| schema.is_assignable(record.actual_type(), type_name))
    }

    /// Look up the target id for the `CfdValue::Ref` at `site`.
    ///
    /// Returns `None` when no ref lives at that path.
    #[must_use]
    pub fn resolve_ref(&self, site: &RefSite) -> Option<CfdRecordHandle> {
        self.refs.resolve(site)
    }

    pub fn ref_edges(&self) -> impl Iterator<Item = &RefEdge> {
        self.refs.edges()
    }

    pub fn ref_edges_from_host(
        &self,
        host: CfdRecordHandle,
    ) -> impl Iterator<Item = &RefEdge> + '_ {
        self.refs.from_host(host)
    }

    pub fn ref_edges_to_target(
        &self,
        target: CfdRecordHandle,
    ) -> impl Iterator<Item = &RefEdge> + '_ {
        self.refs.to_target(target)
    }
}
