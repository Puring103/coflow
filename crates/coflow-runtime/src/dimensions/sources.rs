use coflow_cft::{BucketName, CftSchema, DimensionName, FieldName, TypeName};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DimensionField {
    pub dimension: DimensionName,
    pub source_type: TypeName,
    pub source_field: FieldName,
    pub bucket: BucketName,
    pub is_singleton: bool,
}

pub fn dimension_fields(schema: &CftSchema) -> Vec<DimensionField> {
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
                bucket: dimension
                    .bucket
                    .clone()
                    .unwrap_or_else(|| BucketName::from(schema_type.name.clone())),
                is_singleton: schema_type.is_singleton,
            });
        }
    }
    fields
}
