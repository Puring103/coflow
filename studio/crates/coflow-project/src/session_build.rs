use std::collections::BTreeSet;
use std::sync::Arc;

use crate::api::DiagnosticSet;
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

/// 从源码构建不可变项目快照；读写能力由公开会话包装类型约束。
///
/// # Errors
///
/// Returns unrecoverable project/config/schema I/O errors. User-fixable
/// project, schema, loader, model, and check problems are captured in the
/// returned session diagnostics.
pub(crate) fn open_project_session(project: Project) -> Result<ProjectSession, DiagnosticSet> {
    finish_project_session(open_schema_session(project)?, &[]).map(|output| output.session)
}

pub(crate) fn open_project_session_with_source_overrides(
    project: Project,
    source_overrides: &[DataSourceTextOverride],
) -> Result<ProjectSession, DiagnosticSet> {
    finish_project_session(open_schema_session(project)?, source_overrides)
        .map(|output| output.session)
}

pub(crate) fn open_project_session_from_schema(
    schema_session: ProjectSchemaSession,
) -> Result<ProjectSession, DiagnosticSet> {
    finish_project_session(schema_session, &[]).map(|output| output.session)
}

pub(crate) struct SessionBuildOutput {
    pub(crate) session: ProjectSession,
}

pub(crate) fn rebuild_project_session_from_generation(
    session: &ProjectSession,
    impact: &MutationImpact,
    source_overrides: &[DataSourceTextOverride],
) -> Result<SessionBuildOutput, DiagnosticSet> {
    let ctx = SessionBuildContext {
        project: session.project.clone(),
        modules: Arc::clone(&session.modules),
        schema: session.schema.clone(),
        dimension_plan: Arc::clone(&session.dimension_plan),
        source_overrides,
    };
    let mut diagnostics = DiagnosticsStore::empty();
    let LoadedSessionData {
        model,
        indexes,
        source_data,
        execution_stats,
    } = rebuild_data_pipeline(&ctx, session, impact, &mut diagnostics)?;
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
    source_overrides: &[DataSourceTextOverride],
) -> Result<SessionBuildOutput, DiagnosticSet> {
    let ProjectSchemaSession {
        project,
        modules,
        schema,
        mut diagnostics,
    } = schema_session;

    // 未完成的源码也可以保存和重新打开；无有效 schema 时只提供诊断与源码目录。
    // 空 schema 不暴露旧代际的记录，修复源码后再构建正常数据视图。
    let schema = match schema {
        Some(schema) => schema,
        None => Arc::new(coflow_core::schema::build_schema(
            &coflow_core::schema::parse_modules(std::iter::empty::<coflow_core::schema::CftFile>())
        ).map_err(|_| diagnostics.as_set().clone())?),
    };

    let dimension_plan = Arc::new(DimensionRuntimePlan::compile(&schema, &project));
    let ctx = SessionBuildContext {
        project,
        modules,
        schema,
        dimension_plan,
        source_overrides,
    };

    let LoadedSessionData {
        model,
        indexes,
        source_data,
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
            execution_stats,
        ),
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
    dimension_plan: Arc<DimensionRuntimePlan>,
    source_overrides: &'a [DataSourceTextOverride],
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
    ctx: &SessionBuildContext<'_>,
    diagnostics: &mut DiagnosticsStore,
) -> Result<LoadedSessionData, DiagnosticSet> {
    finish_data_load(ctx, diagnostics, load_data(ctx, true))
}

fn rebuild_data_pipeline(
    ctx: &SessionBuildContext<'_>,
    previous: &ProjectSession,
    impact: &MutationImpact,
    diagnostics: &mut DiagnosticsStore,
) -> Result<LoadedSessionData, DiagnosticSet> {
    finish_data_load(
        ctx,
        diagnostics,
        load_cached_data(
            ctx,
            &previous.source_data,
            CachedLoadOptions {
                reload_paths: &impact.affected_files,
                run_checks: true,
            },
        ),
    )
}

/// 首次加载与缓存重建共用诊断归属和索引发布规则。
fn finish_data_load(
    ctx: &SessionBuildContext<'_>,
    diagnostics: &mut DiagnosticsStore,
    result: Result<(ProjectLoadOutput, SessionIndexBuilder), Box<DataLoadFailure>>,
) -> Result<LoadedSessionData, DiagnosticSet> {
    let (output, indexes) = match result {
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
    ctx: &SessionBuildContext<'_>,
    run_checks: bool,
) -> Result<(ProjectLoadOutput, SessionIndexBuilder), Box<DataLoadFailure>> {
    let mut indexes = SessionIndexBuilder::default();
    let output = match load_project_data(
        &ctx.project,
        &ctx.schema,
        &mut indexes,
        LoadProjectDataOptions { run_checks },
        ctx.source_overrides,
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
    ctx: &SessionBuildContext<'_>,
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
            source_overrides: ctx.source_overrides,
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
    ctx: SessionBuildContext<'_>,
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
