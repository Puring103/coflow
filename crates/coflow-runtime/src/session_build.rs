use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use coflow_api::{DiagnosticSet, ProviderRegistry};
use coflow_cft::{CftModuleSet, CftSchema};
use coflow_data_model::CfdDataModel;
use coflow_project::Project;

use crate::checks::CheckState;
use crate::dimensions;
use crate::dimensions::{DimensionGenerationTransaction, DimensionRuntimePlan};
use crate::indexes::{DiagnosticsStore, SessionIndexBuilder, SessionIndexes};
use crate::load::{
    empty_load_output, empty_model, load_project_data, reload_project_data_from_cache,
    LoadDiagnostics, LoadProjectDataOptions, ProjectLoadOutput, SourceDataCache,
};
use crate::project_schema::open_project_schema_attempt;
use crate::session::{ProjectSchemaSession, ProjectSession};
use crate::writes::MutationImpact;
use crate::{FullFallbackReason, ProjectExecutionStats};

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

pub(crate) fn open_project_session_from_schema(
    schema_session: ProjectSchemaSession,
    registry: &ProviderRegistry,
    options: SessionOpenOptions,
) -> Result<ProjectSession, DiagnosticSet> {
    finish_project_session(schema_session, registry, options).map(|output| output.session)
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
    finish_project_session(open_schema_session(project)?, registry, options)
}

pub(crate) fn rebuild_project_session_from_generation(
    session: &ProjectSession,
    registry: &ProviderRegistry,
    impact: &MutationImpact,
) -> Result<SessionBuildOutput, DiagnosticSet> {
    let ctx = SessionBuildContext {
        project: session.project.clone(),
        modules: Arc::clone(&session.modules),
        schema: session.schema.clone(),
        registry,
        dimension_mode: DimensionBuildMode::Generate,
        dimension_plan: Arc::clone(&session.dimension_plan),
    };
    let mut diagnostics = DiagnosticsStore::empty();
    let LoadedSessionData {
        model,
        indexes,
        source_data,
        check_state,
        changed_dimension_paths,
        execution_stats,
    } = rebuild_data_pipeline(&ctx, session, impact, &mut diagnostics)?;
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
    registry: &ProviderRegistry,
    options: SessionOpenOptions,
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
    let ctx = SessionBuildContext {
        project,
        modules,
        schema,
        registry,
        dimension_mode: options.dimension_mode(),
        dimension_plan,
    };

    let LoadedSessionData {
        model,
        indexes,
        source_data,
        check_state,
        changed_dimension_paths,
        execution_stats,
    } = if diagnostics.is_empty() {
        build_data_pipeline(&ctx, &mut diagnostics)?
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

struct SessionBuildContext<'a> {
    project: Project,
    modules: Arc<CftModuleSet>,
    schema: Arc<CftSchema>,
    registry: &'a ProviderRegistry,
    dimension_mode: DimensionBuildMode,
    dimension_plan: Arc<DimensionRuntimePlan>,
}

impl SessionBuildContext<'_> {
    fn has_dimension_fields(&self) -> bool {
        !self.dimension_plan.is_empty()
    }

    fn should_generate_dimensions(&self) -> bool {
        self.dimension_mode == DimensionBuildMode::Generate
            && !self.project.config.dimensions.is_empty()
    }
}

struct LoadedSessionData {
    model: CfdDataModel,
    indexes: SessionIndexes,
    source_data: SourceDataCache,
    check_state: CheckState,
    changed_dimension_paths: Vec<PathBuf>,
    execution_stats: ProjectExecutionStats,
}

impl LoadedSessionData {
    fn empty(schema: &CftSchema) -> Result<Self, DiagnosticSet> {
        Ok(Self {
            model: empty_model(schema)?,
            indexes: SessionIndexes::default(),
            source_data: SourceDataCache::default(),
            check_state: CheckState::default(),
            changed_dimension_paths: Vec::new(),
            execution_stats: ProjectExecutionStats::default(),
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
                model: diagnostic_fallback_output(&ctx.schema, diagnostics)?.model,
                indexes: load_failure.indexes.finalize_rejected(),
                source_data: SourceDataCache::default(),
                check_state: CheckState::default(),
                changed_dimension_paths: Vec::new(),
                execution_stats: ProjectExecutionStats::default(),
            });
        }
    };

    let mut execution_stats = output.statistics;
    let mut dimensions = commit_dimensions_if_needed(ctx, &output, None, diagnostics);
    record_dimension_work(&mut execution_stats, &dimensions);
    if diagnostics.is_empty() && ctx.has_dimension_fields() {
        let (reloaded, reloaded_indexes) = reload_with_dimensions(ctx, diagnostics)?;
        execution_stats.merge(reloaded.statistics);
        output = reloaded;
        indexes = reloaded_indexes;
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
        source_data: output.source_data,
        check_state: output.check_state,
        changed_dimension_paths: dimensions.changed_paths,
        execution_stats,
    })
}

