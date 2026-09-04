use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use crate::api::{CfdSourceCatalog, DiagnosticSet};
use crate::data_model::CfdDataModel;
use crate::project::Project;
use coflow_language::cft::{CftModuleSet, CftSchema};

use crate::checks::CheckDiagnosticStore;
use crate::dimensions;
use crate::cfd_loader::CfdWriter;
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
    pub(crate) changed_dimension_paths: Vec<PathBuf>,
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
    catalog: &CfdSourceCatalog,
    impact: &MutationImpact,
    source_overrides: &[DataSourceTextOverride],
) -> Result<SessionBuildOutput, DiagnosticSet> {
    let mut ctx = SessionBuildContext {
        project: session.project.clone(),
        modules: Arc::clone(&session.modules),
        schema: session.schema.clone(),
        catalog: catalog.clone(),
        mode: SessionOpenOptions::Build,
        dimension_plan: Arc::clone(&session.dimension_plan),
        source_overrides: source_overrides.to_vec(),
        publish_immediately: false,
    };
    let mut diagnostics = DiagnosticsStore::empty();
    let LoadedSessionData {
        model,
        indexes,
        source_data,
        check_state,
        changed_dimension_paths,
        execution_stats,
        writer: _,
    } = rebuild_data_pipeline(&mut ctx, session, impact, &mut diagnostics)?;
    Ok(SessionBuildOutput {
        session: assemble_session(
            ctx,
            model,
            diagnostics,
            indexes,
            source_data,
            check_state,
            execution_stats,
        ),
        changed_dimension_paths,
    })
}

