use super::CftTypeMeta;
use crate::{CftSchemaDefaultValue, CftSchemaTypeRef};
use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ValueDependencyMode {
    SchemaDefaults,
    Minimal,
    EditableShape,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueDependencyStep {
    pub owner_type: String,
    pub field: String,
    pub target_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueDependencyCycle {
    steps: Vec<ValueDependencyStep>,
}

impl ValueDependencyCycle {
    fn canonical(mut steps: Vec<ValueDependencyStep>) -> Self {
        if let Some(start) = steps
            .iter()
            .enumerate()
            .min_by_key(|(_, step)| (&step.owner_type, &step.field, &step.target_type))
            .map(|(index, _)| index)
        {
            steps.rotate_left(start);
        }
        Self { steps }
    }

    #[must_use]
    pub fn steps(&self) -> &[ValueDependencyStep] {
        &self.steps
    }
}

impl fmt::Display for ValueDependencyCycle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Some(first) = self.steps.first() else {
            return f.write_str("unknown value dependency cycle");
        };
        write!(f, "{}.{}", first.owner_type, first.field)?;
        for step in self.steps.iter().skip(1) {
            write!(f, " -> {}.{}", step.owner_type, step.field)?;
        }
        write!(
            f,
            " -> {}",
            self.steps.last().map_or("?", |step| &step.target_type)
        )
    }
}

#[derive(Debug, Clone)]
pub struct ValueDependencyPlan {
    graphs: BTreeMap<ValueDependencyMode, BTreeMap<String, Vec<ValueDependencyStep>>>,
}

impl ValueDependencyPlan {
    pub(super) fn compile(types: &BTreeMap<String, CftTypeMeta>) -> Self {
        let mut graphs = BTreeMap::new();
        for mode in [
            ValueDependencyMode::SchemaDefaults,
            ValueDependencyMode::Minimal,
            ValueDependencyMode::EditableShape,
        ] {
            graphs.insert(mode, dependency_graph(types, mode));
        }
        Self { graphs }
    }

    #[must_use]
    pub fn materialization_order<'a>(
        &'a self,
        type_name: &str,
        mode: ValueDependencyMode,
    ) -> Option<Result<Vec<&'a str>, ValueDependencyCycle>> {
        let graph = self.graphs.get(&mode)?;
        let (root, _) = graph.get_key_value(type_name)?;
        Some(compile_root(root, graph))
    }
}

fn dependency_graph(
    types: &BTreeMap<String, CftTypeMeta>,
    mode: ValueDependencyMode,
) -> BTreeMap<String, Vec<ValueDependencyStep>> {
    types
        .iter()
        .map(|(type_name, meta)| {
            let dependencies = meta
                .all_fields
                .iter()
                .filter_map(|field| {
                    let target_type = dependency_target(field, mode, types)?;
                    Some(ValueDependencyStep {
                        owner_type: type_name.clone(),
                        field: field.name.clone(),
                        target_type: target_type.to_string(),
                    })
                })
                .collect();
            (type_name.clone(), dependencies)
        })
        .collect()
}

fn dependency_target<'a>(
    field: &'a super::CftFieldMeta,
    mode: ValueDependencyMode,
    types: &BTreeMap<String, CftTypeMeta>,
) -> Option<&'a str> {
    let ty = match mode {
        ValueDependencyMode::SchemaDefaults => {
            matches!(field.default, Some(CftSchemaDefaultValue::EmptyObject))
                .then_some(non_nullable(&field.ty_ref))?
        }
        ValueDependencyMode::Minimal => {
            if field.default.is_some() {
                return None;
            }
            &field.ty_ref
        }
        ValueDependencyMode::EditableShape => match field.default {
            Some(CftSchemaDefaultValue::EmptyObject) => non_nullable(&field.ty_ref),
            Some(_) => return None,
            None => &field.ty_ref,
        },
    };
    let CftSchemaTypeRef::Named(target_type) = ty else {
        return None;
    };
    types.contains_key(target_type).then_some(target_type)
}

fn non_nullable(ty: &CftSchemaTypeRef) -> &CftSchemaTypeRef {
    match ty {
        CftSchemaTypeRef::Nullable(inner) => non_nullable(inner),
        other => other,
    }
}

fn compile_root<'a>(
    root: &'a str,
    graph: &'a BTreeMap<String, Vec<ValueDependencyStep>>,
) -> Result<Vec<&'a str>, ValueDependencyCycle> {
    let mut states = BTreeMap::<&str, VisitState>::new();
    let mut nodes = Vec::new();
    let mut incoming = Vec::new();
    let mut order = Vec::new();
    visit(
        root,
        graph,
        &mut states,
        &mut nodes,
        &mut incoming,
        &mut order,
    )?;
    Ok(order)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VisitState {
    Visiting,
    Complete,
}

fn visit<'a>(
    type_name: &'a str,
    graph: &'a BTreeMap<String, Vec<ValueDependencyStep>>,
    states: &mut BTreeMap<&'a str, VisitState>,
    nodes: &mut Vec<&'a str>,
    incoming: &mut Vec<ValueDependencyStep>,
    order: &mut Vec<&'a str>,
) -> Result<(), ValueDependencyCycle> {
    if states.get(type_name) == Some(&VisitState::Complete) {
        return Ok(());
    }
    states.insert(type_name, VisitState::Visiting);
    nodes.push(type_name);

    for edge in graph.get(type_name).into_iter().flatten() {
        match states.get(edge.target_type.as_str()) {
            Some(VisitState::Visiting) => {
                let cycle_start = nodes
                    .iter()
                    .position(|node| *node == edge.target_type)
                    .unwrap_or(0);
                let mut steps = incoming[cycle_start..].to_vec();
                steps.push(edge.clone());
                return Err(ValueDependencyCycle::canonical(steps));
            }
            Some(VisitState::Complete) => {}
            None => {
                incoming.push(edge.clone());
                visit(
                    edge.target_type.as_str(),
                    graph,
                    states,
                    nodes,
                    incoming,
                    order,
                )?;
                incoming.pop();
            }
        }
    }

    nodes.pop();
    states.insert(type_name, VisitState::Complete);
    order.push(type_name);
    Ok(())
}