fn rebuild_data_pipeline(
    ctx: &SessionBuildContext<'_>,
    previous: &ProjectSession,
    impact: &MutationImpact,
    diagnostics: &mut DiagnosticsStore,
) -> Result<LoadedSessionData, DiagnosticSet> {
    let (mut output, mut indexes) = match load_cached_data(
        ctx,
        &previous.source_data,
        &impact.affected_files,
        false,
        !ctx.has_dimension_fields(),
        false,
        (!impact.structural_change).then_some(&previous.check_state),
        &impact.changed_records,
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
                check_state: CheckState::default(),
                changed_dimension_paths: Vec::new(),
                execution_stats: ProjectExecutionStats::default(),
            });
        }
    };

    let mut execution_stats = rebuild_execution_stats(&output, impact);
    let mut dimensions = commit_dimensions_if_needed(
        ctx,
        &output,
        (!impact.structural_change).then_some(&impact.changed_records),
        diagnostics,
    );
    record_dimension_work(&mut execution_stats, &dimensions);
    if diagnostics.is_empty() && ctx.has_dimension_fields() {
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
            &dimension_reload_paths,
            refresh_dimension_topology,
            true,
            true,
            (!impact.structural_change).then_some(&previous.check_state),
            &impact.changed_records,
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
    rollback_dimensions_after_failed_pipeline(ctx, &mut dimensions.transaction, diagnostics);
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
    ctx: &SessionBuildContext<'_>,
    include_implicit_dimension_sources: bool,
    run_checks: bool,
) -> Result<(ProjectLoadOutput, SessionIndexBuilder), Box<DataLoadFailure>> {
    let mut indexes = SessionIndexBuilder::default();
    let output = match load_project_data(
        &ctx.project,
        &ctx.schema,
        &ctx.dimension_plan,
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

#[allow(clippy::too_many_arguments)]
fn load_cached_data(
    ctx: &SessionBuildContext<'_>,
    previous: &SourceDataCache,
    reload_paths: &BTreeSet<String>,
    include_implicit_dimension_sources: bool,
    run_checks: bool,
    refresh_implicit_dimension_sources: bool,
    previous_checks: Option<&CheckState>,
    changed_records: &BTreeSet<crate::RecordCoordinate>,
) -> Result<(ProjectLoadOutput, SessionIndexBuilder), Box<DataLoadFailure>> {
    let mut indexes = SessionIndexBuilder::default();
    let output = match reload_project_data_from_cache(
        &ctx.project,
        &ctx.schema,
        &ctx.dimension_plan,
        ctx.registry,
        &mut indexes,
        previous,
        reload_paths,
        LoadProjectDataOptions {
            include_implicit_dimension_sources,
            run_checks,
        },
        refresh_implicit_dimension_sources,
        previous_checks,
        changed_records,
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
    path.strip_prefix(&project.root_dir).map_or_else(
        |_| path.display().to_string(),
        coflow_project::path_to_slash,
    )
}

struct DataLoadFailure {
    diagnostics: LoadDiagnostics,
    indexes: SessionIndexBuilder,
}

#[derive(Default)]
struct CommittedDimensions {
    transaction: Option<DimensionGenerationTransaction>,
    changed_paths: Vec<PathBuf>,
    planned_sources: usize,
    written_sources: usize,
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
                model: diagnostic_fallback_output(&ctx.schema, diagnostics)?.model,
                indexes: load_failure.indexes.finalize_rejected(),
                source_data: SourceDataCache::default(),
                check_state: CheckState::default(),
                changed_dimension_paths: Vec::new(),
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
        check_state: output.check_state,
        changed_dimension_paths: Vec::new(),
        execution_stats: output.statistics,
    })
}

fn commit_dimensions_if_needed(
    ctx: &SessionBuildContext<'_>,
    output: &ProjectLoadOutput,
    changed_records: Option<&BTreeSet<crate::RecordCoordinate>>,
    diagnostics: &mut DiagnosticsStore,
) -> CommittedDimensions {
    if !ctx.should_generate_dimensions() {
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
        ctx.registry,
    );
    diagnostics.extend(dimension_result.diagnostics);
    CommittedDimensions {
        transaction: (!dimension_result.transaction.is_empty())
            .then_some(dimension_result.transaction),
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

fn rebuild_execution_stats(
    output: &ProjectLoadOutput,
    impact: &MutationImpact,
) -> ProjectExecutionStats {
    let mut statistics = output.statistics;
    if impact.structural_change {
        statistics.mark_full_fallback(FullFallbackReason::StructuralMutation);
    }
    statistics
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
    source_data: SourceDataCache,
    check_state: CheckState,
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
        loader_extensions: loader_extensions(ctx.registry),
        source_data,
        check_state,
        execution_stats,
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
