use crate::build::{BuildSchema, RecordDraft, ValueDraft};
use crate::diagnostics::CfdPath;
use crate::model::{
    CfdRecord, CfdRecordId, CfdValue, DimensionRefCoordinate, RefEdge, RefEdgeId, RefSite,
    SpreadEdge, SpreadEdgeId, SpreadSite,
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
    pub(crate) by_site: BTreeMap<SpreadSite, Vec<SpreadEdgeId>>,
    pub(crate) by_host: BTreeMap<CfdRecordId, Vec<SpreadEdgeId>>,
    pub(crate) by_source: BTreeMap<CfdRecordId, Vec<SpreadEdgeId>>,
}

pub(crate) fn build_spread_indexes(
    drafts: &[RecordDraft],
    record_by_domain_key: &BTreeMap<TypeName, BTreeMap<RecordKey, CfdRecordId>>,
    schema: &BuildSchema<'_>,
) -> SpreadIndexes {
    let mut out = SpreadIndexes::default();
    for (index, draft) in drafts.iter().enumerate() {
        collect_spread_edges(
            draft,
            CfdRecordId::from_index(index),
            &CfdPath::root(),
            drafts,
            record_by_domain_key,
            schema,
            &mut out,
        );
    }
    out
}

fn collect_spread_edges(
    draft: &RecordDraft,
    host: CfdRecordId,
    path: &CfdPath,
    drafts: &[RecordDraft],
    record_by_domain_key: &BTreeMap<TypeName, BTreeMap<RecordKey, CfdRecordId>>,
    schema: &BuildSchema<'_>,
    out: &mut SpreadIndexes,
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
        let Some(expected_type) = schema
            .resolve_type(&source.expected_type)
            .map(|ty| ty.name.clone())
        else {
            continue;
        };
        let Some(inheritance_root) = schema.inheritance_root(&source.expected_type).cloned() else {
            continue;
        };
        let Some((source_id, source_key)) = lookup_domain_ref(
            schema,
            record_by_domain_key,
            &source.expected_type,
            &source.key,
        ) else {
            continue;
        };
        let Some(source_draft) = drafts.get(source_id.index()) else {
            continue;
        };
        let Some(source_type) = schema
            .resolve_type(&source_draft.actual_type)
            .map(|ty| ty.name.clone())
        else {
            continue;
        };
        let site = SpreadSite::new(host, path.clone());
        let id = SpreadEdgeId::new(out.edges.len());
        out.edges.push(SpreadEdge {
            id,
            site: site.clone(),
            host,
            path: path.clone(),
            fields,
            expected_type,
            inheritance_root,
            source_key,
            source: source_id,
            source_type,
        });
        out.by_site.entry(site).or_default().push(id);
        out.by_host.entry(host).or_default().push(id);
        out.by_source.entry(source_id).or_default().push(id);
    }

    for (field, value) in &draft.fields {
        collect_nested_spread_edges(
            value,
            host,
            &path.clone().field(field.as_str()),
            drafts,
            record_by_domain_key,
            schema,
            out,
        );
    }
}

fn collect_nested_spread_edges(
    value: &ValueDraft,
    host: CfdRecordId,
    path: &CfdPath,
    drafts: &[RecordDraft],
    record_by_domain_key: &BTreeMap<TypeName, BTreeMap<RecordKey, CfdRecordId>>,
    schema: &BuildSchema<'_>,
    out: &mut SpreadIndexes,
) {
    match value {
        ValueDraft::Object(draft) => {
            collect_spread_edges(draft, host, path, drafts, record_by_domain_key, schema, out);
        }
        ValueDraft::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                collect_nested_spread_edges(
                    item,
                    host,
                    &path.clone().index(index),
                    drafts,
                    record_by_domain_key,
                    schema,
                    out,
                );
            }
        }
        ValueDraft::Dict(entries) => {
            for (key, item) in entries {
                collect_nested_spread_edges(
                    item,
                    host,
                    &path.clone().dict_key_value(key),
                    drafts,
                    record_by_domain_key,
                    schema,
                    out,
                );
            }
        }
        ValueDraft::DictSpread { spreads, entries } => {
            for item in spreads {
                collect_nested_spread_edges(
                    item,
                    host,
                    path,
                    drafts,
                    record_by_domain_key,
                    schema,
                    out,
                );
            }
            for (key, item) in entries {
                collect_nested_spread_edges(
                    item,
                    host,
                    &path.clone().dict_key_value(key),
                    drafts,
                    record_by_domain_key,
                    schema,
                    out,
                );
            }
        }
        ValueDraft::Value(_)
        | ValueDraft::PendingRef { .. }
        | ValueDraft::PendingSpreadField { .. } => {}
    }
}

pub(crate) fn build_ref_indexes(
    records: &[CfdRecord],
    record_by_domain_key: &BTreeMap<TypeName, BTreeMap<RecordKey, CfdRecordId>>,
    schema: &BuildSchema<'_>,
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
        records,
        record_by_domain_key,
        schema,
        spread_edges_by_host,
    };
    for (index, record) in records.iter().enumerate() {
        let host = CfdRecordId::from_index(index);
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
    records: &'a [CfdRecord],
    record_by_domain_key: &'a BTreeMap<TypeName, BTreeMap<RecordKey, CfdRecordId>>,
    schema: &'a BuildSchema<'schema>,
    spread_edges_by_host: BTreeMap<CfdRecordId, Vec<&'a SpreadEdge>>,
}

impl RefEdgeBuildContext<'_, '_> {
    fn is_spread_inherited_path(&self, host: CfdRecordId, path: &CfdPath) -> bool {
        self.spread_edges_by_host
            .get(&host)
            .is_some_and(|edges| edges.iter().any(|edge| edge.covers_path(path)))
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
    if dimension.is_none() && context.is_spread_inherited_path(host, path) {
        return;
    }
    match (value, ty.non_nullable()) {
        (CfdValue::Ref(key), CftValueType::RecordRef(expected_type)) => {
            let Some(expected_type_name) = context
                .schema
                .resolve_type(expected_type)
                .map(|ty| ty.name.clone())
            else {
                return;
            };
            let Some(inheritance_root) = context.schema.inheritance_root(expected_type).cloned()
            else {
                return;
            };
            let Some((target, _)) = lookup_domain_ref(
                context.schema,
                context.record_by_domain_key,
                expected_type,
                key,
            ) else {
                return;
            };
            let Some(target_record) = context.records.get(target.index()) else {
                return;
            };
            let Some(target_type) = context
                .schema
                .resolve_type(target_record.actual_type())
                .map(|ty| ty.name.clone())
            else {
                return;
            };
            let site = dimension.map_or_else(
                || RefSite::new(host, path.clone()),
                |dimension| RefSite::in_dimension(host, path.clone(), dimension.clone()),
            );
            let id = RefEdgeId::new(out.edges.len());
            out.edges.push(RefEdge {
                id,
                site: site.clone(),
                expected_type: expected_type_name,
                inheritance_root,
                key: key.clone(),
                target,
                target_type,
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
    schema: &BuildSchema<'_>,
    record_by_domain_key: &BTreeMap<TypeName, BTreeMap<RecordKey, CfdRecordId>>,
    target_type: &str,
    key: &str,
) -> Option<(CfdRecordId, RecordKey)> {
    schema
        .inheritance_root(target_type)
        .and_then(|inheritance_root| record_by_domain_key.get(inheritance_root))
        .and_then(|records| records.get_key_value(key))
        .map(|(key, id)| (*id, key.clone()))
}
