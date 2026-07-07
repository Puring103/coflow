use crate::diagnostic::CfdPath;
use crate::model::{
    CfdDomainId, CfdRecord, CfdRecordId, CfdValue, RefEdge, RefEdgeId, RefSite, SpreadEdge,
    SpreadEdgeId, SpreadSite,
};
use crate::schema_view::{CfdValueDraft, RecordDraft, SchemaView};
use coflow_cft::CftSchemaTypeRef;
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
    pub(crate) by_source: BTreeMap<CfdRecordId, Vec<SpreadEdgeId>>,
}

pub(crate) fn build_spread_indexes(
    drafts: &[RecordDraft],
    record_by_domain_key: &BTreeMap<(CfdDomainId, String), CfdRecordId>,
    schema: &SchemaView,
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
    record_by_domain_key: &BTreeMap<(CfdDomainId, String), CfdRecordId>,
    schema: &SchemaView,
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
        let Some(expected_type) = schema.type_id(&source.expected_type) else {
            continue;
        };
        let Some(domain) = schema.type_domain_id(&source.expected_type) else {
            continue;
        };
        let Some(source_id) = lookup_domain_ref(
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
        let Some(source_type) = schema.type_id(&source_draft.actual_type) else {
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
            domain,
            source_key: source.key,
            source: source_id,
            source_type,
        });
        out.by_site.entry(site).or_default().push(id);
        out.by_source.entry(source_id).or_default().push(id);
    }

    for (field, value) in &draft.fields {
        collect_nested_spread_edges(
            value,
            host,
            &path.clone().field(field.clone()),
            drafts,
            record_by_domain_key,
            schema,
            out,
        );
    }
}

fn collect_nested_spread_edges(
    value: &CfdValueDraft,
    host: CfdRecordId,
    path: &CfdPath,
    drafts: &[RecordDraft],
    record_by_domain_key: &BTreeMap<(CfdDomainId, String), CfdRecordId>,
    schema: &SchemaView,
    out: &mut SpreadIndexes,
) {
    match value {
        CfdValueDraft::Object(draft) => {
            collect_spread_edges(draft, host, path, drafts, record_by_domain_key, schema, out);
        }
        CfdValueDraft::Array(items) => {
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
        CfdValueDraft::Dict(entries) => {
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
        CfdValueDraft::DictSpread { spreads, entries } => {
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
        CfdValueDraft::Value(_)
        | CfdValueDraft::PendingRef { .. }
        | CfdValueDraft::PendingSpreadField { .. } => {}
    }
}

pub(crate) fn build_ref_indexes(
    records: &[CfdRecord],
    record_by_domain_key: &BTreeMap<(CfdDomainId, String), CfdRecordId>,
    schema: &SchemaView,
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
                .iter()
                .find(|field| field.name == *name)
            else {
                continue;
            };
            collect_ref_edges(
                value,
                &field.ty_ref,
                host,
                root.clone().field(name.clone()),
                &context,
                &mut out,
            );
        }
    }
    out
}

struct RefEdgeBuildContext<'a> {
    records: &'a [CfdRecord],
    record_by_domain_key: &'a BTreeMap<(CfdDomainId, String), CfdRecordId>,
    schema: &'a SchemaView,
    spread_edges_by_host: BTreeMap<CfdRecordId, Vec<&'a SpreadEdge>>,
}

impl RefEdgeBuildContext<'_> {
    fn is_spread_inherited_path(&self, host: CfdRecordId, path: &CfdPath) -> bool {
        self.spread_edges_by_host
            .get(&host)
            .is_some_and(|edges| edges.iter().any(|edge| edge.covers_path(path)))
    }
}

fn collect_ref_edges(
    value: &CfdValue,
    ty: &CftSchemaTypeRef,
    host: CfdRecordId,
    path: CfdPath,
    context: &RefEdgeBuildContext<'_>,
    out: &mut RefIndexes,
) {
    if context.is_spread_inherited_path(host, &path) {
        return;
    }
    match (value, ty.non_nullable()) {
        (CfdValue::Ref(key), CftSchemaTypeRef::Ref(expected_type)) => {
            let Some(expected_type_id) = context.schema.type_id(expected_type) else {
                return;
            };
            let Some(domain) = context.schema.type_domain_id(expected_type) else {
                return;
            };
            let Some(target) = lookup_domain_ref(
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
            let Some(target_type) = context.schema.type_id(target_record.actual_type()) else {
                return;
            };
            let site = RefSite::new(host, path.clone());
            let id = RefEdgeId::new(out.edges.len());
            out.edges.push(RefEdge {
                id,
                site: site.clone(),
                host,
                path,
                expected_type: expected_type_id,
                domain,
                key: key.clone(),
                target,
                target_type,
            });
            out.by_site.insert(site, id);
            out.by_host.entry(host).or_default().push(id);
            out.by_target.entry(target).or_default().push(id);
        }
        (CfdValue::Object(boxed), CftSchemaTypeRef::Named(_)) => {
            for (name, inner) in &boxed.fields {
                let Some(field) = context
                    .schema
                    .full_fields(&boxed.actual_type)
                    .iter()
                    .find(|field| field.name == *name)
                else {
                    continue;
                };
                collect_ref_edges(
                    inner,
                    &field.ty_ref,
                    host,
                    path.clone().field(name.clone()),
                    context,
                    out,
                );
            }
        }
        (CfdValue::Array(items), CftSchemaTypeRef::Array(inner_ty)) => {
            for (index, item) in items.iter().enumerate() {
                collect_ref_edges(
                    item,
                    inner_ty,
                    host,
                    path.clone().index(index),
                    context,
                    out,
                );
            }
        }
        (CfdValue::Dict(entries), CftSchemaTypeRef::Dict(_, value_ty)) => {
            for (key, item) in entries {
                collect_ref_edges(
                    item,
                    value_ty,
                    host,
                    path.clone().dict_key_value(key),
                    context,
                    out,
                );
            }
        }
        _ => {}
    }
}

fn lookup_domain_ref(
    schema: &SchemaView,
    record_by_domain_key: &BTreeMap<(CfdDomainId, String), CfdRecordId>,
    target_type: &str,
    key: &str,
) -> Option<CfdRecordId> {
    schema
        .type_domain_id(target_type)
        .and_then(|domain_id| record_by_domain_key.get(&(domain_id, key.to_string())))
        .copied()
}