fn finish_project_session(
    schema_session: ProjectSchemaSession,
    catalog: &CfdSourceCatalog,
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
        catalog: if options == SessionOpenOptions::Build {
            catalog.staged_writes()
        } else {
            catalog.clone()
        },
        mode: options,
        dimension_plan,
        source_overrides: source_overrides.to_vec(),
        publish_immediately: options == SessionOpenOptions::Build,
    };

    let LoadedSessionData {
        model,
        indexes,
        source_data,
        check_state,
        changed_dimension_paths,
        execution_stats,
        writer,
    } = if diagnostics.is_empty() {
        build_data_pipeline(&mut ctx, &mut diagnostics)?
    } else {
        LoadedSessionData::empty(&ctx.schema)?
    };

    if ctx.publish_immediately && diagnostics.is_empty() {
        if let Some(writer) = writer {
            writer.publish()?;
        }
    }

    Ok(SessionBuildOutput {
        session: assemble_session(
            ctx,
            model,
            diagnostics,
            indexes,
            source_data,
            check_state,
            execution_stats,
        ),
        changed_dimension_paths,
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
    catalog: CfdSourceCatalog,
    mode: SessionOpenOptions,
    dimension_plan: Arc<DimensionRuntimePlan>,
    source_overrides: Vec<DataSourceTextOverride>,
    publish_immediately: bool,
}

impl SessionBuildContext {
    fn has_dimension_fields(&self) -> bool {
        !self.dimension_plan.is_empty()
    }

    fn should_generate_dimensions(&self) -> bool {
        self.mode == SessionOpenOptions::Build && !self.project.config().dimensions.is_empty()
    }
}

struct LoadedSessionData {
    model: CfdDataModel,
    indexes: SessionIndexes,
    source_data: SourceDataCache,
    check_state: CheckDiagnosticStore,
    changed_dimension_paths: Vec<PathBuf>,
    execution_stats: ProjectExecutionStats,
    writer: Option<Arc<CfdWriter>>,
}

impl LoadedSessionData {
    fn empty(schema: &CftSchema) -> Result<Self, DiagnosticSet> {
        Ok(Self {
            model: empty_model(schema)?,
            indexes: SessionIndexes::default(),
            source_data: SourceDataCache::default(),
            check_state: CheckDiagnosticStore::default(),
            changed_dimension_paths: Vec::new(),
            execution_stats: ProjectExecutionStats::default(),
            writer: None,
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
    let (mut output, mut indexes) = match load_base_data(ctx) {
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
                check_state: CheckDiagnosticStore::default(),
                changed_dimension_paths: Vec::new(),
                execution_stats: ProjectExecutionStats::default(),
                writer: None,
            });
        }
    };

    let mut execution_stats = output.statistics;
    let mut dimensions = commit_dimensions_if_needed(ctx, &output, None, diagnostics);
    record_dimension_work(&mut execution_stats, &dimensions);
    merge_dimension_overrides(ctx, dimensions.writer.as_ref())?;
    if diagnostics.is_empty() && output.diagnostics.is_empty() && ctx.has_dimension_fields() {
        let (reloaded, reloaded_indexes) = reload_with_dimensions(ctx, diagnostics)?;
        execution_stats.merge(reloaded.statistics);
        output = reloaded;
        indexes = reloaded_indexes;
    }

    let indexes = indexes.finalize_with_model(&output.model);
    diagnostics.extend_with_logical_locations(output.diagnostics, output.logical_locations);
    if !diagnostics.is_empty() {
        dimensions.changed_paths.clear();
    }

    Ok(LoadedSessionData {
        model: output.model,
        indexes,
        source_data: output.source_data,
        check_state: output.check_state,
        changed_dimension_paths: dimensions.changed_paths,
        execution_stats,
        writer: dimensions.writer,
    })
}

#[allow(clippy::too_many_lines)]
fn rebuild_data_pipeline(
    ctx: &mut SessionBuildContext,
    previous: &ProjectSession,
    impact: &MutationImpact,
    diagnostics: &mut DiagnosticsStore,
) -> Result<LoadedSessionData, DiagnosticSet> {
    let changed_records = impact.changed_records();
    let check_impact = impact.check_impact(&ctx.schema);
    let (mut output, mut indexes) = match load_cached_data(
        ctx,
        &previous.source_data,
        CachedLoadOptions {
            reload_paths: &impact.affected_files,
            include_implicit_dimension_sources: false,
            run_checks: !ctx.has_dimension_fields(),
            refresh_implicit_dimension_sources: false,
            previous_checks: Some(&previous.check_state),
            check_impact: &check_impact,
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
                check_state: CheckDiagnosticStore::default(),
                changed_dimension_paths: Vec::new(),
                execution_stats: ProjectExecutionStats::default(),
                writer: None,
            });
        }
    };

    let mut execution_stats = output.statistics;
    let mut dimensions = commit_dimensions_if_needed(
        ctx,
        &output,
        (!impact.structural_change).then_some(&changed_records),
        diagnostics,
    );
    record_dimension_work(&mut execution_stats, &dimensions);
    merge_dimension_overrides(ctx, dimensions.writer.as_ref())?;
    if diagnostics.is_empty() && output.diagnostics.is_empty() && ctx.has_dimension_fields() {
        let cache = output
            .source_data
            .base_with_previous_dimensions(&previous.source_data);
        let implicit_paths = previous.source_data.implicit_display_paths();
        let mut dimension_reload_paths = impact
            .affected_files
            .intersection(&implicit_paths)
            .cloned()
            .collect::<BTreeSet<_>>();
        dimension_reload_paths.extend(
            impact
                .affected_files
                .iter()
                .filter(|path| {
                    ctx.dimension_plan
                        .is_managed_source_path(&ctx.project, path)
                })
                .cloned(),
        );
        dimension_reload_paths.extend(
            dimensions
                .changed_paths
                .iter()
                .map(|path| project_display_path(&ctx.project, path)),
        );
        let refresh_dimension_topology = !dimension_reload_paths.is_empty();
        match load_cached_data(
            ctx,
            &cache,
            CachedLoadOptions {
                reload_paths: &dimension_reload_paths,
                include_implicit_dimension_sources: refresh_dimension_topology,
                run_checks: true,
                refresh_implicit_dimension_sources: true,
                previous_checks: Some(&previous.check_state),
                check_impact: &check_impact,
            },
        ) {
            Ok((reloaded, reloaded_indexes)) => {
                execution_stats.merge(reloaded.statistics);
                output = reloaded;
                indexes = reloaded_indexes;
            }
            Err(load_failure) => {
                diagnostics.extend_with_logical_locations(
                    load_failure.diagnostics.diagnostics,
                    load_failure.diagnostics.logical_locations,
                );
                output = diagnostic_fallback_output(&ctx.schema, diagnostics)?;
                indexes = load_failure.indexes;
            }
        }
    }

    let indexes = indexes.finalize_with_model(&output.model);
    diagnostics.extend_with_logical_locations(output.diagnostics, output.logical_locations);
    if !diagnostics.is_empty() {
        dimensions.changed_paths.clear();
    }
    Ok(LoadedSessionData {
        model: output.model,
        indexes,
        source_data: output.source_data,
        check_state: output.check_state,
        changed_dimension_paths: dimensions.changed_paths,
        execution_stats,
        writer: dimensions.writer,
    })
}

