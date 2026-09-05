use crate::{
    CftDiagnostic, CftErrorCode, CftSchemaDefaultValue, CftType, CftValueType, FieldName, TypeName,
};
use std::collections::{BTreeMap, BTreeSet};

type FieldKey = (TypeName, FieldName);

#[derive(Clone)]
struct Dependency {
    target: FieldKey,
}

/// 默认表达式只连接实际需要继续读取的字段默认值，显式提供的字段不会误报递归。
pub(crate) fn validate_default_materialization(
    types: &BTreeMap<TypeName, CftType>,
) -> Vec<CftDiagnostic> {
    let fields = types
        .values()
        .flat_map(|ty| ty.own_fields.iter())
        .map(|field| {
            (
                (field.declaring_type.clone(), field.name.clone()),
                field.as_ref(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let graph = fields
        .iter()
        .filter_map(|(key, field)| {
            let default = field.default.as_ref()?;
            let mut dependencies = Vec::new();
            collect_dependencies(&field.value_type, default, types, &mut dependencies);
            Some((key.clone(), dependencies))
        })
        .collect::<BTreeMap<_, _>>();

    let mut diagnostics = Vec::new();
    let mut complete = BTreeSet::new();
    for root in graph.keys() {
        if complete.contains(root) {
            continue;
        }
        let mut positions = BTreeMap::new();
        let mut nodes = vec![root.clone()];
        let mut frames = vec![(root.clone(), 0usize)];
        positions.insert(root.clone(), 0usize);

        while let Some((node, next_edge)) = frames.last_mut() {
            let edges = graph.get(node).map_or(&[][..], Vec::as_slice);
            let Some(edge) = edges.get(*next_edge).cloned() else {
                let completed = node.clone();
                frames.pop();
                positions.remove(&completed);
                nodes.pop();
                complete.insert(completed);
                continue;
            };
            *next_edge += 1;
            if complete.contains(&edge.target) || !graph.contains_key(&edge.target) {
                continue;
            }
            if let Some(start) = positions.get(&edge.target).copied() {
                let mut path = nodes[start..].iter().map(field_label).collect::<Vec<_>>();
                path.push(field_label(&edge.target));
                let source = fields[node];
                let module = types[&source.declaring_type].module.clone();
                diagnostics.push(CftDiagnostic::error(
                    CftErrorCode::DefaultMaterializationCycle,
                    module,
                    source.span,
                    format!(
                        "default value materialization cycle: {}",
                        path.join(" -> ")
                    ),
                ));
                continue;
            }
            positions.insert(edge.target.clone(), nodes.len());
            nodes.push(edge.target.clone());
            frames.push((edge.target, 0));
        }
    }
    diagnostics
}

fn collect_dependencies(
    ty: &CftValueType,
    value: &CftSchemaDefaultValue,
    types: &BTreeMap<TypeName, CftType>,
    out: &mut Vec<Dependency>,
) {
    match (ty, value) {
        (CftValueType::Option(inner), CftSchemaDefaultValue::OptionSome(value)) => {
            collect_dependencies(inner, value, types, out);
        }
        (CftValueType::Array(inner), CftSchemaDefaultValue::Array(values)) => {
            for value in values {
                collect_dependencies(inner, value, types, out);
            }
        }
        (
            CftValueType::Dict(key_type, value_type),
            CftSchemaDefaultValue::Dictionary(entries),
        ) => {
            for (key, value) in entries {
                collect_dependencies(key_type, key, types, out);
                collect_dependencies(value_type, value, types, out);
            }
        }
        (CftValueType::Object(expected), CftSchemaDefaultValue::EmptyObject) => {
            collect_missing_fields(expected, &BTreeSet::new(), types, out);
        }
        (
            CftValueType::Object(expected),
            CftSchemaDefaultValue::Object { type_name, fields },
        ) if types.contains_key(type_name) && is_assignable(types, type_name, expected) => {
            let supplied = fields
                .iter()
                .map(|(name, _)| name.clone())
                .collect::<BTreeSet<_>>();
            if let Some(meta) = types.get(type_name) {
                for (name, value) in fields {
                    if let Some(field) = meta.all_fields.iter().find(|field| field.name == *name) {
                        collect_dependencies(&field.value_type, value, types, out);
                    }
                }
            }
            collect_missing_fields(type_name, &supplied, types, out);
        }
        _ => {}
    }
}

fn collect_missing_fields(
    type_name: &TypeName,
    supplied: &BTreeSet<FieldName>,
    types: &BTreeMap<TypeName, CftType>,
    out: &mut Vec<Dependency>,
) {
    let Some(meta) = types.get(type_name) else {
        return;
    };
    for field in &meta.all_fields {
        if !supplied.contains(&field.name) && field.default.is_some() {
            out.push(Dependency {
                target: (field.declaring_type.clone(), field.name.clone()),
            });
        }
    }
}

fn is_assignable(
    types: &BTreeMap<TypeName, CftType>,
    actual: &TypeName,
    expected: &TypeName,
) -> bool {
    let mut current = Some(actual);
    while let Some(name) = current {
        if name == expected {
            return true;
        }
        current = types.get(name).and_then(|ty| ty.parent.as_ref());
    }
    false
}

fn field_label((owner, field): &FieldKey) -> String {
    format!("{owner}.{field}")
}
