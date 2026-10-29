use crate::build::BuildSchema;
use crate::diagnostics::CfdPath;
use crate::indexes::ReferenceFieldPlan;
use crate::model::{
    CfdRecord, CfdRecordHandle, CfdValue, DimensionRefCoordinate, RefEdge, RefEdgeId, RefSite,
};
use coflow_language::cft::{CftValueType, RecordId, TypeName};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct HostRefs {
    edges: Vec<RefEdge>,
    by_site: BTreeMap<RefSite, usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RefIndexes {
    // 引用按宿主分块，反向索引只保存块内地址；局部替换不复制其他记录的路径。
    hosts: coflow_storage::PagedVec<Arc<HostRefs>>,
    incoming: coflow_storage::PagedVec<Arc<Vec<RefEdgeId>>>,
}

impl RefIndexes {
    pub(crate) fn resolve(&self, site: &RefSite) -> Option<CfdRecordHandle> {
        let host = self.hosts.get(site.host.index())?;
        host.edges
            .get(*host.by_site.get(site)?)
            .map(|edge| edge.target)
    }

    pub(crate) fn edges(&self) -> impl Iterator<Item = &RefEdge> {
        self.hosts.iter().flat_map(|host| host.edges.iter())
    }

    pub(crate) fn edges_from_host(&self, host: CfdRecordHandle) -> impl Iterator<Item = &RefEdge> {
        self.hosts
            .get(host.index())
            .into_iter()
            .flat_map(|host| host.edges.iter())
    }

    pub(crate) fn to_target(&self, target: CfdRecordHandle) -> impl Iterator<Item = &RefEdge> {
        self.incoming
            .get(target.index())
            .into_iter()
            .flat_map(|ids| ids.iter())
            .filter_map(|id| self.hosts.get(id.host().index())?.edges.get(id.index()))
    }

    pub(crate) fn replace_hosts<'a>(
        &mut self,
        records: impl IntoIterator<Item = (CfdRecordHandle, &'a CfdRecord)>,
        record_by_domain_id: &BTreeMap<TypeName, BTreeMap<RecordId, CfdRecordHandle>>,
        schema: BuildSchema<'_>,
    ) -> usize {
        let plan = ReferenceFieldPlan::new(schema);
        let context = RefEdgeBuildContext {
            record_by_domain_id,
            schema,
            plan: &plan,
        };
        let mut targets =
            BTreeMap::<CfdRecordHandle, (BTreeSet<CfdRecordHandle>, Vec<RefEdgeId>)>::new();
        let mut visited = 0;
        for (host, record) in records {
            let references = record_refs(record, host, &context);
            visited += references.edges.len();
            // 普通字段变化不改动引用块；旧代次与当前代次继续共享同一邻接表。
            if self.hosts[host.index()].as_ref() == &references {
                continue;
            }
            for edge in &self.hosts[host.index()].edges {
                targets.entry(edge.target).or_default().0.insert(host);
            }
            for (index, edge) in references.edges.iter().enumerate() {
                targets
                    .entry(edge.target)
                    .or_default()
                    .1
                    .push(RefEdgeId::new(host, index));
            }
            self.hosts[host.index()] = Arc::new(references);
        }
        for (target, (removed_hosts, added)) in targets {
            // 按目标合并整批变化，高扇入目标的旧列表只过滤一次。
            let incoming = Arc::make_mut(&mut self.incoming[target.index()]);
            incoming.retain(|id| !removed_hosts.contains(&id.host()));
            incoming.extend(added);
            // 与全量构建的宿主顺序一致，不能让更新顺序改变查询或诊断结果。
            incoming.sort_unstable();
        }
        visited
    }
}

pub(crate) fn build_ref_indexes(
    records: &[CfdRecord],
    record_by_domain_id: &BTreeMap<TypeName, BTreeMap<RecordId, CfdRecordHandle>>,
    schema: BuildSchema<'_>,
    plan: &ReferenceFieldPlan<'_>,
) -> RefIndexes {
    build_ref_indexes_iter(records.iter(), record_by_domain_id, schema, plan)
}

/// 成员变化重建句柄索引时直接借用共享记录，不复制未变化记录的头部和值树。
pub(crate) fn build_ref_indexes_iter<'a>(
    records: impl ExactSizeIterator<Item = &'a CfdRecord>,
    record_by_domain_id: &BTreeMap<TypeName, BTreeMap<RecordId, CfdRecordHandle>>,
    schema: BuildSchema<'_>,
    plan: &ReferenceFieldPlan<'_>,
) -> RefIndexes {
    // 空入边共享同一不可变块，修改时由 Arc 写时复制隔离。
    let empty_incoming = Arc::new(Vec::new());
    let mut out = RefIndexes {
        hosts: coflow_storage::PagedVec::default(),
        incoming: vec![empty_incoming; records.len()].into(),
    };
    let context = RefEdgeBuildContext {
        record_by_domain_id,
        schema,
        plan,
    };
    let empty_host = Arc::new(HostRefs::default());
    for (index, record) in records.enumerate() {
        let host = CfdRecordHandle::new(index);
        let references = record_refs(record, host, &context);
        for (index, edge) in references.edges.iter().enumerate() {
            if let Some(incoming) = out.incoming.get_mut(edge.target.index()) {
                Arc::make_mut(incoming).push(RefEdgeId::new(host, index));
            }
        }
        out.hosts.push(if references.edges.is_empty() {
            Arc::clone(&empty_host)
        } else {
            Arc::new(references)
        });
    }
    out
}

