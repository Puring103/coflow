use crate::build::{BuildSchema, RecordDraft, ValueDraft};
use crate::diagnostics::CfdPath;
use crate::model::{
    CfdRecord, CfdRecordId, CfdValue, DimensionRefCoordinate, RefEdge, RefEdgeId, RefSite,
    SpreadEdge, SpreadEdgeId,
};
use coflow_cft::{CftValueType, RecordKey, TypeName};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(crate) struct RefIndexes {
    pub(crate) edges: Vec<RefEdge>,
    pub(crate) by_site: BTreeMap<RefSite, RefEdgeId>,
    pub(crate) by_host: BTreeMap<CfdRecordId, Vec<RefEdgeId>>,
    pub(crate) by_target: BTreeMap<CfdRecordId, Vec<RefEdgeId>>,
}

#[derive(Default)]
pub(crate) struct SpreadIndexes {
    pub(crate) edges: Vec<SpreadEdge>,
    pub(crate) by_host: BTreeMap<CfdRecordId, Vec<SpreadEdgeId>>,
    pub(crate) by_source: BTreeMap<CfdRecordId, Vec<SpreadEdgeId>>,
}

#[derive(Clone, Copy)]
pub(crate) struct SpreadIndexContext<'a, 'schema> {
    drafts: &'a [RecordDraft],
    record_by_domain_key: &'a BTreeMap<TypeName, BTreeMap<RecordKey, CfdRecordId>>,
    schema: BuildSchema<'schema>,
}

impl<'a, 'schema> SpreadIndexContext<'a, 'schema> {
    pub(crate) const fn new(
        drafts: &'a [RecordDraft],
        record_by_domain_key: &'a BTreeMap<TypeName, BTreeMap<RecordKey, CfdRecordId>>,
        schema: BuildSchema<'schema>,
    ) -> Self {
        Self {
            drafts,
            record_by_domain_key,
            schema,
        }
    }
}

pub(crate) fn build_spread_indexes(context: SpreadIndexContext<'_, '_>) -> SpreadIndexes {
    let mut out = SpreadIndexes::default();
    {
        let mut builder = SpreadEdgeBuilder {
            context,
            out: &mut out,
        };
        for (index, draft) in context.drafts.iter().enumerate() {
            builder.collect_record(draft, CfdRecordId::new(index), &CfdPath::root(), None);
        }
    }
    out
}

pub(crate) fn extend_dimension_spread_indexes(
    out: &mut SpreadIndexes,
    value: &ValueDraft,
    host: CfdRecordId,
    path: &CfdPath,
    dimension: &DimensionRefCoordinate,
    context: SpreadIndexContext<'_, '_>,
) {
    SpreadEdgeBuilder { context, out }.collect_value(value, host, path, Some(dimension));
}

struct SpreadEdgeBuilder<'a, 'context, 'schema> {
    context: SpreadIndexContext<'context, 'schema>,
    out: &'a mut SpreadIndexes,
}

impl SpreadEdgeBuilder<'_, '_, '_> {
    fn collect_record(
        &mut self,
        draft: &RecordDraft,
        host: CfdRecordId,
        path: &CfdPath,
        dimension: Option<&DimensionRefCoordinate>,
    ) {
        let mut fields_by_source = draft
            .spread_sources
            .iter()
            .cloned()
            .map(|source| (source, BTreeSet::new()))
            .collect::<BTreeMap<_, _>>();
        for (field, source) in &draft.spread_field_sources {
            fields_by_source
                .entry(source.clone())
                .or_default()
                .insert(field.clone());
        }

        for (source, fields) in fields_by_source {
            let schema = self.context.schema;
            let Some(source_id) = lookup_domain_ref(
                schema,
                self.context.record_by_domain_key,
                &source.expected_type,
                &source.key,
            ) else {
                continue;
            };
            let id = SpreadEdgeId::new(self.out.edges.len());
            self.out.edges.push(SpreadEdge {
                host,
                path: path.clone(),
                dimension: dimension.cloned(),
                fields,
                source: source_id,
            });
            self.out.by_host.entry(host).or_default().push(id);
            self.out.by_source.entry(source_id).or_default().push(id);
        }

        for (field, value) in &draft.fields {
            self.collect_value(value, host, &path.clone().field(field.as_str()), dimension);
        }
    }

    fn collect_value(
        &mut self,
        value: &ValueDraft,
        host: CfdRecordId,
        path: &CfdPath,
        dimension: Option<&DimensionRefCoordinate>,
    ) {
        match value {
            ValueDraft::Object(draft) => self.collect_record(draft, host, path, dimension),
            ValueDraft::Array(items) => {
                for (index, item) in items.iter().enumerate() {
                    self.collect_value(item, host, &path.clone().index(index), dimension);
                }
            }
            ValueDraft::Dict(entries) => {
                for (key, item) in entries {
                    self.collect_value(item, host, &path.clone().dict_key_value(key), dimension);
                }
            }
            ValueDraft::DictSpread { spreads, entries } => {
                for item in spreads {
                    self.collect_value(item, host, path, dimension);
                }
                for (key, item) in entries {
                    self.collect_value(item, host, &path.clone().dict_key_value(key), dimension);
                }
            }
            ValueDraft::Value(_)
            | ValueDraft::PendingRef { .. }
            | ValueDraft::PendingSpreadField { .. } => {}
        }
    }
}

