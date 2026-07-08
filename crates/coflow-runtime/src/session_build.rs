use std::collections::BTreeSet;

use coflow_api::{CftContainer, ProviderRegistry};
use coflow_cft::CftSchemaView;
use coflow_data_model::CfdDataModel;
use coflow_project::Project;

use crate::dimensions;
use crate::dimensions::{DimensionField, DimensionGenerationTransaction};
use crate::indexes::{DependencyIndex, DiagnosticsStore, FileIndex, RecordIndex, SourceIndex};
use crate::load::{
    empty_load_output, empty_model, load_project_data, LoadDiagnostics, LoadProjectDataOptions,
    ProjectLoadOutput,
};
use crate::schema_build::build_project_schema_with_diagnostics;
use crate::session::{ProjectSchemaSession, ProjectSession};

/// Builds a project into a reusable runtime session for the normal `build`
/// pipeline.
///
/// This entry point may write generated dimension sources before the final
/// reload. Use [`open_project_session_read_only`] for editor, inspection, and
/// background tasks that must not mutate project files.
///
/// # Errors
///
/// Returns unrecoverable project/config/schema I/O errors. User-fixable
/// project, schema, loader, model, and check problems are captured in the
/// returned session diagnostics.
pub fn build_project_session_for_build(
    project: Project,
    registry: &ProviderRegistry,
) -> Result<ProjectSession, String> {
    build_project_session_with_mode(project, registry, DimensionBuildMode::Generate)
}

/// Opens, loads, and checks a project without writing generated dimension
/// sources or other derived files.
///
/// # Errors
///
/// Returns unrecoverable project/config/schema I/O errors. User-fixable
/// project, schema, loader, model, and check problems are captured in the
/// returned session diagnostics.
pub fn open_project_session_read_only(
    project: Project,
    registry: &ProviderRegistry,
) -> Result<ProjectSession, String> {
    build_project_session_with_mode(project, registry, DimensionBuildMode::ReadOnly)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DimensionBuildMode {
    Generate,
    ReadOnly,
}

fn build_project_session_with_mode(
    project: Project,
    registry: &ProviderRegistry,
    dimension_mode: DimensionBuildMode,
) -> Result<ProjectSession, String> {
    let ProjectSchemaSession {
        project,
        schema,
        mut diagnostics,
    } = build_schema_session(project)?;

    let schema_view = CftSchemaView::new(&schema);
    let dimension_fields = dimensions::dimension_fields(&schema_view);
    let ctx = SessionBuildContext {
        project,
        schema,
        registry,
        dimension_mode,
        dimension_fields,
    };

    let LoadedSessionData {
        model,
        dependencies,
        indexes,
    } = if diagnostics.is_empty() {
        build_data_pipeline(&ctx, &mut diagnostics)?
    } else {
        LoadedSessionData::empty()?
    };

    Ok(assemble_session(
        ctx,
        model,
        dependencies,
        diagnostics,
        indexes,
    ))
}

fn build_schema_session(project: Project) -> Result<ProjectSchemaSession, String> {
    let mut initial_diagnostics = project.schema_diagnostic_set();
    initial_diagnostics.extend(project.data_diagnostic_set());
    build_project_schema_with_diagnostics(project, initial_diagnostics)
}

struct SessionBuildContext<'a> {
    project: Project,
    schema: CftContainer,
    registry: &'a ProviderRegistry,
    dimension_mode: DimensionBuildMode,
    dimension_fields: Vec<DimensionField>,
}

impl SessionBuildContext<'_> {
    const fn has_dimension_fields(&self) -> bool {
        !self.dimension_fields.is_empty()
    }

    fn should_generate_dimensions(&self) -> bool {
        self.dimension_mode == DimensionBuildMode::Generate && self.has_dimension_fields()
    }
}

#[derive(Default)]
struct SessionIndexes {
    sources: SourceIndex,
    records: RecordIndex,
    files: FileIndex,
}

struct LoadedSessionData {
    model: CfdDataModel,
    dependencies: DependencyIndex,
    indexes: SessionIndexes,
}

impl LoadedSessionData {
    fn empty() -> Result<Self, String> {
        Ok(Self {
            model: empty_model()?,
            dependencies: DependencyIndex::default(),
            indexes: SessionIndexes::default(),
        })
    }
}

