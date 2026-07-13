use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use coflow_api::{DiagnosticSet, ProviderRegistry};
use coflow_cft::CftContainer;
use coflow_data_model::CfdDataModel;
use coflow_project::Project;

use crate::dimensions;
use crate::dimensions::{DimensionField, DimensionGenerationTransaction};
use crate::indexes::{DiagnosticsStore, SessionIndexBuilder, SessionIndexes};
use crate::load::{
    empty_load_output, empty_model, load_project_data, LoadDiagnostics, LoadProjectDataOptions,
    ProjectLoadOutput,
};
use crate::schema_build::build_project_schema_with_diagnostics;
use crate::session::{ProjectSchemaSession, ProjectSession};

/// Opens a project into a reusable runtime session using explicit side-effect
/// intent.
///
/// [`SessionOpenOptions::build`] may write generated dimension sources before
/// the final reload. [`SessionOpenOptions::read_only`] is for editor,
/// inspection, and background tasks that must not mutate project files.
///
/// # Errors
///
/// Returns unrecoverable project/config/schema I/O errors. User-fixable
/// project, schema, loader, model, and check problems are captured in the
/// returned session diagnostics.
pub(crate) fn open_project_session(
    project: Project,
    registry: &ProviderRegistry,
    options: SessionOpenOptions,
) -> Result<ProjectSession, DiagnosticSet> {
    build_project_session_with_effects(project, registry, options).map(|output| output.session)
}

pub(crate) struct SessionBuildOutput {
    pub(crate) session: ProjectSession,
    pub(crate) changed_dimension_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SessionOpenOptions {
    intent: SessionIntent,
}

impl SessionOpenOptions {
    pub(crate) const fn build() -> Self {
        Self {
            intent: SessionIntent::Build,
        }
    }

    pub(crate) const fn read_only() -> Self {
        Self {
            intent: SessionIntent::ReadOnly,
        }
    }

