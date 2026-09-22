use coflow_core::schema::{CftSchema, DimensionName, FieldName, TypeName};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct DimensionField {
    pub dimension: DimensionName,
    pub source_type: TypeName,
    pub source_field: FieldName,
    pub is_singleton: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct DimensionRuntimePlan {
    fields: Vec<DimensionField>,
}

impl DimensionRuntimePlan {
    pub(crate) fn compile(schema: &CftSchema, _project: &crate::project::Project) -> Self {
        let mut fields = Vec::new();
        for schema_type in schema.all_types() {
            for field in schema_type.own_fields() {
                let Some(dimension) = field.dimension.as_ref() else {
                    continue;
                };
                fields.push(DimensionField {
                    dimension: dimension.dimension.clone(),
                    source_type: schema_type.name.clone(),
                    source_field: field.name.clone(),
                    is_singleton: schema_type.is_singleton,
                });
            }
        }
        Self { fields }
    }

    pub(crate) fn fields(&self) -> &[DimensionField] {
        &self.fields
    }
}
