use super::dimension_checks;
use crate::schema::{CftSchema, LocatedBudgetError};
use crate::{CftSchemaCheckBlock, CftType, CftValueType, DimensionName, FieldName, TypeName};
use coflow_structure::{StructuralBudget, StructureKind, TraversalCursor};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Debug, Clone, Default)]
pub struct TypedCheckPlan {
    owners_by_actual: BTreeMap<TypeName, Vec<TypeName>>,
    nested_fields_by_actual: BTreeMap<TypeName, BTreeSet<FieldName>>,
    dimension_statements_by_owner: BTreeMap<TypeName, BTreeMap<DimensionName, Vec<usize>>>,
}

impl TypedCheckPlan {
    pub(in crate::schema) fn compile(
        types: &BTreeMap<TypeName, CftType>,
        budget: &mut StructuralBudget,
    ) -> Result<Self, LocatedBudgetError> {
        let mut owners_by_actual = BTreeMap::new();
        for actual_type in types.keys() {
            let mut owners = Vec::new();
            let mut current = Some(actual_type);
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
                        module: meta.module.clone(),
                        span: meta.span,
                    })?;
                owners.push(meta.name.clone());
                current = meta.parent.as_ref();
            }
            owners.reverse();
            owners_by_actual.insert(actual_type.clone(), owners);
        }
        let nested_fields_by_actual = compile_nested_fields(types, &owners_by_actual, budget)?;
        let mut dimension_statements_by_owner = BTreeMap::new();
        for name in types.keys() {
            dimension_statements_by_owner.insert(
                name.clone(),
                dimension_checks::dimension_checks_for_type(types, name, budget)?,
            );
        }
        Ok(Self {
            owners_by_actual,
            nested_fields_by_actual,
            dimension_statements_by_owner,
        })
    }

    pub(super) fn owners(&self, actual_type: &str) -> &[TypeName] {
        self.owners_by_actual
            .get(actual_type)
            .map_or(&[], Vec::as_slice)
    }

    pub(in crate::schema) fn field_has_nested_checks(
        &self,
        actual_type: &str,
        field_name: &str,
    ) -> bool {
        self.nested_fields_by_actual
            .get(actual_type)
            .is_some_and(|fields| fields.contains(field_name))
    }

    fn dimension_statement_indices(&self, owner: &TypeName, dimension: &str) -> Option<&[usize]> {
        self.dimension_statements_by_owner
            .get(owner)?
            .get(dimension)
            .map(Vec::as_slice)
    }
}

fn compile_nested_fields(
    types: &BTreeMap<TypeName, CftType>,
    owners_by_actual: &BTreeMap<TypeName, Vec<TypeName>>,
    budget: &mut StructuralBudget,
) -> Result<BTreeMap<TypeName, BTreeSet<FieldName>>, LocatedBudgetError> {
    let mut assignable_by_target = BTreeMap::<TypeName, BTreeSet<TypeName>>::new();
    for (candidate, meta) in types {
        let mut current = Some(candidate);
        while let Some(target) = current {
            charge_plan_work(budget, meta)?;
            assignable_by_target
                .entry(target.clone())
                .or_default()
                .insert(candidate.clone());
            current = types.get(target).and_then(|ty| ty.parent.as_ref());
        }
    }

    let mut reverse_dependencies = BTreeMap::<TypeName, BTreeSet<TypeName>>::new();
    for (owner_name, owner) in types {
        for field in &owner.all_fields {
            let Some(target) = nested_type_target(&field.value_type) else {
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
            let Some(target) = nested_type_target(&field.value_type) else {
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

fn nested_type_target(ty: &CftValueType) -> Option<&TypeName> {
    match ty {
        CftValueType::Object(name) => Some(name),
        CftValueType::Array(inner) | CftValueType::Nullable(inner) => nested_type_target(inner),
        CftValueType::Dict(_, value) => nested_type_target(value),
        CftValueType::Int
        | CftValueType::Float
        | CftValueType::Bool
        | CftValueType::String
        | CftValueType::Enum(_)
        | CftValueType::RecordRef(_) => None,
    }
}

fn charge_plan_work(
    budget: &mut StructuralBudget,
    owner: &CftType,
) -> Result<(), LocatedBudgetError> {
    budget
        .charge_work(StructureKind::SchemaDependency, 1)
        .map_err(|error| LocatedBudgetError {
            error,
            module: owner.module.clone(),
            span: owner.span,
        })
}

#[derive(Debug)]
pub struct TypedCheckSchedule<'schema, 'dimension> {
    schema: &'schema CftSchema,
    owners: std::slice::Iter<'schema, TypeName>,
    dimension: Option<&'dimension str>,
}

#[derive(Debug, Clone, Copy)]
pub struct ScheduledCheckBlock<'schema> {
    block: &'schema CftSchemaCheckBlock,
    statement_indices: Option<&'schema [usize]>,
}

impl<'schema> ScheduledCheckBlock<'schema> {
    #[must_use]
    pub const fn block(&self) -> &'schema CftSchemaCheckBlock {
        self.block
    }

    #[must_use]
    pub const fn statement_indices(&self) -> Option<&'schema [usize]> {
        self.statement_indices
    }
}

impl<'schema, 'dimension> TypedCheckSchedule<'schema, 'dimension> {
    pub(in crate::schema) fn new(
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
    type Item = ScheduledCheckBlock<'schema>;

    fn next(&mut self) -> Option<Self::Item> {
        for owner in self.owners.by_ref() {
            let meta = self.schema.types.get(owner)?;
            if let Some(dimension) = self.dimension {
                if let (Some(block), Some(statement_indices)) = (
                    meta.check.as_ref(),
                    self.schema
                        .typed_checks
                        .dimension_statement_indices(owner, dimension),
                ) {
                    return Some(ScheduledCheckBlock {
                        block,
                        statement_indices: Some(statement_indices),
                    });
                }
            } else if let Some(check) = meta.check.as_ref() {
                return Some(ScheduledCheckBlock {
                    block: check,
                    statement_indices: None,
                });
            }
        }
        None
    }
}
