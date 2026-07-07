//! Shared project runtime for Coflow hosts.

#![cfg_attr(
    not(test),
    deny(
        clippy::dbg_macro,
        clippy::expect_used,
        clippy::panic,
        clippy::panic_in_result_fn,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable,
        clippy::unwrap_used
    )
)]
#![allow(clippy::multiple_crate_versions)]

mod data_files;
mod data_patch;
mod data_read;
mod dimensions;
mod files;
mod indexes;
mod load;
mod mutation;
mod records;
mod schema_build;
mod schema_inspect;
mod session;
mod write_rules;
mod writes;

pub use data_files::{
    create_data_file, sync_data_header, DataCreateFileOptions, DataFileReport,
    DataSyncHeaderOptions,
};
pub use data_patch::{
    DataPatchAppliedOp, DataPatchFailedOp, DataPatchOp, DataPatchReport, DataPatchRequest,
    PatchPathSegment, PatchRecordSelector,
};
pub use data_read::{
    data_get, data_list, data_sources, DataGetQuery, DataGetReport, DataListQuery, DataListReport,
    DataRecordInfo, DataRecordSummary, DataSourceInfo, DataSourcesReport,
};
pub use dimensions::{
    builtin_display_name as dimension_builtin_display_name, dimensions_for_project,
    resolved_display_name as dimension_resolved_display_name, DimensionFieldInfo, DimensionInfo,
};
pub use files::{DimensionGroup, FileTreeNode, FileTreeOptions};
pub use indexes::{
    DependencyIndex, DiagnosticLogicalLocation, DiagnosticsStore, FileIndex, RecordIndex,
    RecordRef, ResolvedSourceEntry, SourceId, SourceIndex,
};
// Re-export helpers that hosts (tauri editor, CLI) call when translating
// engine data to a wire format so they don't diverge in path formatting.
pub use load::{configured_project_source, format_cfd_path as format_field_path};
pub use mutation::{
    DefaultMaterialization, MutationAppliedOp, MutationFailedOp, MutationFields, MutationOp,
    MutationReport, MutationRequest, MutationValue, PreparedMutation,
};
pub use records::{RecordTarget, RecordView, WriteOutcome};
pub use schema_build::build_project_schema_session;
pub use schema_inspect::{
    inspect_schema, schema_files, SchemaAnnotation, SchemaAnnotationValueInfo, SchemaConstInfo,
    SchemaConstValueInfo, SchemaDefaultValueInfo, SchemaEnumInfo, SchemaEnumVariantInfo,
    SchemaFieldInfo, SchemaFileInfo, SchemaFilesReport, SchemaInspectReport, SchemaTypeInfo,
    SchemaTypeRefInfo,
};
pub use session::{ProjectSchemaSession, ProjectSession, RecordCoordinate};

use coflow_api::ProviderRegistry;
use coflow_cft::CftSchemaView;
use coflow_project::Project;
use std::collections::BTreeSet;

use load::{empty_load_output, empty_model, load_project_data, LoadProjectDataOptions};
use schema_build::build_project_schema_with_diagnostics;

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
