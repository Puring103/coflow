use super::{CftTypeMeta, CompiledSchema};
use crate::CftSchemaCheckBlock;
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct TypedCheckPlan {
    owners_by_actual: BTreeMap<String, Vec<String>>,
}

impl TypedCheckPlan {
    pub(super) fn compile(types: &BTreeMap<String, CftTypeMeta>) -> Self {
        let owners_by_actual = types
            .keys()
            .map(|actual_type| {
                let mut owners = Vec::new();
                let mut current = Some(actual_type.as_str());
                while let Some(type_name) = current {
                    let Some(meta) = types.get(type_name) else {
                        break;
                    };
                    owners.push(meta.name.clone());
                    current = meta.parent.as_deref();
                }
                owners.reverse();
                (actual_type.clone(), owners)
            })
            .collect();
        Self { owners_by_actual }
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
