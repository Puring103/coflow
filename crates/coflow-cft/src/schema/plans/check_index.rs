use crate::schema::{
    CheckDependency, CheckOwner, CheckStatementId, CheckStatementInfo, LocatedBudgetError,
};
use crate::{CftSchemaCheckStmt, CftTopLevelCheck, CftType, CftValueType, FieldName, TypeName};
use coflow_structure::{StructuralBudget, StructureKind, TraversalCursor};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Debug, Clone)]
pub struct CheckStatementRef<'schema> {
    pub info: &'schema CheckStatementInfo,
    pub statement: &'schema CftSchemaCheckStmt,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct CheckIndex {
    statements: Vec<CheckStatementInfo>,
    by_dependency: BTreeMap<CheckDependency, BTreeSet<CheckStatementId>>,
    cross_record_dependencies: BTreeSet<(CheckStatementId, CheckDependency)>,
    by_owner: BTreeMap<CheckOwner, Vec<CheckStatementId>>,
    by_actual_type: BTreeMap<TypeName, BTreeSet<CheckStatementId>>,
    owners_by_actual_type: BTreeMap<TypeName, Vec<TypeName>>,
    nested_hosts_by_type: BTreeMap<TypeName, BTreeSet<TypeName>>,
    nested_fields_by_actual: BTreeMap<TypeName, BTreeSet<FieldName>>,
    nested_statements_by_actual_field: BTreeMap<(TypeName, FieldName), BTreeSet<CheckStatementId>>,
}

impl CheckIndex {
    pub(in crate::schema) fn compile(
        types: &BTreeMap<TypeName, CftType>,
        project_checks: &BTreeMap<crate::CheckName, CftTopLevelCheck>,
        budget: &mut StructuralBudget,
    ) -> Result<Self, LocatedBudgetError> {
        let owners_by_actual_type = compile_owner_chains(types, budget)?;
        let (nested_hosts_by_type, nested_types_by_actual_field) =
            compile_nested_hosts(types, &owners_by_actual_type, budget)?;

        let mut roots = Vec::new();
        for ty in types.values() {
            if let Some(block) = &ty.check {
                for root_index in 0..block.stmts.len() {
                    roots.push((
                        ty.module.clone(),
                        ty.span.start,
                        CheckOwner::Type(ty.name.clone()),
                        root_index,
                    ));
                }
            }
        }
        for check in project_checks.values() {
            for root_index in 0..check.block.stmts.len() {
                roots.push((
                    check.module.clone(),
                    check.span.start,
                    CheckOwner::Project(check.name.clone()),
                    root_index,
                ));
            }
        }
        roots.sort();

        let mut statements = Vec::with_capacity(roots.len());
        let mut by_dependency = BTreeMap::<CheckDependency, BTreeSet<CheckStatementId>>::new();
        let mut cross_record_dependencies = BTreeSet::new();
        let mut by_owner = BTreeMap::<CheckOwner, Vec<CheckStatementId>>::new();
        for (_, _, owner, root_index) in roots {
            let id = CheckStatementId::new(u32::try_from(statements.len()).unwrap_or(u32::MAX));
            let block = match &owner {
                CheckOwner::Type(name) => types.get(name).and_then(|ty| ty.check.as_ref()),
                CheckOwner::Project(name) => project_checks.get(name).map(|check| &check.block),
            };
            let dependencies = block
                .and_then(|block| block.statement_dependencies.get(root_index))
                .cloned()
                .unwrap_or_default();
            let dimensions = block
                .map(|block| {
                    block
                        .dimension_statements
                        .iter()
                        .filter(|(_, indices)| indices.contains(&root_index))
                        .map(|(dimension, _)| dimension.clone())
                        .collect()
                })
                .unwrap_or_default();
            let info = CheckStatementInfo {
                id,
                owner: owner.clone(),
                root_index,
                dependencies,
                dimensions,
            };
            for dependency in &info.dependencies {
                by_dependency
                    .entry(dependency.clone())
                    .or_default()
                    .insert(id);
            }
            if let Some(dependencies) =
                block.and_then(|block| block.statement_cross_record_dependencies.get(root_index))
            {
                cross_record_dependencies.extend(
                    dependencies
                        .iter()
                        .cloned()
                        .map(|dependency| (id, dependency)),
                );
            }
            by_owner.entry(owner).or_default().push(id);
            statements.push(info);
        }

        let mut by_actual_type = BTreeMap::new();
        for actual_type in types.keys() {
            let mut ids = BTreeSet::new();
            for contained_type in std::iter::once(actual_type).chain(
                nested_hosts_by_type
                    .iter()
                    .filter(|(_, hosts)| hosts.contains(actual_type))
                    .map(|(nested, _)| nested),
            ) {
                for owner in owners_by_actual_type
                    .get(contained_type)
                    .into_iter()
                    .flatten()
                {
                    ids.extend(
                        by_owner
                            .get(&CheckOwner::Type(owner.clone()))
                            .into_iter()
                            .flatten()
                            .copied(),
                    );
                }
            }
            by_actual_type.insert(actual_type.clone(), ids);
        }

        let mut nested_statements_by_actual_field = BTreeMap::new();
        for (key, nested_types) in nested_types_by_actual_field {
            let mut ids = BTreeSet::new();
            for nested_type in nested_types {
                for owner in owners_by_actual_type
                    .get(&nested_type)
                    .into_iter()
                    .flatten()
                {
                    ids.extend(
                        by_owner
                            .get(&CheckOwner::Type(owner.clone()))
                            .into_iter()
                            .flatten()
                            .copied(),
                    );
                }
            }
            if !ids.is_empty() {
                nested_statements_by_actual_field.insert(key, ids);
            }
        }
        let mut nested_fields_by_actual = BTreeMap::<TypeName, BTreeSet<FieldName>>::new();
        for (actual_type, field) in nested_statements_by_actual_field.keys() {
            nested_fields_by_actual
                .entry(actual_type.clone())
                .or_default()
                .insert(field.clone());
        }

        Ok(Self {
            statements,
            by_dependency,
            cross_record_dependencies,
            by_owner,
            by_actual_type,
            owners_by_actual_type,
            nested_hosts_by_type,
            nested_fields_by_actual,
            nested_statements_by_actual_field,
        })
    }

