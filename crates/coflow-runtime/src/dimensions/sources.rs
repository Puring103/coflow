use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use coflow_cft::{BucketName, CftSchema, DimensionName, FieldName, TypeName};
use coflow_data_model::RecordCoordinate;
use coflow_project::Project;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DimensionField {
    pub dimension: DimensionName,
    pub source_type: TypeName,
    pub source_field: FieldName,
    pub bucket: BucketName,
    pub is_singleton: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct DimensionRuntimePlan {
    fields: Vec<DimensionField>,
    fields_by_dimension: BTreeMap<DimensionName, Vec<usize>>,
    managed_directories: Vec<PathBuf>,
}

impl DimensionRuntimePlan {
    pub(crate) fn compile(schema: &CftSchema, project: &Project) -> Self {
        let mut fields = Vec::new();
        let mut fields_by_dimension: BTreeMap<DimensionName, Vec<usize>> = BTreeMap::new();
        for schema_type in schema.all_types() {
            for field in schema_type.own_fields() {
                let Some(dimension) = field.dimension.as_ref() else {
                    continue;
                };
                let index = fields.len();
                fields.push(DimensionField {
                    dimension: dimension.dimension.clone(),
                    source_type: schema_type.name.clone(),
                    source_field: field.name.clone(),
                    bucket: dimension
                        .bucket
                        .clone()
                        .unwrap_or_else(|| BucketName::from(schema_type.name.clone())),
                    is_singleton: schema_type.is_singleton,
                });
                fields_by_dimension
                    .entry(dimension.dimension.clone())
                    .or_default()
                    .push(index);
            }
        }
        let managed_directories = project
            .config
            .dimensions
            .iter()
            .filter(|(dimension, _)| fields_by_dimension.contains_key(dimension.as_str()))
            .filter_map(|(_, config)| config.out_dir.as_ref())
            .map(|directory| project.resolve_path(directory))
            .collect();
        Self {
            fields,
            fields_by_dimension,
            managed_directories,
        }
    }

    pub(crate) fn fields(&self) -> &[DimensionField] {
        &self.fields
    }

    pub(crate) fn fields_for(&self, dimension: &str) -> impl Iterator<Item = &DimensionField> {
        self.fields_by_dimension
            .get(dimension)
            .into_iter()
            .flat_map(|indices| indices.iter())
            .map(|index| &self.fields[*index])
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    pub(crate) fn is_managed_source_path(&self, project: &Project, display_path: &str) -> bool {
        let path = project.resolve_path(Path::new(display_path));
        matches!(
            path.extension().and_then(|extension| extension.to_str()),
            Some("csv" | "cfd")
        ) && self
            .managed_directories
            .iter()
            .any(|directory| path.starts_with(directory))
    }

    pub(crate) fn affected_field_indices(
        &self,
        schema: &CftSchema,
        changed: &std::collections::BTreeSet<RecordCoordinate>,
    ) -> std::collections::BTreeSet<usize> {
        self.fields
            .iter()
            .enumerate()
            .filter_map(|(index, field)| {
                changed
                    .iter()
                    .any(|record| schema.is_assignable(&record.actual_type, &field.source_type))
                    .then_some(index)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::collections::{BTreeMap, BTreeSet};

    use coflow_cft::{BucketName, DimensionName, FieldName, TypeName};
    use coflow_data_model::RecordCoordinate;

    use super::{DimensionField, DimensionRuntimePlan};

    #[test]
    fn changed_record_types_select_only_assignable_dimension_fields() {
        let modules = coflow_cft::parse_modules([coflow_cft::CftFile::new(
            coflow_cft::ModuleId::from("test.cft"),
            "test.cft".into(),
            "type Base { value: int; } type Child: Base {} type Other { value: int; }",
        )]);
        let schema = coflow_cft::build_schema(&modules, &coflow_cft::CftDimensionInputs::default())
            .expect("schema");
        let dimension = DimensionName::new("language").expect("dimension");
        let fields = vec![
            DimensionField {
                dimension: dimension.clone(),
                source_type: TypeName::new("Base").expect("type"),
                source_field: FieldName::new("value").expect("field"),
                bucket: BucketName::new("Base").expect("bucket"),
                is_singleton: false,
            },
            DimensionField {
                dimension: dimension.clone(),
                source_type: TypeName::new("Other").expect("type"),
                source_field: FieldName::new("value").expect("field"),
                bucket: BucketName::new("Other").expect("bucket"),
                is_singleton: false,
            },
        ];
        let plan = DimensionRuntimePlan {
            fields,
            fields_by_dimension: BTreeMap::from([(dimension, vec![0, 1])]),
            managed_directories: Vec::new(),
        };
        let changed =
            BTreeSet::from([RecordCoordinate::try_new("Child", "item").expect("coordinate")]);

        assert_eq!(
            plan.affected_field_indices(&schema, &changed),
            BTreeSet::from([0])
        );
    }
}
