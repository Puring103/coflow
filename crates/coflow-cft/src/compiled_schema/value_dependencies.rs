use super::{CftTypeMeta, LocatedBudgetError};
use crate::{CftSchemaDefaultValue, CftSchemaTypeRef, ModuleId, Span};
use coflow_structure::{StructuralBudget, StructureKind, TraversalCursor};
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
    module: ModuleId,
    span: Span,
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

#[derive(Debug, Clone, Default)]
pub struct ValueDependencyPlan {
    roots: BTreeMap<
        ValueDependencyMode,
        BTreeMap<String, Result<Vec<String>, ValueDependencyCycle>>,
    >,
}

impl ValueDependencyPlan {
    pub(super) fn compile(
        types: &BTreeMap<String, CftTypeMeta>,
        budget: &mut StructuralBudget,
    ) -> Result<Self, LocatedBudgetError> {
        let mut roots = BTreeMap::new();
        for mode in [
            ValueDependencyMode::SchemaDefaults,
            ValueDependencyMode::Minimal,
            ValueDependencyMode::EditableShape,
        ] {
            let graph = dependency_graph(types, mode, budget)?;
            let compiled = graph
                .keys()
                .map(|root| {
                    compile_root(root, &graph, budget).map(|result| {
                        let result = result.map(|order| {
                            order.into_iter().map(str::to_string).collect::<Vec<_>>()
                        });
                        (root.clone(), result)
                    })
                })
                .collect::<Result<BTreeMap<_, _>, _>>()?;
            roots.insert(mode, compiled);
        }
        Ok(Self { roots })
    }

    #[must_use]
    pub fn materialization_order<'a>(
        &'a self,
        type_name: &str,
        mode: ValueDependencyMode,
    ) -> Option<Result<Vec<&'a str>, ValueDependencyCycle>> {
        let result = self.roots.get(&mode)?.get(type_name)?;
        Some(match result {
            Ok(order) => Ok(order.iter().map(String::as_str).collect()),
            Err(cycle) => Err(cycle.clone()),
        })
    }
}

fn dependency_graph(
    types: &BTreeMap<String, CftTypeMeta>,
    mode: ValueDependencyMode,
    budget: &mut StructuralBudget,
) -> Result<BTreeMap<String, Vec<ValueDependencyStep>>, LocatedBudgetError> {
    let mut graph = BTreeMap::new();
    for (type_name, meta) in types {
        let mut dependencies = Vec::new();
        for field in &meta.all_fields {
            budget
                .charge_work(StructureKind::SchemaDependency, 1)
                .map_err(|error| LocatedBudgetError {
                    error,
                    module: ModuleId::new(field.module.clone()),
                    span: field.span,
                })?;
            let Some(target_type) = dependency_target(field, mode, types) else {
                continue;
            };
            dependencies.push(ValueDependencyStep {
                owner_type: type_name.clone(),
                field: field.name.clone(),
                target_type: target_type.to_string(),
                module: ModuleId::new(field.module.clone()),
                span: field.span,
            });
        }
        graph.insert(type_name.clone(), dependencies);
    }
    Ok(graph)
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
    budget: &mut StructuralBudget,
) -> Result<Result<Vec<&'a str>, ValueDependencyCycle>, LocatedBudgetError> {
    let mut states = BTreeMap::<&str, VisitState>::new();
    let mut nodes = Vec::new();
    let mut incoming = Vec::new();
    let mut order = Vec::new();
    let mut stack = vec![VisitFrame {
        type_name: root,
        next_edge: 0,
    }];
    states.insert(root, VisitState::Visiting);
    nodes.push(root);

    while let Some(frame) = stack.last_mut() {
        let edges = graph.get(frame.type_name).map_or(&[][..], Vec::as_slice);
        if let Some(edge) = edges.get(frame.next_edge) {
            frame.next_edge += 1;
            match states.get(edge.target_type.as_str()) {
                Some(VisitState::Visiting) => {
                    let cycle_start = nodes
                        .iter()
                        .position(|node| *node == edge.target_type)
                        .unwrap_or(0);
                    let mut steps = incoming[cycle_start..].to_vec();
                    steps.push(edge.clone());
                    return Ok(Err(ValueDependencyCycle::canonical(steps)));
                }
                Some(VisitState::Complete) => {
                    charge_edge(budget, edge)?;
                }
                None => {
                    charge_edge(budget, edge)?;
                    let depth = u64::try_from(nodes.len())
                        .unwrap_or(u64::MAX)
                        .saturating_add(1);
                    budget
                        .check_additional_depth(
                            TraversalCursor::root(),
                            StructureKind::SchemaDependency,
                            depth,
                        )
                        .map_err(|error| LocatedBudgetError {
                            error,
                            module: edge.module.clone(),
                            span: edge.span,
                        })?;
                    let target = graph
                        .get_key_value(edge.target_type.as_str())
                        .map_or(edge.target_type.as_str(), |(name, _)| name.as_str());
                    incoming.push(edge.clone());
                    states.insert(target, VisitState::Visiting);
                    nodes.push(target);
                    stack.push(VisitFrame {
                        type_name: target,
                        next_edge: 0,
                    });
                }
            }
            continue;
        }

        let completed = frame.type_name;
        stack.pop();
        nodes.pop();
        if !stack.is_empty() {
            incoming.pop();
        }
        states.insert(completed, VisitState::Complete);
        order.push(completed);
    }
    Ok(Ok(order))
}

fn charge_edge(
    budget: &mut StructuralBudget,
    edge: &ValueDependencyStep,
) -> Result<(), LocatedBudgetError> {
    budget
        .charge_work(StructureKind::SchemaDependency, 1)
        .map_err(|error| LocatedBudgetError {
            error,
            module: edge.module.clone(),
            span: edge.span,
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VisitState {
    Visiting,
    Complete,
}

struct VisitFrame<'a> {
    type_name: &'a str,
    next_edge: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iterative_root_compilation_handles_a_ten_thousand_node_chain() {
        const NODE_COUNT: usize = 10_000;
        let mut graph = BTreeMap::new();
        for index in 0..NODE_COUNT {
            let owner = format!("T{index}");
            let edges = (index + 1 < NODE_COUNT)
                .then(|| {
                    vec![ValueDependencyStep {
                        owner_type: owner.clone(),
                        field: "next".to_string(),
                        target_type: format!("T{}", index + 1),
                        module: ModuleId::from("test"),
                        span: Span::new(index, index + 1),
                    }]
                })
                .unwrap_or_default();
            graph.insert(owner, edges);
        }

        let mut budget = StructuralBudget::new(coflow_structure::StructuralLimits::new(
            NODE_COUNT as u64,
            1,
            NODE_COUNT as u64,
        ));
        let order = compile_root("T0", &graph, &mut budget)
            .expect("within budget")
            .expect("acyclic chain");
        assert_eq!(order.len(), NODE_COUNT);
        assert_eq!(order.first().copied(), Some("T9999"));
        assert_eq!(order.last().copied(), Some("T0"));
    }
}