fn build_data_pipeline(
    ctx: &SessionBuildContext<'_>,
    diagnostics: &mut DiagnosticsStore,
) -> Result<LoadedSessionData, String> {
    let (mut output, mut indexes) = match load_base_data(ctx) {
        Ok(loaded) => loaded,
        Err(load_diagnostics) => {
            diagnostics.extend_with_logical_locations(
                load_diagnostics.diagnostics,
                load_diagnostics.logical_locations,
            );
            return LoadedSessionData::empty();
        }
    };

    let mut dimension_transaction = commit_dimensions_if_needed(ctx, &output, diagnostics);
    if diagnostics.is_empty() && ctx.has_dimension_fields() {
        (output, indexes) = reload_with_dimensions(ctx, diagnostics)?;
    }

    indexes.records.finalize_with_model(&output.model);
    diagnostics.extend_with_logical_locations(output.diagnostics, output.logical_locations);
    rollback_dimensions_after_failed_pipeline(ctx, &mut dimension_transaction, diagnostics);

    Ok(LoadedSessionData {
        model: output.model,
        dependencies: output.dependencies,
        indexes,
    })
}

fn load_base_data(
    ctx: &SessionBuildContext<'_>,
) -> Result<(ProjectLoadOutput, SessionIndexes), LoadDiagnostics> {
    load_data(ctx, false, !ctx.has_dimension_fields())
}

fn reload_with_dimensions(
    ctx: &SessionBuildContext<'_>,
    diagnostics: &mut DiagnosticsStore,
) -> Result<(ProjectLoadOutput, SessionIndexes), String> {
    match load_data(ctx, true, true) {
        Ok(loaded) => Ok(loaded),
        Err(load_diagnostics) => {
            diagnostics.extend_with_logical_locations(
                load_diagnostics.diagnostics,
                load_diagnostics.logical_locations,
            );
            Ok((empty_load_output()?, SessionIndexes::default()))
        }
    }
}

fn load_data(
    ctx: &SessionBuildContext<'_>,
    include_implicit_dimension_sources: bool,
    run_checks: bool,
) -> Result<(ProjectLoadOutput, SessionIndexes), LoadDiagnostics> {
    let mut indexes = SessionIndexes::default();
    let output = load_project_data(
        &ctx.project,
        &ctx.schema,
        ctx.registry,
        &mut indexes.sources,
        &mut indexes.records,
        &mut indexes.files,
        LoadProjectDataOptions {
            include_implicit_dimension_sources,
            run_checks,
        },
    )?;
    Ok((output, indexes))
}

fn commit_dimensions_if_needed(
    ctx: &SessionBuildContext<'_>,
    output: &ProjectLoadOutput,
    diagnostics: &mut DiagnosticsStore,
) -> Option<DimensionGenerationTransaction> {
    if !ctx.should_generate_dimensions() {
        return None;
    }

    let dimension_result = dimensions::regenerate_dimension_sources(
        &ctx.project,
        &output.model,
        &ctx.dimension_fields,
        ctx.registry,
    );
    diagnostics.extend(dimension_result.diagnostics);
    if dimension_result.transaction.is_empty() {
        None
    } else {
        Some(dimension_result.transaction)
    }
}

fn rollback_dimensions_after_failed_pipeline(
    ctx: &SessionBuildContext<'_>,
    dimension_transaction: &mut Option<DimensionGenerationTransaction>,
    diagnostics: &mut DiagnosticsStore,
) {
    if diagnostics.is_empty() {
        return;
    }
    if let Some(transaction) = dimension_transaction.take() {
        diagnostics.extend(transaction.rollback(&ctx.project.config_path));
    }
}

fn assemble_session(
    ctx: SessionBuildContext<'_>,
    model: CfdDataModel,
    dependencies: DependencyIndex,
    diagnostics: DiagnosticsStore,
    indexes: SessionIndexes,
) -> ProjectSession {
    ProjectSession {
        project: ctx.project,
        schema: ctx.schema,
        model,
        diagnostics,
        sources: indexes.sources,
        records: indexes.records,
        files: indexes.files,
        dependencies,
        loader_extensions: loader_extensions(ctx.registry),
    }
}

fn loader_extensions(registry: &ProviderRegistry) -> BTreeSet<String> {
    let mut extensions = BTreeSet::new();
    for loader in registry.source_providers() {
        for ext in loader.descriptor().extensions {
            extensions.insert((*ext).to_string());
        }
    }
    extensions
}
