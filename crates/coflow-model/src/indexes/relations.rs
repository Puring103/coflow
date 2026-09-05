use crate::build::BuildSchema;
use crate::diagnostics::CfdPath;
use crate::diagnostics::{CfdDiagnostic, CfdErrorCode};
use crate::model::{
    CfdRecord, CfdRecordId, CfdValue, DimensionRefCoordinate, RefEdge, RefEdgeId, RefSite,
};
use coflow_language::cft::{CftValueType, RecordKey, TypeName};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RefIndexes {
    pub(crate) edges: Vec<RefEdge>,
    pub(crate) by_site: BTreeMap<RefSite, RefEdgeId>,
    pub(crate) by_host: BTreeMap<CfdRecordId, Vec<RefEdgeId>>,
    pub(crate) by_target: BTreeMap<CfdRecordId, Vec<RefEdgeId>>,
}

pub(crate) fn build_ref_indexes(
    records: &[CfdRecord],
    record_by_domain_key: &BTreeMap<TypeName, BTreeMap<RecordKey, CfdRecordId>>,
    schema: BuildSchema<'_>,
) -> RefIndexes {
    let mut out = RefIndexes::default();
    let context = RefEdgeBuildContext {
        record_by_domain_key,
        schema,
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

pub(crate) fn first_ref_cycle(
    indexes: &RefIndexes,
    records: &[CfdRecord],
) -> Option<CfdDiagnostic> {
    let mut states = vec![0_u8; records.len()];
    let mut record_stack = Vec::new();
    let mut frames = Vec::new();

    for root in 0..records.len() {
        if states[root] != 0 {
            continue;
        }
        let root = CfdRecordId::new(root);
        states[root.index()] = 1;
        record_stack.push(root);
        frames.push((root, 0_usize));

        while let Some((host, next_edge)) = frames.last_mut() {
            let outgoing = indexes.by_host.get(host).map_or(&[][..], Vec::as_slice);
            let Some(edge_id) = outgoing.get(*next_edge).copied() else {
                states[host.index()] = 2;
                frames.pop();
                record_stack.pop();
                continue;
            };
            *next_edge += 1;
            let edge = &indexes.edges[edge_id.index()];
            match states.get(edge.target.index()).copied().unwrap_or(2) {
                0 => {
                    states[edge.target.index()] = 1;
                    record_stack.push(edge.target);
                    frames.push((edge.target, 0));
                }
                1 => {
                    let cycle_start = record_stack
                        .iter()
                        .position(|record| *record == edge.target)
                        .unwrap_or(0);
                    let mut cycle = record_stack[cycle_start..]
                        .iter()
                        .filter_map(|record| display_record(records, *record))
                        .collect::<Vec<_>>();
                    if let Some(first) = cycle.first().cloned() {
                        cycle.push(first);
                    }
                    let mut diagnostic = CfdDiagnostic::error(
                            CfdErrorCode::RefCycle,
                            format!("record reference cycle: {}", cycle.join(" -> ")),
                        )
                        .with_primary(Some(edge.site.host), edge.site.path.clone())
                        .with_primary_message("this reference closes the cycle");
                    if let Some(dimension) = &edge.site.dimension {
                        if let Some(origin) = records
                            .get(edge.site.host.index())
                            .and_then(|record| record.dimension_field(dimension.field.as_str()))
                            .and_then(|values| values.variants.get(&dimension.variant))
                            .map(|value| value.origin.clone())
                        {
                            diagnostic = diagnostic.with_primary_origin(origin);
                        }
                    }
                    return Some(diagnostic);
                }
                _ => {}
            }
        }
    }
    None
}

fn display_record(records: &[CfdRecord], record: CfdRecordId) -> Option<String> {
    records
        .get(record.index())
        .map(|record| format!("{}:{}", record.actual_type(), record.key()))
}

struct RefEdgeBuildContext<'a, 'schema> {
    record_by_domain_key: &'a BTreeMap<TypeName, BTreeMap<RecordKey, CfdRecordId>>,
    schema: BuildSchema<'schema>,
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
    match (value, ty) {
        (CfdValue::OptionSome(value), CftValueType::Option(inner)) => collect_ref_edges(
            value, inner, host, path, dimension, context, out,
        ),
        (CfdValue::ResultOk(value), CftValueType::Result(ok, _)) => collect_ref_edges(
            value, ok, host, path, dimension, context, out,
        ),
        (CfdValue::ResultErr(value), CftValueType::Result(_, error)) => collect_ref_edges(
            value, error, host, path, dimension, context, out,
        ),
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