    pub(in crate::schema) fn statement(&self, id: CheckStatementId) -> Option<&CheckStatementInfo> {
        self.statements.get(id.index())
    }

    pub(in crate::schema) fn statements(&self) -> impl Iterator<Item = &CheckStatementInfo> {
        self.statements.iter()
    }

    pub(in crate::schema) fn for_dependency(
        &self,
        dependency: &CheckDependency,
    ) -> impl Iterator<Item = CheckStatementId> + '_ {
        self.by_dependency
            .get(dependency)
            .into_iter()
            .flat_map(|ids| ids.iter().copied())
    }

    pub(in crate::schema) fn dependency_is_cross_record(
        &self,
        statement: CheckStatementId,
        dependency: &CheckDependency,
    ) -> bool {
        self.cross_record_dependencies
            .contains(&(statement, dependency.clone()))
    }

    pub(in crate::schema) fn for_actual_type(
        &self,
        actual_type: &str,
    ) -> impl Iterator<Item = CheckStatementId> + '_ {
        self.by_actual_type
            .get(actual_type)
            .into_iter()
            .flat_map(|ids| ids.iter().copied())
    }

    pub(in crate::schema) fn for_owner(&self, owner: &CheckOwner) -> &[CheckStatementId] {
        self.by_owner.get(owner).map_or(&[], Vec::as_slice)
    }

    pub(in crate::schema) fn hosts_for_nested_type(
        &self,
        nested_type: &str,
    ) -> impl Iterator<Item = &TypeName> {
        self.nested_hosts_by_type
            .get(nested_type)
            .into_iter()
            .flat_map(BTreeSet::iter)
    }

    pub(in crate::schema) fn for_nested_field(
        &self,
        actual_type: &TypeName,
        field_name: &FieldName,
    ) -> impl Iterator<Item = CheckStatementId> + '_ {
        self.nested_statements_by_actual_field
            .get(&(actual_type.clone(), field_name.clone()))
            .into_iter()
            .flat_map(|ids| ids.iter().copied())
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

    pub(in crate::schema) fn owner_applies_to_actual(&self, owner: &str, actual: &str) -> bool {
        self.owners_by_actual_type
            .get(actual)
            .is_some_and(|owners| owners.iter().any(|candidate| candidate.as_str() == owner))
    }
}

