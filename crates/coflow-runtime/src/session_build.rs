use std::collections::BTreeSet;
use std::sync::Arc;

use crate::api::DiagnosticSet;
use crate::data_model::CfdDataModel;
use crate::project::Project;
use coflow_language::cft::{CftModuleSet, CftSchema};

use crate::checks::CheckDiagnosticStore;
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

/// Opens a project into an editor session with declarative check reports.
///
/// 仅加载项目选定的 CFD 来源；维度覆盖不会生成或修改数据文件。
///
/// # Errors
///
/// Returns unrecoverable project/config/schema I/O errors. User-fixable
/// project, schema, loader, model, and check problems are captured in the
/// returned session diagnostics.
pub(crate) fn open_project_session(project: Project) -> Result<ProjectSession, DiagnosticSet> {
    finish_project_session(open_schema_session(project)?, &[], true).map(|output| output.session)
}

// CLI 检查与代码生成只验证声明和 CFD 数据，不执行编辑器业务规则。
pub(crate) fn open_data_session(project: Project) -> Result<ProjectSession, DiagnosticSet> {
    finish_project_session(open_schema_session(project)?, &[], false).map(|output| output.session)
}

pub(crate) fn open_project_session_with_source_overrides(
    project: Project,
    source_overrides: &[DataSourceTextOverride],
) -> Result<ProjectSession, DiagnosticSet> {
    finish_project_session(open_schema_session(project)?, source_overrides, true)
        .map(|output| output.session)
}

pub(crate) fn open_project_session_from_schema(
    schema_session: ProjectSchemaSession,
) -> Result<ProjectSession, DiagnosticSet> {
    finish_project_session(schema_session, &[], true).map(|output| output.session)
}

pub(crate) struct SessionBuildOutput {
    pub(crate) session: ProjectSession,
    pub(crate) project_file_updates: Vec<crate::ProjectFileUpdate>,
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
        source_overrides: source_overrides.to_vec(),
    };
    let project_file_updates = ctx.project.include_data_files(
        source_overrides
            .iter()
            .filter(|item| !item.deleted)
            .map(|item| item.normalized_path.clone()),
    )?;
    let mut diagnostics = DiagnosticsStore::empty();
    let LoadedSessionData {
        model,
        indexes,
        source_data,
        check_state,
        execution_stats,
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
        project_file_updates,
    })
}

fn finish_project_session(
    schema_session: ProjectSchemaSession,
    source_overrides: &[DataSourceTextOverride],
    run_checks: bool,
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

    let mut ctx = SessionBuildContext {
        project,
        modules,
        schema,
        source_overrides: source_overrides.to_vec(),
    };

    let LoadedSessionData {
        model,
        indexes,
        source_data,
        check_state,
        execution_stats,
    } = if diagnostics.is_empty() {
        build_data_pipeline(&mut ctx, &mut diagnostics, run_checks)?
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
        project_file_updates: Vec::new(),
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
    source_overrides: Vec<DataSourceTextOverride>,
}

struct LoadedSessionData {
    model: CfdDataModel,
    indexes: SessionIndexes,
    source_data: SourceDataCache,
    check_state: CheckDiagnosticStore,
    execution_stats: ProjectExecutionStats,
}

impl LoadedSessionData {
    fn empty(schema: &CftSchema) -> Result<Self, DiagnosticSet> {
        Ok(Self {
            model: empty_model(schema)?,
            indexes: SessionIndexes::default(),
            source_data: SourceDataCache::default(),
            check_state: CheckDiagnosticStore::default(),
            execution_stats: ProjectExecutionStats::default(),
        })
    }
}

fn build_data_pipeline(
    ctx: &mut SessionBuildContext,
    diagnostics: &mut DiagnosticsStore,
    run_checks: bool,
) -> Result<LoadedSessionData, DiagnosticSet> {
    let (output, indexes) = match load_data(ctx, run_checks) {
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
    let check_impact = impact.check_impact(&ctx.schema);
    let (output, indexes) = match load_cached_data(
        ctx,
        &previous.source_data,
        CachedLoadOptions {
            reload_paths: &impact.affected_files,
            run_checks: true,
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
    previous_checks: Option<&'a CheckDiagnosticStore>,
    check_impact: &'a crate::checks::impact::CheckImpact,
}

fn load_cached_data(
    ctx: &SessionBuildContext,
    previous: &SourceDataCache,
    options: CachedLoadOptions<'_>,
) -> Result<(ProjectLoadOutput, SessionIndexBuilder), Box<DataLoadFailure>> {
    let mut indexes = SessionIndexBuilder::default();
    let pending = previous.with_pending_sources(&ctx.project, &ctx.source_overrides);
    let output = match reload_project_data_from_cache(
        &ctx.schema,
        &mut indexes,
        &pending,
        options.reload_paths,
        ReloadProjectDataOptions {
            load: LoadProjectDataOptions {
                run_checks: options.run_checks,
            },
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

struct DataLoadFailure {
    diagnostics: LoadDiagnostics,
    indexes: SessionIndexBuilder,
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