    const fn dimension_mode(self) -> DimensionBuildMode {
        match self.intent {
            SessionIntent::Build => DimensionBuildMode::Generate,
            SessionIntent::ReadOnly => DimensionBuildMode::ReadOnly,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionIntent {
    Build,
    ReadOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DimensionBuildMode {
    Generate,
    ReadOnly,
}

pub(crate) fn build_project_session_with_effects(
    project: Project,
    registry: &ProviderRegistry,
    options: SessionOpenOptions,
) -> Result<SessionBuildOutput, DiagnosticSet> {
    finish_project_session(build_schema_session(project)?, registry, options)
}

pub(crate) fn rebuild_project_session_from_generation(
    session: &ProjectSession,
    registry: &ProviderRegistry,
) -> Result<SessionBuildOutput, DiagnosticSet> {
    finish_project_session(
        ProjectSchemaSession {
            project: session.project.clone(),
            schema: session.schema.clone(),
            diagnostics: DiagnosticsStore::empty(),
        },
        registry,
        SessionOpenOptions::build(),
    )
}

fn finish_project_session(
    schema_session: ProjectSchemaSession,
    registry: &ProviderRegistry,
    options: SessionOpenOptions,
) -> Result<SessionBuildOutput, DiagnosticSet> {
    let ProjectSchemaSession {
        project,
        schema,
        mut diagnostics,
    } = schema_session;

    let dimension_fields = dimensions::dimension_fields(schema.compiled_schema());
    let ctx = SessionBuildContext {
        project,
        schema,
        registry,
        dimension_mode: options.dimension_mode(),
        dimension_fields,
    };

    let LoadedSessionData {
        model,
        indexes,
        changed_dimension_paths,
    } = if diagnostics.is_empty() {
        build_data_pipeline(&ctx, &mut diagnostics)?
    } else {
        LoadedSessionData::empty()?
    };

    Ok(SessionBuildOutput {
        session: assemble_session(ctx, model, diagnostics, indexes),
        changed_dimension_paths,
    })
}

fn build_schema_session(project: Project) -> Result<ProjectSchemaSession, DiagnosticSet> {
    let mut initial_diagnostics = project.schema_diagnostic_set();
    initial_diagnostics.extend(project.data_diagnostic_set());
    build_project_schema_with_diagnostics(project, initial_diagnostics)
}

struct SessionBuildContext<'a> {
    project: Project,
    schema: Arc<CftContainer>,
    registry: &'a ProviderRegistry,
    dimension_mode: DimensionBuildMode,
    dimension_fields: Vec<DimensionField>,
}

impl SessionBuildContext<'_> {
    const fn has_dimension_fields(&self) -> bool {
        !self.dimension_fields.is_empty()
    }

    fn should_generate_dimensions(&self) -> bool {
        self.dimension_mode == DimensionBuildMode::Generate
            && !self.project.config.dimensions.is_empty()
    }
}

struct LoadedSessionData {
    model: CfdDataModel,
    indexes: SessionIndexes,
    changed_dimension_paths: Vec<PathBuf>,
}

impl LoadedSessionData {
    fn empty() -> Result<Self, DiagnosticSet> {
        Ok(Self {
            model: empty_model()?,
            indexes: SessionIndexes::default(),
            changed_dimension_paths: Vec::new(),
        })
    }
}

fn build_data_pipeline(
    ctx: &SessionBuildContext<'_>,
    diagnostics: &mut DiagnosticsStore,
) -> Result<LoadedSessionData, DiagnosticSet> {
    if ctx.dimension_mode == DimensionBuildMode::ReadOnly {
        return build_read_only_data(ctx, diagnostics);
    }
    let (mut output, mut indexes) = match load_base_data(ctx) {
        Ok(loaded) => loaded,
        Err(load_failure) => {
            diagnostics.extend_with_logical_locations(
                load_failure.diagnostics.diagnostics,
                load_failure.diagnostics.logical_locations,
            );
            return Ok(LoadedSessionData {
                model: empty_model()?,
                indexes: load_failure.indexes.finalize_rejected(),
                changed_dimension_paths: Vec::new(),
            });
        }
    };

    let mut dimensions = commit_dimensions_if_needed(ctx, &output, diagnostics);
    if diagnostics.is_empty() && ctx.has_dimension_fields() {
        (output, indexes) = reload_with_dimensions(ctx, diagnostics)?;
    }

    let indexes = indexes.finalize_with_model(&output.model);
    diagnostics.extend_with_logical_locations(output.diagnostics, output.logical_locations);
    rollback_dimensions_after_failed_pipeline(ctx, &mut dimensions.transaction, diagnostics);
    if !diagnostics.is_empty() {
        dimensions.changed_paths.clear();
    }

    Ok(LoadedSessionData {
        model: output.model,
        indexes,
        changed_dimension_paths: dimensions.changed_paths,
    })
}

fn load_base_data(
    ctx: &SessionBuildContext<'_>,
) -> Result<(ProjectLoadOutput, SessionIndexBuilder), Box<DataLoadFailure>> {
    load_data(ctx, false, !ctx.has_dimension_fields())
}

fn reload_with_dimensions(
    ctx: &SessionBuildContext<'_>,
    diagnostics: &mut DiagnosticsStore,
) -> Result<(ProjectLoadOutput, SessionIndexBuilder), DiagnosticSet> {
    match load_data(ctx, true, true) {
        Ok(loaded) => Ok(loaded),
        Err(load_failure) => {
            diagnostics.extend_with_logical_locations(
                load_failure.diagnostics.diagnostics,
                load_failure.diagnostics.logical_locations,
            );
            Ok((empty_load_output()?, load_failure.indexes))
        }
    }
}

fn load_data(
    ctx: &SessionBuildContext<'_>,
    include_implicit_dimension_sources: bool,
    run_checks: bool,
) -> Result<(ProjectLoadOutput, SessionIndexBuilder), Box<DataLoadFailure>> {
    let mut indexes = SessionIndexBuilder::default();
    let output = match load_project_data(
        &ctx.project,
        &ctx.schema,
        ctx.schema.compiled_schema(),
        ctx.registry,
        &mut indexes,
        LoadProjectDataOptions {
            include_implicit_dimension_sources,
            run_checks,
        },
    ) {
        Ok(output) => output,
        Err(diagnostics) => {
            return Err(Box::new(DataLoadFailure {
                diagnostics,
                indexes,
            }));
        }
    };
    Ok((output, indexes))
}

struct DataLoadFailure {
    diagnostics: LoadDiagnostics,
    indexes: SessionIndexBuilder,
}

#[derive(Default)]
struct CommittedDimensions {
    transaction: Option<DimensionGenerationTransaction>,
    changed_paths: Vec<PathBuf>,
}

fn build_read_only_data(
    ctx: &SessionBuildContext<'_>,
    diagnostics: &mut DiagnosticsStore,
) -> Result<LoadedSessionData, DiagnosticSet> {
    let (output, indexes) = match load_data(ctx, ctx.has_dimension_fields(), true) {
        Ok(loaded) => loaded,
        Err(load_failure) => {
            diagnostics.extend_with_logical_locations(
                load_failure.diagnostics.diagnostics,
                load_failure.diagnostics.logical_locations,
            );
            return Ok(LoadedSessionData {
                model: empty_model()?,
                indexes: load_failure.indexes.finalize_rejected(),
                changed_dimension_paths: Vec::new(),
            });
        }
    };
    let indexes = indexes.finalize_with_model(&output.model);
    diagnostics.extend_with_logical_locations(output.diagnostics, output.logical_locations);
    Ok(LoadedSessionData {
        model: output.model,
        indexes,
        changed_dimension_paths: Vec::new(),
    })
}

fn commit_dimensions_if_needed(
    ctx: &SessionBuildContext<'_>,
    output: &ProjectLoadOutput,
    diagnostics: &mut DiagnosticsStore,
) -> CommittedDimensions {
    if !ctx.should_generate_dimensions() {
        return CommittedDimensions::default();
    }

    let dimension_result = dimensions::regenerate_dimension_sources(
        &ctx.project,
        &output.model,
        &ctx.dimension_fields,
        ctx.registry,
    );
    diagnostics.extend(dimension_result.diagnostics);
    CommittedDimensions {
        transaction: (!dimension_result.transaction.is_empty())
            .then_some(dimension_result.transaction),
        changed_paths: dimension_result.changed_paths,
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