fn record_refs(
    record: &CfdRecord,
    host: CfdRecordHandle,
    context: &RefEdgeBuildContext<'_, '_>,
) -> HostRefs {
    let mut out = HostRefs::default();
    let root = CfdPath::root();
    for field in context.plan.fields(record.actual_type(), true) {
        let Some(value) = record.field(field.name.as_str()) else {
            continue;
        };
        collect_ref_edges(
            value,
            &field.value_type,
            host,
            &root.clone().field(field.name.as_str()),
            None,
            context,
            &mut out,
        );
    }
    for field in context.plan.fields(record.actual_type(), true) {
        let Some(values) = record.dimension_fields.get(&field.name) else {
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
                &root.clone().field(field.name.as_str()),
                Some(&coordinate),
                context,
                &mut out,
            );
        }
    }
    out
}

struct RefEdgeBuildContext<'a, 'schema> {
    record_by_domain_id: &'a BTreeMap<TypeName, BTreeMap<RecordId, CfdRecordHandle>>,
    schema: BuildSchema<'schema>,
    plan: &'a ReferenceFieldPlan<'schema>,
}

fn collect_ref_edges(
    value: &CfdValue,
    ty: &CftValueType,
    host: CfdRecordHandle,
    path: &CfdPath,
    dimension: Option<&DimensionRefCoordinate>,
    context: &RefEdgeBuildContext<'_, '_>,
    out: &mut HostRefs,
) {
    match (value, ty) {
        (CfdValue::OptionSome(value), CftValueType::Option(inner)) => {
            collect_ref_edges(value, inner, host, path, dimension, context, out);
        }
        (CfdValue::Ref(key), CftValueType::RecordRef(expected_type)) => {
            let Some(target) = lookup_domain_ref(
                context.schema,
                context.record_by_domain_id,
                expected_type,
                key,
            ) else {
                return;
            };
            let site = dimension.map_or_else(
                || RefSite::new(host, path.clone()),
                |dimension| RefSite::in_dimension(host, path.clone(), dimension.clone()),
            );
            let id = out.edges.len();
            out.edges.push(RefEdge {
                site: site.clone(),
                target,
            });
            out.by_site.insert(site, id);
        }
        (CfdValue::Object(boxed), CftValueType::Object(_)) => {
            for field in context.plan.fields(boxed.actual_type.as_str(), true) {
                let Some(inner) = boxed.field(field.name.as_str()) else {
                    continue;
                };
                collect_ref_edges(
                    inner,
                    &field.value_type,
                    host,
                    &path.clone().field(field.name.as_str()),
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
    record_by_domain_id: &BTreeMap<TypeName, BTreeMap<RecordId, CfdRecordHandle>>,
    target_type: &str,
    key: &str,
) -> Option<CfdRecordHandle> {
    schema
        .inheritance_root(target_type)
        .and_then(|inheritance_root| record_by_domain_id.get(inheritance_root))
        .and_then(|records| records.get(key))
        .copied()
}
