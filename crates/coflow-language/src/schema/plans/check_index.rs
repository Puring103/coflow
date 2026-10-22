use crate::limits::{StructureKind, TraversalCursor};
use crate::schema::AnalysisBudget;
use crate::schema::{
    CheckDependency, CheckOwner, CheckStatementId, CheckStatementInfo, LocatedBudgetError,
};
use crate::{CftSchemaCheckStmt, CftTopLevelCheck, CftType, TypeName};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
pub struct CheckStatementRef<'schema> {
    pub info: &'schema CheckStatementInfo,
    pub statement: &'schema CftSchemaCheckStmt,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct CheckIndex {
    statements: Vec<CheckStatementInfo>,
    by_rule: BTreeMap<String, Vec<CheckStatementId>>,
    by_dependency: BTreeMap<CheckDependency, BTreeSet<CheckStatementId>>,
    cross_record_dependencies: BTreeSet<(CheckStatementId, CheckDependency)>,
    by_actual_type: BTreeMap<TypeName, Vec<CheckStatementId>>,
    owners_by_actual_type: BTreeMap<TypeName, Vec<TypeName>>,
}

impl CheckIndex {
    #[allow(clippy::too_many_lines)]
    pub(in crate::schema) fn compile(
        types: &BTreeMap<TypeName, CftType>,
        project_checks: &BTreeMap<crate::CheckName, CftTopLevelCheck>,
        budget: &mut AnalysisBudget,
    ) -> Result<Self, LocatedBudgetError> {
        let owners_by_actual_type = compile_owner_chains(types, budget)?;

        let mut roots = Vec::new();
        for ty in types.values() {
            for (block_index, block) in ty.checks.iter().enumerate() {
                for root_index in 0..block.stmts.len() {
                    roots.push((
                        ty.module.clone(),
                        ty.span.start,
                        CheckOwner::Type(ty.name.clone()),
                        block_index,
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
                    0,
                    root_index,
                ));
            }
        }
        roots.sort();

        let mut statements = Vec::with_capacity(roots.len());
        let mut by_rule = BTreeMap::<String, Vec<CheckStatementId>>::new();
        let mut by_dependency = BTreeMap::<CheckDependency, BTreeSet<CheckStatementId>>::new();
        let mut cross_record_dependencies = BTreeSet::new();
        let mut by_owner = BTreeMap::<CheckOwner, Vec<CheckStatementId>>::new();
        for (_, _, owner, block_index, root_index) in roots {
            let id = CheckStatementId::new(u32::try_from(statements.len()).unwrap_or(u32::MAX));
            // roots 直接由同一批已编译块产生，索引在构造期间始终有效。
            let block = match &owner {
                CheckOwner::Type(name) => &types[name].checks[block_index],
                CheckOwner::Project(name) => &project_checks[name].block,
            };
            let dependencies = block.statement_dependencies[root_index]
                .dependencies()
                .cloned()
                .collect();
            let dimensions = block
                .dimension_statements
                .iter()
                .filter(|(_, indices)| indices.contains(&root_index))
                .map(|(dimension, _)| dimension.clone())
                .collect();
            let info = CheckStatementInfo {
                id,
                owner: owner.clone(),
                root_index,
                block_index,
                rule_name: block.name.clone(),
                rule_id: match &owner {
                    CheckOwner::Type(name) => {
                        format!("{name}::{}", block.name)
                    }
                    CheckOwner::Project(name) => name.to_string(),
                },
                dependencies,
                dimensions,
            };
            for dependency in &info.dependencies {
                by_dependency
                    .entry(dependency.clone())
                    .or_default()
                    .insert(id);
            }
            if let Some(dependencies) = block.statement_dependencies.get(root_index) {
                cross_record_dependencies.extend(
                    dependencies
                        .cross_record_dependencies()
                        .cloned()
                        .map(|dependency| (id, dependency)),
                );
            }
            by_rule.entry(info.rule_id.clone()).or_default().push(id);
            by_owner.entry(owner).or_default().push(id);
            statements.push(info);
        }

        let mut by_actual_type = BTreeMap::new();
        for actual_type in types.keys() {
            let mut ids = Vec::new();
            for owner in owners_by_actual_type.get(actual_type).into_iter().flatten() {
                ids.extend(
                    by_owner
                        .get(&CheckOwner::Type(owner.clone()))
                        .into_iter()
                        .flatten()
                        .copied(),
                );
            }
            by_actual_type.insert(actual_type.clone(), ids);
        }

        Ok(Self {
            statements,
            by_rule,
            by_dependency,
            cross_record_dependencies,
            by_actual_type,
            owners_by_actual_type,
        })
    }

    pub(in crate::schema) fn statement(&self, id: CheckStatementId) -> Option<&CheckStatementInfo> {
        self.statements.get(id.index())
    }

    pub(in crate::schema) fn for_rule(
        &self,
        rule: &str,
    ) -> impl Iterator<Item = CheckStatementId> + '_ {
        self.by_rule.get(rule).into_iter().flatten().copied()
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

    pub(in crate::schema) fn owner_applies_to_actual(&self, owner: &str, actual: &str) -> bool {
        self.owners_by_actual_type
            .get(actual)
            .is_some_and(|owners| owners.iter().any(|candidate| candidate.as_str() == owner))
    }
}

fn compile_owner_chains(
    types: &BTreeMap<TypeName, CftType>,
    budget: &mut AnalysisBudget,
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

fn charge(
    budget: &mut AnalysisBudget,
    owner: &CftType,
    depth: usize,
) -> Result<(), LocatedBudgetError> {
    budget
        .check_depth(
            TraversalCursor::root(),
            StructureKind::SchemaDependency,
            u64::try_from(depth).unwrap_or(u64::MAX),
        )
        .and_then(|()| budget.charge(StructureKind::SchemaDependency, 1))
        .map_err(|error| LocatedBudgetError {
            error,
            module: owner.module.clone(),
            span: owner.span,
        })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::limits::StructuralLimits;
    use crate::{build_schema, parse_modules, CftFile, ModuleId};

    #[test]
    fn dependency_index_compilation_obeys_the_schema_budget() {
        let modules = parse_modules([CftFile::from_source(
            ModuleId::from("main"),
            "data Part { value: int; } table Item { part: Part; check Valid { part.value > 0; } }",
        )]);
        let schema = build_schema(&modules).expect("schema");
        let mut budget = AnalysisBudget::new(StructuralLimits::new(1, 1, 1));

        assert!(
            CheckIndex::compile(&schema.types, &schema.top_level_checks, &mut budget,).is_err()
        );
    }
}
