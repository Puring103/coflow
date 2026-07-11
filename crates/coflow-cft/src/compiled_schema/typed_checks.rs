use super::{CftTypeMeta, CompiledSchema, LocatedBudgetError};
use crate::{CftSchemaCheckBlock, ModuleId};
use coflow_structure::{StructuralBudget, StructureKind, TraversalCursor};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default)]
pub struct TypedCheckPlan {
    owners_by_actual: BTreeMap<String, Vec<String>>,
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
        Ok(Self { owners_by_actual })
    }

    pub(super) fn owners(&self, actual_type: &str) -> &[String] {
        self.owners_by_actual
            .get(actual_type)
            .map_or(&[], Vec::as_slice)
    }
}

#[derive(Debug)]
pub struct TypedCheckSchedule<'schema, 'dimension> {
    schema: &'schema CompiledSchema,
    owners: std::slice::Iter<'schema, String>,
    dimension: Option<&'dimension str>,
}

impl<'schema, 'dimension> TypedCheckSchedule<'schema, 'dimension> {
    pub(super) fn new(
        schema: &'schema CompiledSchema,
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