fn compile_owner_chains(
    types: &BTreeMap<TypeName, CftType>,
    budget: &mut StructuralBudget,
) -> Result<BTreeMap<TypeName, Vec<TypeName>>, LocatedBudgetError> {
    let mut result = BTreeMap::new();
    for actual_type in types.keys() {
        let mut owners = Vec::new();
        let mut current = Some(actual_type);
        while let Some(type_name) = current {
            let Some(meta) = types.get(type_name) else {
                break;
            };
            charge(budget, meta, owners.len().saturating_add(1))?;
            owners.push(meta.name.clone());
            current = meta.parent.as_ref();
        }
        owners.reverse();
        result.insert(actual_type.clone(), owners);
    }
    Ok(result)
}

fn compile_nested_hosts(
    types: &BTreeMap<TypeName, CftType>,
    owners: &BTreeMap<TypeName, Vec<TypeName>>,
    budget: &mut StructuralBudget,
) -> Result<
    (
        BTreeMap<TypeName, BTreeSet<TypeName>>,
        BTreeMap<(TypeName, FieldName), BTreeSet<TypeName>>,
    ),
    LocatedBudgetError,
> {
    let mut direct = BTreeMap::<TypeName, Vec<(FieldName, TypeName)>>::new();
    for (host, meta) in types {
        for field in &meta.all_fields {
            if let Some(target) = nested_type_target(&field.value_type) {
                direct
                    .entry(host.clone())
                    .or_default()
                    .push((field.name.clone(), target.clone()));
            }
        }
    }

    let has_checks = |actual: &TypeName| {
        owners.get(actual).into_iter().flatten().any(|owner| {
            types.get(owner).is_some_and(|meta| {
                meta.check
                    .as_ref()
                    .is_some_and(|block| !block.stmts.is_empty())
            })
        })
    };
    let mut hosts_by_nested = BTreeMap::<TypeName, BTreeSet<TypeName>>::new();
    let mut nested_types_by_field = BTreeMap::<(TypeName, FieldName), BTreeSet<TypeName>>::new();
    for host in types.keys() {
        if let Some(edges) = direct.get(host) {
            for (field, target) in edges {
                let mut seen = BTreeSet::new();
                let mut queue = types
                    .keys()
                    .filter(|candidate| {
                        owners
                            .get(*candidate)
                            .is_some_and(|chain| chain.contains(target))
                    })
                    .cloned()
                    .map(|nested| (nested, 1_usize))
                    .collect::<VecDeque<_>>();
                while let Some((nested, depth)) = queue.pop_front() {
                    if !seen.insert(nested.clone()) {
                        continue;
                    }
                    let Some(meta) = types.get(host) else {
                        continue;
                    };
                    charge(budget, meta, depth)?;
                    hosts_by_nested
                        .entry(nested.clone())
                        .or_default()
                        .insert(host.clone());
                    if let Some(nested_edges) = direct.get(&nested) {
                        for (_, nested_target) in nested_edges {
                            queue.extend(
                                types
                                    .keys()
                                    .filter(|candidate| {
                                        owners
                                            .get(*candidate)
                                            .is_some_and(|chain| chain.contains(nested_target))
                                    })
                                    .cloned()
                                    .map(|candidate| (candidate, depth.saturating_add(1))),
                            );
                        }
                    }
                }
                if seen.iter().any(&has_checks) {
                    nested_types_by_field.insert((host.clone(), field.clone()), seen);
                }
            }
        }
    }
    Ok((hosts_by_nested, nested_types_by_field))
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

fn charge(
    budget: &mut StructuralBudget,
    owner: &CftType,
    depth: usize,
) -> Result<(), LocatedBudgetError> {
    budget
        .check_additional_depth(
            TraversalCursor::root(),
            StructureKind::SchemaDependency,
            u64::try_from(depth).unwrap_or(u64::MAX),
        )
        .and_then(|()| budget.charge_work(StructureKind::SchemaDependency, 1))
        .map_err(|error| LocatedBudgetError {
            error,
            module: owner.module.clone(),
            span: owner.span,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build_schema, parse_modules, CftDimensionInputs, CftFile, ModuleId};
    use coflow_structure::StructuralLimits;

    #[test]
    fn dependency_index_compilation_obeys_the_schema_budget() {
        let modules = parse_modules([CftFile::from_source(
            ModuleId::from("main"),
            "type Part { value: int; check { value > 0; } } type Item { part: Part; }",
        )]);
        let schema = build_schema(&modules, &CftDimensionInputs::default()).expect("schema");
        let mut budget = StructuralBudget::new(StructuralLimits::new(1, 1, 1));

        assert!(
            CheckIndex::compile(&schema.types, &schema.top_level_checks, &mut budget,).is_err()
        );
    }
}