fn load_base_data(
    ctx: &SessionBuildContext,
) -> Result<(ProjectLoadOutput, SessionIndexBuilder), Box<DataLoadFailure>> {
    load_data(ctx, false, !ctx.has_dimension_fields())
}

fn reload_with_dimensions(
    ctx: &SessionBuildContext,
    diagnostics: &mut DiagnosticsStore,
) -> Result<(ProjectLoadOutput, SessionIndexBuilder), DiagnosticSet> {
    match load_data(ctx, true, true) {
        Ok(loaded) => Ok(loaded),
        Err(load_failure) => {
            diagnostics.extend_with_logical_locations(
                load_failure.diagnostics.diagnostics,
                load_failure.diagnostics.logical_locations,
            );
            Ok((
                diagnostic_fallback_output(&ctx.schema, diagnostics)?,
                load_failure.indexes,
            ))
        }
    }
}

fn diagnostic_fallback_output(
    schema: &CftSchema,
    diagnostics: &DiagnosticsStore,
) -> Result<ProjectLoadOutput, DiagnosticSet> {
    empty_load_output(schema).map_err(|_| diagnostics.as_set().clone())
}

fn load_data(
    ctx: &SessionBuildContext,
    include_implicit_dimension_sources: bool,
    run_checks: bool,
) -> Result<(ProjectLoadOutput, SessionIndexBuilder), Box<DataLoadFailure>> {
    let mut indexes = SessionIndexBuilder::default();
    let output = match load_project_data(
        &ctx.project,
        &ctx.schema,
        &ctx.dimension_plan,
        &ctx.catalog,
        &mut indexes,
        LoadProjectDataOptions {
            include_implicit_dimension_sources,
            run_checks,
        },
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
    include_implicit_dimension_sources: bool,
    run_checks: bool,
    refresh_implicit_dimension_sources: bool,
    previous_checks: Option<&'a CheckDiagnosticStore>,
    check_impact: &'a crate::checks::impact::CheckImpact,
}

fn load_cached_data(
    ctx: &SessionBuildContext,
    previous: &SourceDataCache,
    options: CachedLoadOptions<'_>,
) -> Result<(ProjectLoadOutput, SessionIndexBuilder), Box<DataLoadFailure>> {
    let mut indexes = SessionIndexBuilder::default();
    let output = match reload_project_data_from_cache(
        &ctx.project,
        &ctx.schema,
        &ctx.dimension_plan,
        &ctx.catalog,
        &mut indexes,
        previous,
        options.reload_paths,
        ReloadProjectDataOptions {
            load: LoadProjectDataOptions {
                include_implicit_dimension_sources: options.include_implicit_dimension_sources,
                run_checks: options.run_checks,
            },
            refresh_implicit_dimension_sources: options.refresh_implicit_dimension_sources,
            previous_checks: options.previous_checks,
            check_impact: options.check_impact,
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

fn project_display_path(project: &Project, path: &std::path::Path) -> String {
    path.strip_prefix(project.root_dir()).map_or_else(
        |_| path.display().to_string(),
        crate::project::path_to_slash,
    )
}

struct DataLoadFailure {
    diagnostics: LoadDiagnostics,
    indexes: SessionIndexBuilder,
}

#[derive(Default)]
struct CommittedDimensions {
    writer: Option<Arc<CfdWriter>>,
    changed_paths: Vec<PathBuf>,
    planned_sources: usize,
    written_sources: usize,
}

fn build_read_only_data(
    ctx: &SessionBuildContext,
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
                model: diagnostic_fallback_output(&ctx.schema, diagnostics)?.model,
                indexes: load_failure.indexes.finalize_rejected(),
                source_data: SourceDataCache::default(),
                check_state: CheckDiagnosticStore::default(),
                changed_dimension_paths: Vec::new(),
                execution_stats: ProjectExecutionStats::default(),
                writer: None,
            });
        }
    };
    let indexes = indexes.finalize_with_model(&output.model);
    diagnostics.extend_with_logical_locations(output.diagnostics, output.logical_locations);
    Ok(LoadedSessionData {
        model: output.model,
        indexes,
        source_data: output.source_data,
        check_state: output.check_state,
        changed_dimension_paths: Vec::new(),
        execution_stats: output.statistics,
        writer: None,
    })
}

