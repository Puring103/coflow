use crate::emit::{build_csharp_database, build_csharp_enum, build_csharp_type};
use crate::model::CsharpProject;
use crate::schema_view::SchemaView;
use crate::CsharpCodegenError;
use coflow_cft::CftContainer;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsharpCodegenOptions {
    pub namespace: String,
    pub database_class: String,
}

impl CsharpCodegenOptions {
    #[must_use]
    pub fn new(namespace: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            database_class: "GameConfig".to_string(),
        }
    }

    #[must_use]
    pub fn with_database_class(mut self, database_class: impl Into<String>) -> Self {
        self.database_class = database_class.into();
        self
    }
}

pub(crate) fn build_project(
    schema: &CftContainer,
    options: &CsharpCodegenOptions,
) -> Result<CsharpProject, CsharpCodegenError> {
    let view = SchemaView::new(schema);

    let enums = schema
        .all_enums()
        .map(build_csharp_enum)
        .collect::<Vec<_>>();

    let types = schema
        .all_types()
        .map(|schema_type| build_csharp_type(schema_type, &view))
        .collect::<Result<Vec<_>, _>>()?;

    let tables = view.table_names();
    let database = build_csharp_database(&view, &tables, &options.database_class)?;

    Ok(CsharpProject {
        namespace: options.namespace.clone(),
        database_class: options.database_class.clone(),
        enums,
        types,
        database,
    })
}