pub(crate) fn build_ref_indexes(
    records: &[CfdRecord],
    record_by_domain_key: &BTreeMap<TypeName, BTreeMap<RecordKey, CfdRecordId>>,
    schema: BuildSchema<'_>,
    spread_edges: &[SpreadEdge],
) -> RefIndexes {
    let mut out = RefIndexes::default();
    let mut spread_edges_by_host = BTreeMap::<CfdRecordId, Vec<&SpreadEdge>>::new();
    for edge in spread_edges {
        spread_edges_by_host
            .entry(edge.host)
            .or_default()
            .push(edge);
    }
    let context = RefEdgeBuildContext {
        record_by_domain_key,
        schema,
        spread_edges_by_host,
    };
    for (index, record) in records.iter().enumerate() {
        let host = CfdRecordId::new(index);
        let root = CfdPath::root();
        for (name, value) in record.fields() {
            let Some(field) = context
                .schema
                .full_fields(record.actual_type())
                .find(|field| &field.name == name)
            else {
                continue;
            };
            collect_ref_edges(
                value,
                &field.value_type,
                host,
                &root.clone().field(name.as_str()),
                None,
                &context,
                &mut out,
            );
        }
        for (field_name, values) in &record.dimension_fields {
            let Some(field) = context
                .schema
                .full_fields(record.actual_type())
                .find(|field| &field.name == field_name)
            else {
                continue;
            };
            for (variant, value) in &values.variants {
                let coordinate = DimensionRefCoordinate {
                    field: field.name.clone(),
                    dimension: values.dimension.clone(),
                    variant: variant.clone(),
                };
                collect_ref_edges(
                    &value.value,
                    &field.value_type,
                    host,
                    &root.clone().field(field_name.as_str()),
                    Some(&coordinate),
                    &context,
                    &mut out,
                );
            }
        }
    }
    out
}

struct RefEdgeBuildContext<'a, 'schema> {
    record_by_domain_key: &'a BTreeMap<TypeName, BTreeMap<RecordKey, CfdRecordId>>,
    schema: BuildSchema<'schema>,
    spread_edges_by_host: BTreeMap<CfdRecordId, Vec<&'a SpreadEdge>>,
}

impl RefEdgeBuildContext<'_, '_> {
    fn is_spread_inherited_path(
        &self,
        host: CfdRecordId,
        path: &CfdPath,
        dimension: Option<&DimensionRefCoordinate>,
    ) -> bool {
        self.spread_edges_by_host.get(&host).is_some_and(|edges| {
            edges
                .iter()
                .any(|edge| edge.dimension.as_ref() == dimension && edge.covers_path(path))
        })
    }
}

fn collect_ref_edges(
    value: &CfdValue,
    ty: &CftValueType,
    host: CfdRecordId,
    path: &CfdPath,
    dimension: Option<&DimensionRefCoordinate>,
    context: &RefEdgeBuildContext<'_, '_>,
    out: &mut RefIndexes,
) {
    if context.is_spread_inherited_path(host, path, dimension) {
        return;
    }
    match (value, ty.non_nullable()) {
        (CfdValue::Ref(key), CftValueType::RecordRef(expected_type)) => {
            let Some(target) = lookup_domain_ref(
                context.schema,
                context.record_by_domain_key,
                expected_type,
                key,
            ) else {
                return;
            };
            let site = dimension.map_or_else(
                || RefSite::new(host, path.clone()),
                |dimension| RefSite::in_dimension(host, path.clone(), dimension.clone()),
            );
            let id = RefEdgeId::new(out.edges.len());
            out.edges.push(RefEdge {
                site: site.clone(),
                target,
            });
            out.by_site.insert(site, id);
            out.by_host.entry(host).or_default().push(id);
            out.by_target.entry(target).or_default().push(id);
        }
        (CfdValue::Object(boxed), CftValueType::Object(_)) => {
            for (name, inner) in &boxed.fields {
                let Some(field) = context
                    .schema
                    .full_fields(boxed.actual_type.as_str())
                    .find(|field| &field.name == name)
                else {
                    continue;
                };
                collect_ref_edges(
                    inner,
                    &field.value_type,
                    host,
                    &path.clone().field(name.as_str()),
                    dimension,
                    context,
                    out,
                );
            }
        }
        (CfdValue::Array(items), CftValueType::Array(inner_ty)) => {
            for (index, item) in items.iter().enumerate() {
                collect_ref_edges(
                    item,
                    inner_ty,
                    host,
                    &path.clone().index(index),
                    dimension,
                    context,
                    out,
                );
            }
        }
        (CfdValue::Dict(entries), CftValueType::Dict(_, value_ty)) => {
            for (key, item) in entries {
                collect_ref_edges(
                    item,
                    value_ty,
                    host,
                    &path.clone().dict_key_value(key),
                    dimension,
                    context,
                    out,
                );
            }
        }
        _ => {}
    }
}

fn lookup_domain_ref(
    schema: BuildSchema<'_>,
    record_by_domain_key: &BTreeMap<TypeName, BTreeMap<RecordKey, CfdRecordId>>,
    target_type: &str,
    key: &str,
) -> Option<CfdRecordId> {
    schema
        .inheritance_root(target_type)
        .and_then(|inheritance_root| record_by_domain_key.get(inheritance_root))
        .and_then(|records| records.get(key))
        .copied()
}
