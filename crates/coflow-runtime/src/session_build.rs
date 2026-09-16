use std::collections::BTreeSet;
use std::sync::Arc;

use crate::api::{CfdSourceCatalog, DiagnosticSet};
use crate::data_model::CfdDataModel;
use crate::project::Project;
use coflow_core::schema::{CftModuleSet, CftSchema};

use crate::dimensions::DimensionRuntimePlan;
use crate::indexes::{DiagnosticsStore, SessionIndexBuilder, SessionIndexes};
use crate::load::{
    empty_load_output, empty_model, load_project_data, reload_project_data_from_cache,
    DataSourceTextOverride, LoadDiagnostics, LoadProjectDataOptions, ProjectLoadOutput,
    ReloadProjectDataOptions, SourceDataCache,
};
use crate::project_schema::open_project_schema_attempt;
use crate::session::{ProjectSchemaSession, ProjectSession};
use crate::writes::MutationImpact;
use crate::ProjectExecutionStats;

/// Opens a project into a reusable runtime session using explicit side-effect
/// intent.
///
/// [`SessionOpenOptions::read_only`] is for editor, inspection, and background
/// tasks that must not mutate project files.
///
/// # Errors
///
/// Returns unrecoverable project/config/schema I/O errors. User-fixable
/// project, schema, loader, model, and check problems are captured in the
/// returned session diagnostics.
pub(crate) fn open_project_session(
    project: Project,
    catalog: &CfdSourceCatalog,
    options: SessionOpenOptions,
) -> Result<ProjectSession, DiagnosticSet> {
    build_project_session_with_effects(project, catalog, options).map(|output| output.session)
}

pub(crate) fn open_project_session_with_source_overrides(
    project: Project,
    catalog: &CfdSourceCatalog,
    options: SessionOpenOptions,
    source_overrides: &[DataSourceTextOverride],
) -> Result<ProjectSession, DiagnosticSet> {
    finish_project_session(
        open_schema_session(project)?,
        catalog,
        options,
        source_overrides,
    )
    .map(|output| output.session)
}

pub(crate) fn open_project_session_from_schema(
    schema_session: ProjectSchemaSession,
    catalog: &CfdSourceCatalog,
    options: SessionOpenOptions,
) -> Result<ProjectSession, DiagnosticSet> {
    finish_project_session(schema_session, catalog, options, &[]).map(|output| output.session)
}

pub(crate) struct SessionBuildOutput {
    pub(crate) session: ProjectSession,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionOpenOptions {
    Build,
    ReadOnly,
}

impl SessionOpenOptions {
    pub(crate) const fn build() -> Self {
        Self::Build
    }

