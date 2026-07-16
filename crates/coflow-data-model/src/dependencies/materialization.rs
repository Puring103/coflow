use coflow_cft::{CftSchema, ValueDependencyCycle, ValueDependencyMode};

pub(crate) fn schema_default_cycle(
    schema: &CftSchema,
    type_name: &str,
) -> Option<ValueDependencyCycle> {
    schema
        .value_dependencies()
        .materialization_order(type_name, ValueDependencyMode::SchemaDefaults)?
        .err()
}
