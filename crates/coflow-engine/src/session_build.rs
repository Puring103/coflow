use std::collections::BTreeSet;

use coflow_api::ProviderRegistry;
use coflow_cft::CftSchemaView;
use coflow_project::Project;

use crate::dimensions;
use crate::indexes::{DependencyIndex, FileIndex, RecordIndex, SourceIndex};
use crate::load::{empty_load_output, empty_model, load_project_data, LoadProjectDataOptions};
use crate::schema_build::build_project_schema_with_diagnostics;
use crate::session::{ProjectSchemaSession, ProjectSession};

/// Opens, loads, builds, and checks a project into a reusable runtime session.
///
/// # Errors
///
/// Returns unrecoverable project/config/schema I/O errors. User-fixable
/// project, schema, loader, model, and check problems are captured in the
/// returned session diagnostics.
pub fn build_project_session(
    project: Project,
    registry: &ProviderRegistry,
) -> Result<ProjectSession, String> {
    build_project_session_with_dimension_mode(project, registry, DimensionBuildMode::Generate)
}

/// Opens, loads, and checks a project without writing generated dimension
/// sources or other derived files.
///
/// # Errors
///
/// Returns unrecoverable project/config/schema I/O errors. User-fixable
/// project, schema, loader, model, and check problems are captured in the
/// returned session diagnostics.
pub fn build_project_session_read_only(
    project: Project,
    registry: &ProviderRegistry,
) -> Result<ProjectSession, String> {
    build_project_session_with_dimension_mode(project, registry, DimensionBuildMode::ReadOnly)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DimensionBuildMode {
    Generate,
    ReadOnly,
}

fn build_project_session_with_dimension_mode(
    project: Project,
    registry: &ProviderRegistry,
    dimension_mode: DimensionBuildMode,
) -> Result<ProjectSession, String> {
    let mut initial_diagnostics = project.schema_diagnostic_set();
    initial_diagnostics.extend(project.data_diagnostic_set());
    let schema_session = build_project_schema_with_diagnostics(project, initial_diagnostics)?;
    let ProjectSchemaSession {
        project,
        schema,
        mut diagnostics,
    } = schema_session;

    let mut sources = SourceIndex::default();
    let mut records = RecordIndex::default();
    let mut files = FileIndex::default();
    let schema_view = CftSchemaView::new(&schema);
    let dimension_fields = dimensions::dimension_fields(&schema_view);
    let (model, dependencies) = if diagnostics.is_empty() {
        match load_project_data(
            &project,
            &schema,
            registry,
            &mut sources,
            &mut records,
            &mut files,
            LoadProjectDataOptions {
                include_implicit_dimension_sources: false,
                run_checks: dimension_fields.is_empty(),
            },
        ) {
            Ok(mut output) => {
                let has_dimension_fields = !dimension_fields.is_empty();
                let should_generate_dimensions =
                    dimension_mode == DimensionBuildMode::Generate && has_dimension_fields;
                let mut dimension_transaction = None;
                if should_generate_dimensions {
                    let dimension_result = dimensions::regenerate_dimension_sources(
                        &project,
                        &output.model,
                        &dimension_fields,
                        registry,
                    );
                    diagnostics.extend(dimension_result.diagnostics);
                    if diagnostics.is_empty() && !dimension_result.transaction.is_empty() {
                        dimension_transaction = Some(dimension_result.transaction);
                    }
                }
                if diagnostics.is_empty() && has_dimension_fields {
                    sources = SourceIndex::default();
                    records = RecordIndex::default();
                    files = FileIndex::default();
                    match load_project_data(
                        &project,
                        &schema,
                        registry,
                        &mut sources,
                        &mut records,
                        &mut files,
                        LoadProjectDataOptions {
                            include_implicit_dimension_sources: true,
                            run_checks: true,
                        },
                    ) {
                        Ok(reloaded) => output = reloaded,
                        Err(load_diagnostics) => {
                            diagnostics.extend_with_logical_locations(
                                load_diagnostics.diagnostics,
                                load_diagnostics.logical_locations,
                            );
                            output = empty_load_output()?;
                        }
                    }
                }
                records.finalize_with_model(&output.model);
                diagnostics
                    .extend_with_logical_locations(output.diagnostics, output.logical_locations);
                if !diagnostics.is_empty() {
                    if let Some(transaction) = dimension_transaction.take() {
                        diagnostics.extend(transaction.rollback(&project.config_path));
                    }
                }
                (output.model, output.dependencies)
            }
            Err(load_diagnostics) => {
                diagnostics.extend_with_logical_locations(
                    load_diagnostics.diagnostics,
                    load_diagnostics.logical_locations,
                );
                (empty_model()?, DependencyIndex::default())
            }
        }
    } else {
        (empty_model()?, DependencyIndex::default())
    };

    Ok(ProjectSession {
        project,
        schema,
        model,
        diagnostics,
        sources,
        records,
        files,
        dependencies,
        loader_extensions: loader_extensions(registry),
    })
}

fn loader_extensions(registry: &ProviderRegistry) -> BTreeSet<String> {
    let mut extensions = BTreeSet::new();
    for loader in registry.loaders() {
        for ext in loader.descriptor().extensions {
            extensions.insert((*ext).to_string());
        }
    }
    extensions
}
