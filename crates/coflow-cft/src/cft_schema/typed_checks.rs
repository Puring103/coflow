use super::{CftTypeMeta, CftSchema, LocatedBudgetError};
use crate::{CftSchemaCheckBlock, CftSchemaTypeRef, ModuleId};
use coflow_structure::{StructuralBudget, StructureKind, TraversalCursor};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Debug, Clone, Default)]
pub struct TypedCheckPlan {
    owners_by_actual: BTreeMap<String, Vec<String>>,
    nested_fields_by_actual: BTreeMap<String, BTreeSet<String>>,
}

impl TypedCheckPlan {
    pub(super) fn compile(
        types: &BTreeMap<String, CftTypeMeta>,
        budget: &mut StructuralBudget,
    ) -> Result<Self, LocatedBudgetError> {
        let mut owners_by_actual = BTreeMap::new();
        for actual_type in types.keys() {
            let mut owners = Vec::new();
            let mut current = Some(actual_type.as_str());
            while let Some(type_name) = current {
                let Some(meta) = types.get(type_name) else {
                    break;
                };
                let depth = u64::try_from(owners.len())
                    .unwrap_or(u64::MAX)
                    .saturating_add(1);
                budget
                    .check_additional_depth(
                        TraversalCursor::root(),
                        StructureKind::SchemaDependency,
                        depth,
                    )
                    .and_then(|()| budget.charge_work(StructureKind::SchemaDependency, 1))
                    .map_err(|error| LocatedBudgetError {
                        error,
                        module: ModuleId::new(meta.module.clone()),
                        span: meta.span,
                    })?;
                owners.push(meta.name.clone());
                current = meta.parent.as_deref();
            }
            owners.reverse();
            owners_by_actual.insert(actual_type.clone(), owners);
        }
        let nested_fields_by_actual = compile_nested_fields(types, &owners_by_actual, budget)?;
        Ok(Self {
            owners_by_actual,
            nested_fields_by_actual,
        })
    }

    pub(super) fn owners(&self, actual_type: &str) -> &[String] {
        self.owners_by_actual
            .get(actual_type)
            .map_or(&[], Vec::as_slice)
    }

    pub(super) fn field_has_nested_checks(&self, actual_type: &str, field_name: &str) -> bool {
        self.nested_fields_by_actual
            .get(actual_type)
            .is_some_and(|fields| fields.contains(field_name))
    }
}

fn compile_nested_fields(
    types: &BTreeMap<String, CftTypeMeta>,
    owners_by_actual: &BTreeMap<String, Vec<String>>,
    budget: &mut StructuralBudget,
) -> Result<BTreeMap<String, BTreeSet<String>>, LocatedBudgetError> {
    let mut assignable_by_target = BTreeMap::<String, BTreeSet<String>>::new();
    for (candidate, meta) in types {
        let mut current = Some(candidate.as_str());
        while let Some(target) = current {
            charge_plan_work(budget, meta)?;
            assignable_by_target
                .entry(target.to_string())
                .or_default()
                .insert(candidate.clone());
            current = types.get(target).and_then(|ty| ty.parent.as_deref());
        }
    }

    let mut reverse_dependencies = BTreeMap::<String, BTreeSet<String>>::new();
    for (owner_name, owner) in types {
        for field in &owner.all_fields {
            let Some(target) = nested_type_target(&field.ty_ref) else {
                continue;
            };
            for candidate in assignable_by_target.get(target).into_iter().flatten() {
                charge_plan_work(budget, owner)?;
                reverse_dependencies
                    .entry(candidate.clone())
                    .or_default()
                    .insert(owner_name.clone());
            }
        }
    }

    let mut types_with_checks = owners_by_actual
        .iter()
        .filter(|(_, owners)| {
            owners
                .iter()
                .any(|owner| types.get(owner).is_some_and(|meta| meta.check.is_some()))
        })
        .map(|(actual_type, _)| actual_type.clone())
        .collect::<BTreeSet<_>>();
    let mut queue = types_with_checks.iter().cloned().collect::<VecDeque<_>>();
    while let Some(contained_type) = queue.pop_front() {
        for owner in reverse_dependencies
            .get(&contained_type)
            .into_iter()
            .flatten()
        {
            if types_with_checks.insert(owner.clone()) {
                queue.push_back(owner.clone());
            }
        }
    }

    let mut fields_by_actual = BTreeMap::new();
    for (actual_type, meta) in types {
        let mut fields = BTreeSet::new();
        for field in &meta.all_fields {
            let Some(target) = nested_type_target(&field.ty_ref) else {
                continue;
            };
            let has_nested_checks = assignable_by_target
                .get(target)
                .is_some_and(|candidates| !candidates.is_disjoint(&types_with_checks));
            if has_nested_checks {
                fields.insert(field.name.clone());
            }
        }
        fields_by_actual.insert(actual_type.clone(), fields);
    }
    Ok(fields_by_actual)
}

fn nested_type_target(ty: &CftSchemaTypeRef) -> Option<&str> {
    match ty {
        CftSchemaTypeRef::Named(name) => Some(name),
        CftSchemaTypeRef::Array(inner) | CftSchemaTypeRef::Nullable(inner) => {
            nested_type_target(inner)
        }
        CftSchemaTypeRef::Dict(_, value) => nested_type_target(value),
        CftSchemaTypeRef::Int
        | CftSchemaTypeRef::Float
        | CftSchemaTypeRef::Bool
        | CftSchemaTypeRef::String
        | CftSchemaTypeRef::Ref(_) => None,
    }
}

fn charge_plan_work(
    budget: &mut StructuralBudget,
    owner: &CftTypeMeta,
) -> Result<(), LocatedBudgetError> {
    budget
        .charge_work(StructureKind::SchemaDependency, 1)
        .map_err(|error| LocatedBudgetError {
            error,
            module: ModuleId::new(owner.module.clone()),
            span: owner.span,
        })
}

#[derive(Debug)]
pub struct TypedCheckSchedule<'schema, 'dimension> {
    schema: &'schema CftSchema,
    owners: std::slice::Iter<'schema, String>,
    dimension: Option<&'dimension str>,
}

impl<'schema, 'dimension> TypedCheckSchedule<'schema, 'dimension> {
    pub(super) fn new(
        schema: &'schema CftSchema,
        actual_type: &str,
        dimension: Option<&'dimension str>,
    ) -> Self {
        Self {
            schema,
            owners: schema.typed_checks.owners(actual_type).iter(),
            dimension,
        }
    }
}

impl<'schema> Iterator for TypedCheckSchedule<'schema, '_> {
    type Item = &'schema CftSchemaCheckBlock;

    fn next(&mut self) -> Option<Self::Item> {
        for owner in self.owners.by_ref() {
            let meta = self.schema.types.get(owner)?;
            if let Some(dimension) = self.dimension {
                if let Some(check) = meta.dimension_checks.get(dimension) {
                    return Some(check);
                }
            } else if let Some(check) = meta.check.as_ref() {
                return Some(check);
            }
        }
        None
    }
}