    pub(crate) const fn read_only() -> Self {
        Self::ReadOnly
    }
}

pub(crate) fn build_project_session_with_effects(
    project: Project,
    catalog: &CfdSourceCatalog,
    options: SessionOpenOptions,
) -> Result<SessionBuildOutput, DiagnosticSet> {
    finish_project_session(open_schema_session(project)?, catalog, options, &[])
}

pub(crate) fn rebuild_project_session_from_generation(
    session: &ProjectSession,
    impact: &MutationImpact,
    source_overrides: &[DataSourceTextOverride],
) -> Result<SessionBuildOutput, DiagnosticSet> {
    let mut ctx = SessionBuildContext {
        project: session.project.clone(),
        modules: Arc::clone(&session.modules),
        schema: session.schema.clone(),
        mode: SessionOpenOptions::Build,
        dimension_plan: Arc::clone(&session.dimension_plan),
        source_overrides: source_overrides.to_vec(),
    };
    let mut diagnostics = DiagnosticsStore::empty();
    let LoadedSessionData {
        model,
        indexes,
        source_data,
        execution_stats,
    } = rebuild_data_pipeline(&mut ctx, session, impact, &mut diagnostics)?;
    Ok(SessionBuildOutput {
        session: assemble_session(
            ctx,
            model,
            diagnostics,
            indexes,
            source_data,
            execution_stats,
        ),
    })
}

fn finish_project_session(
    schema_session: ProjectSchemaSession,
    _catalog: &CfdSourceCatalog,
    options: SessionOpenOptions,
    source_overrides: &[DataSourceTextOverride],
) -> Result<SessionBuildOutput, DiagnosticSet> {
    let ProjectSchemaSession {
        project,
        modules,
        schema,
        mut diagnostics,
    } = schema_session;

    let Some(schema) = schema else {
        return Err(diagnostics.into_set());
    };

    let dimension_plan = Arc::new(DimensionRuntimePlan::compile(&schema, &project));
    let mut ctx = SessionBuildContext {
        project,
        modules,
        schema,
        mode: options,
        dimension_plan,
        source_overrides: source_overrides.to_vec(),
    };

    let LoadedSessionData {
        model,
        indexes,
        source_data,
        execution_stats,
    } = if diagnostics.is_empty() {
        build_data_pipeline(&mut ctx, &mut diagnostics)?
    } else {
        LoadedSessionData::empty(&ctx.schema)?
    };

    Ok(SessionBuildOutput {
        session: assemble_session(
            ctx,
            model,
            diagnostics,
            indexes,
            source_data,
            execution_stats,
        ),
    })
}

fn open_schema_session(project: Project) -> Result<ProjectSchemaSession, DiagnosticSet> {
    let mut initial_diagnostics = project.schema_diagnostic_set();
    initial_diagnostics.extend(project.data_diagnostic_set());
    open_project_schema_attempt(project, initial_diagnostics, &[])
}

struct SessionBuildContext {
    project: Project,
    modules: Arc<CftModuleSet>,
    schema: Arc<CftSchema>,
    mode: SessionOpenOptions,
    dimension_plan: Arc<DimensionRuntimePlan>,
    source_overrides: Vec<DataSourceTextOverride>,
}

struct LoadedSessionData {
    model: CfdDataModel,
    indexes: SessionIndexes,
    source_data: SourceDataCache,
    execution_stats: ProjectExecutionStats,
}

impl LoadedSessionData {
    fn empty(schema: &CftSchema) -> Result<Self, DiagnosticSet> {
        Ok(Self {
            model: empty_model(schema)?,
            indexes: SessionIndexes::default(),
            source_data: SourceDataCache::default(),
            execution_stats: ProjectExecutionStats::default(),
        })
    }
}

fn build_data_pipeline(
    ctx: &mut SessionBuildContext,
    diagnostics: &mut DiagnosticsStore,
) -> Result<LoadedSessionData, DiagnosticSet> {
    if ctx.mode == SessionOpenOptions::ReadOnly {
        return build_read_only_data(ctx, diagnostics);
    }
    let (output, indexes) = match load_data(ctx, true) {
        Ok(loaded) => loaded,
        Err(load_failure) => {
            diagnostics.extend_with_logical_locations(
                load_failure.diagnostics.diagnostics,
                load_failure.diagnostics.logical_locations,
            );
            return Ok(LoadedSessionData {
                model: diagnostic_fallback_output(&ctx.schema, diagnostics)?.model,
                indexes: load_failure.indexes.finalize_rejected(),
                source_data: SourceDataCache::default(),
                execution_stats: ProjectExecutionStats::default(),
            });
        }
    };

    let indexes = indexes.finalize_with_model(&output.model);
    diagnostics.extend_with_logical_locations(output.diagnostics, output.logical_locations);

    Ok(LoadedSessionData {
        model: output.model,
        indexes,
        source_data: output.source_data,
        execution_stats: output.statistics,
    })
}

#[allow(clippy::too_many_lines)]
fn rebuild_data_pipeline(
    ctx: &mut SessionBuildContext,
    previous: &ProjectSession,
    impact: &MutationImpact,
    diagnostics: &mut DiagnosticsStore,
) -> Result<LoadedSessionData, DiagnosticSet> {
    let (output, indexes) = match load_cached_data(
        ctx,
        &previous.source_data,
        CachedLoadOptions {
            reload_paths: &impact.affected_files,
            run_checks: true,
        },
    ) {
        Ok(loaded) => loaded,
        Err(load_failure) => {
            diagnostics.extend_with_logical_locations(
                load_failure.diagnostics.diagnostics,
                load_failure.diagnostics.logical_locations,
            );
            return Ok(LoadedSessionData {
                model: diagnostic_fallback_output(&ctx.schema, diagnostics)?.model,
                indexes: load_failure.indexes.finalize_rejected(),
                source_data: SourceDataCache::default(),
                execution_stats: ProjectExecutionStats::default(),
            });
        }
    };

    let indexes = indexes.finalize_with_model(&output.model);
    diagnostics.extend_with_logical_locations(output.diagnostics, output.logical_locations);
    Ok(LoadedSessionData {
        model: output.model,
        indexes,
        source_data: output.source_data,
        execution_stats: output.statistics,
    })
}

fn diagnostic_fallback_output(
    schema: &CftSchema,
    diagnostics: &DiagnosticsStore,
) -> Result<ProjectLoadOutput, DiagnosticSet> {
    empty_load_output(schema).map_err(|_| diagnostics.as_set().clone())
}

fn load_data(
    ctx: &SessionBuildContext,
    run_checks: bool,
) -> Result<(ProjectLoadOutput, SessionIndexBuilder), Box<DataLoadFailure>> {
    let mut indexes = SessionIndexBuilder::default();
    let output = match load_project_data(
        &ctx.project,
        &ctx.schema,
        &mut indexes,
        LoadProjectDataOptions { run_checks },
        &ctx.source_overrides,
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

#[derive(Clone, Copy)]
struct CachedLoadOptions<'a> {
    reload_paths: &'a BTreeSet<String>,
    run_checks: bool,
}

fn load_cached_data(
    ctx: &SessionBuildContext,
    previous: &SourceDataCache,
    options: CachedLoadOptions<'_>,
) -> Result<(ProjectLoadOutput, SessionIndexBuilder), Box<DataLoadFailure>> {
    let mut indexes = SessionIndexBuilder::default();
    let output = match reload_project_data_from_cache(
        &ctx.schema,
        &mut indexes,
        previous,
        options.reload_paths,
        ReloadProjectDataOptions {
            load: LoadProjectDataOptions {
                run_checks: options.run_checks,
            },
            source_overrides: &ctx.source_overrides,
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

fn build_read_only_data(
    ctx: &SessionBuildContext,
    diagnostics: &mut DiagnosticsStore,
) -> Result<LoadedSessionData, DiagnosticSet> {
    let (output, indexes) = match load_data(ctx, true) {
        Ok(loaded) => loaded,
        Err(load_failure) => {
            diagnostics.extend_with_logical_locations(
                load_failure.diagnostics.diagnostics,
                load_failure.diagnostics.logical_locations,
            );
            return Ok(LoadedSessionData {
                model: diagnostic_fallback_output(&ctx.schema, diagnostics)?.model,
                indexes: load_failure.indexes.finalize_rejected(),
                source_data: SourceDataCache::default(),
                execution_stats: ProjectExecutionStats::default(),
            });
        }
    };
    let indexes = indexes.finalize_with_model(&output.model);
    diagnostics.extend_with_logical_locations(output.diagnostics, output.logical_locations);
    Ok(LoadedSessionData {
        model: output.model,
        indexes,
        source_data: output.source_data,
        execution_stats: output.statistics,
    })
}

fn assemble_session(
    ctx: SessionBuildContext,
    model: CfdDataModel,
    diagnostics: DiagnosticsStore,
    indexes: SessionIndexes,
    source_data: SourceDataCache,
    execution_stats: ProjectExecutionStats,
) -> ProjectSession {
    ProjectSession {
        project: ctx.project,
        modules: ctx.modules,
        schema: ctx.schema,
        dimension_plan: ctx.dimension_plan,
        model,
        diagnostics,
        sources: indexes.sources,
        records: indexes.records,
        files: indexes.files,
        source_data,
        execution_stats,
    }
}