fn commit_dimensions_if_needed(
    ctx: &mut SessionBuildContext,
    output: &ProjectLoadOutput,
    changed_records: Option<&BTreeSet<crate::RecordCoordinate>>,
    diagnostics: &mut DiagnosticsStore,
) -> CommittedDimensions {
    if !ctx.should_generate_dimensions() || !output.diagnostics.is_empty() {
        return CommittedDimensions::default();
    }

    let affected_fields = changed_records.map(|changed| {
        ctx.dimension_plan
            .affected_field_indices(&ctx.schema, changed)
    });
    if affected_fields.as_ref().is_some_and(BTreeSet::is_empty) {
        return CommittedDimensions::default();
    }
    let dimension_result = dimensions::regenerate_dimension_sources_scoped(
        &ctx.project,
        &ctx.schema,
        &output.model,
        ctx.dimension_plan.fields(),
        affected_fields.as_ref(),
        &ctx.catalog,
    );
    diagnostics.extend(dimension_result.diagnostics);
    CommittedDimensions {
        writer: dimension_result.writer,
        changed_paths: dimension_result.changed_paths,
        planned_sources: dimension_result.planned_sources,
        written_sources: dimension_result.written_sources,
    }
}

const fn record_dimension_work(
    statistics: &mut ProjectExecutionStats,
    dimensions: &CommittedDimensions,
) {
    statistics.dimension_sources_planned = statistics
        .dimension_sources_planned
        .saturating_add(dimensions.planned_sources);
    statistics.dimension_sources_written = statistics
        .dimension_sources_written
        .saturating_add(dimensions.written_sources);
}

fn merge_dimension_overrides(
    ctx: &mut SessionBuildContext,
    writer: Option<&Arc<CfdWriter>>,
) -> Result<(), DiagnosticSet> {
    let Some(writer) = writer else {
        return Ok(());
    };
    let mut overrides = ctx.source_overrides.clone();
    overrides.extend(writer.source_overrides()?);
    ctx.source_overrides = overrides;
    Ok(())
}

fn assemble_session(
    ctx: SessionBuildContext,
    model: CfdDataModel,
    diagnostics: DiagnosticsStore,
    indexes: SessionIndexes,
    source_data: SourceDataCache,
    check_state: CheckDiagnosticStore,
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
        check_state,
        execution_stats,
    }
}
