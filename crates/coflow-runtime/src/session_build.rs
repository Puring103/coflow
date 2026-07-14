use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use coflow_api::{DiagnosticSet, ProviderRegistry};
use coflow_cft::CftContainer;
use coflow_data_model::CfdDataModel;
use coflow_project::Project;

use crate::checks::CheckState;
use crate::dimensions;
use crate::dimensions::{DimensionField, DimensionGenerationTransaction};
use crate::indexes::{DiagnosticsStore, SessionIndexBuilder, SessionIndexes};
use crate::load::{
    empty_load_output, empty_model, load_project_data, reload_project_data_from_cache,
    LoadDiagnostics, LoadProjectDataOptions, ProjectLoadOutput, SourceDataCache,
};
use crate::schema_build::build_project_schema_with_diagnostics;
use crate::session::{ProjectSchemaSession, ProjectSession};
use crate::writes::MutationImpact;

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
    impact: &MutationImpact,
) -> Result<SessionBuildOutput, DiagnosticSet> {
    let dimension_fields = dimensions::dimension_fields(session.compiled_schema());
    let ctx = SessionBuildContext {
        project: session.project.clone(),
        schema: session.schema.clone(),
        registry,
        dimension_mode: DimensionBuildMode::Generate,
        dimension_fields,
    };
    let mut diagnostics = DiagnosticsStore::empty();
    let LoadedSessionData {
        model,
        indexes,
        source_data,
        check_state,
        changed_dimension_paths,
    } = rebuild_data_pipeline(&ctx, session, impact, &mut diagnostics)?;
    Ok(SessionBuildOutput {
        session: assemble_session(ctx, model, diagnostics, indexes, source_data, check_state),
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
        source_data,
        check_state,
        changed_dimension_paths,
    } = if diagnostics.is_empty() {
        build_data_pipeline(&ctx, &mut diagnostics)?
    } else {
        LoadedSessionData::empty()?
    };

    Ok(SessionBuildOutput {
        session: assemble_session(ctx, model, diagnostics, indexes, source_data, check_state),
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
    source_data: SourceDataCache,
    check_state: CheckState,
    changed_dimension_paths: Vec<PathBuf>,
}

impl LoadedSessionData {
    fn empty() -> Result<Self, DiagnosticSet> {
        Ok(Self {
            model: empty_model()?,
            indexes: SessionIndexes::default(),
            source_data: SourceDataCache::default(),
            check_state: CheckState::default(),
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
                source_data: SourceDataCache::default(),
                check_state: CheckState::default(),
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
        source_data: output.source_data,
        check_state: output.check_state,
        changed_dimension_paths: dimensions.changed_paths,
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
                model: empty_model()?,
                indexes: load_failure.indexes.finalize_rejected(),
                source_data: SourceDataCache::default(),
                check_state: CheckState::default(),
                changed_dimension_paths: Vec::new(),
            });
        }
    };

    let mut dimensions = commit_dimensions_if_needed(ctx, &output, diagnostics);
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
            dimensions
                .changed_paths
                .iter()
                .map(|path| project_display_path(&ctx.project, path)),
        );
        match load_cached_data(
            ctx,
            &cache,
            &dimension_reload_paths,
            true,
            true,
            true,
            (!impact.structural_change).then_some(&previous.check_state),
            &impact.changed_records,
        ) {
            Ok(loaded) => (output, indexes) = loaded,
            Err(load_failure) => {
                diagnostics.extend_with_logical_locations(
                    load_failure.diagnostics.diagnostics,
                    load_failure.diagnostics.logical_locations,
                );
                output = empty_load_output()?;
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
        ctx.schema.compiled_schema(),
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
                source_data: SourceDataCache::default(),
                check_state: CheckState::default(),
                changed_dimension_paths: Vec::new(),
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
    source_data: SourceDataCache,
    check_state: CheckState,
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
        source_data,
        check_state,
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
