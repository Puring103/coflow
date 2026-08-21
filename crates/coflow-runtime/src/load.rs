use crate::api::{
    map_diagnostics_with_origins, origins_of, Diagnostic, DiagnosticSet,
    DimensionSourceLoadRequest, DimensionSourceSchema, CfdSourceCatalog, ResolvedSource,
    SourceLoadContext, TableContext,
};
use coflow_cft::{CftSchema, RecordKey};
use coflow_data_model::{
    CfdDataModel, CfdDiagnostics, CfdPath, CfdPathSegment, CfdRecordId, DimensionValueDraft,
    LoadedRecordDraft, RecordOrigin,
};
use crate::project::{path_to_slash, Project};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::Arc;

use crate::checks::impact::CheckImpact;
use crate::checks::{
    run_full_project_checks, run_incremental_project_checks, CheckDiagnosticStore,
    ProjectCheckOutput,
};
use crate::dimensions;
use crate::indexes::{
    DiagnosticLogicalLocation, PendingRecordRef, RecordIndexBuilder, ResolvedSourceEntry,
    SessionIndexBuilder, SourceId,
};
use crate::source_resolution::{ResolvedDimensionSource, ResolvedLoaderSource, SourceResolver};
use crate::{ProjectExecutionStats, RecordCoordinate};

#[derive(Debug, Clone)]
pub(crate) struct ProjectLoadOutput {
    pub(crate) model: CfdDataModel,
    pub(crate) diagnostics: DiagnosticSet,
    pub(crate) logical_locations: BTreeMap<usize, DiagnosticLogicalLocation>,
    pub(crate) source_data: SourceDataCache,
    pub(crate) check_state: CheckDiagnosticStore,
    pub(crate) statistics: ProjectExecutionStats,
}

#[derive(Debug, Clone)]
pub struct DataSourceTextOverride {
    pub normalized_path: PathBuf,
    pub source: String,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SourceDataCache {
    batches: Vec<CachedSourceBatch>,
}

#[derive(Debug, Clone)]
struct CachedSourceBatch {
    entry: ResolvedSourceEntry,
    records: Arc<[LoadedRecordDraft]>,
    dimension_values: Arc<[DimensionValueDraft]>,
    dimension_field: Option<dimensions::DimensionField>,
}

impl SourceDataCache {
    pub(crate) fn dimension_sources(
        &self,
    ) -> impl Iterator<Item = (&ResolvedSourceEntry, &dimensions::DimensionField)> {
        self.batches.iter().filter_map(|batch| {
            batch
                .dimension_field
                .as_ref()
                .map(|field| (&batch.entry, field))
        })
    }

    pub(crate) fn base_with_previous_dimensions(&self, previous: &Self) -> Self {
        let mut batches = self.batches.clone();
        batches.extend(
            previous
                .batches
                .iter()
                .filter(|batch| batch.dimension_field.is_some())
                .cloned(),
        );
        Self { batches }
    }

    pub(crate) fn implicit_display_paths(&self) -> BTreeSet<String> {
        self.batches
            .iter()
            .filter(|batch| batch.dimension_field.is_some())
            .map(|batch| batch.entry.display_path.clone())
            .collect()
    }

    pub(crate) fn dimension_source(
        &self,
        declaring_type: &str,
        field: &str,
        dimension: &str,
    ) -> Option<&ResolvedSourceEntry> {
        self.batches.iter().find_map(|batch| {
            let binding = batch.dimension_field.as_ref()?;
            (binding.source_type.as_str() == declaring_type
                && binding.source_field.as_str() == field
                && binding.dimension.as_str() == dimension)
                .then_some(&batch.entry)
        })
    }
}

#[derive(Debug)]
pub(crate) struct LoadDiagnostics {
    pub(crate) diagnostics: DiagnosticSet,
    pub(crate) logical_locations: BTreeMap<usize, DiagnosticLogicalLocation>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LoadProjectDataOptions {
    pub(crate) include_implicit_dimension_sources: bool,
    pub(crate) run_checks: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct ReloadProjectDataOptions<'a> {
    pub(crate) load: LoadProjectDataOptions,
    pub(crate) refresh_implicit_dimension_sources: bool,
    pub(crate) previous_checks: Option<&'a CheckDiagnosticStore>,
    pub(crate) check_impact: &'a CheckImpact,
}

struct LoadState<'a> {
    indexes: &'a mut SessionIndexBuilder,
    records: Vec<LoadedRecordDraft>,
    source_data: SourceDataCache,
}

pub(crate) fn empty_load_output(schema: &CftSchema) -> Result<ProjectLoadOutput, DiagnosticSet> {
    Ok(ProjectLoadOutput {
        model: empty_model(schema)?,
        diagnostics: DiagnosticSet::empty(),
        logical_locations: BTreeMap::new(),
        source_data: SourceDataCache::default(),
        check_state: CheckDiagnosticStore::default(),
        statistics: ProjectExecutionStats::default(),
    })
}

#[allow(clippy::too_many_lines)]
pub(crate) fn load_project_data(
    project: &Project,
    schema: &CftSchema,
    dimension_plan: &dimensions::DimensionRuntimePlan,
    catalog: &CfdSourceCatalog,
    indexes: &mut SessionIndexBuilder,
    options: LoadProjectDataOptions,
    source_overrides: &[DataSourceTextOverride],
) -> Result<ProjectLoadOutput, LoadDiagnostics> {
    let mut statistics = ProjectExecutionStats::default();
    let mut state = LoadState {
        indexes,
        records: Vec::new(),
        source_data: SourceDataCache::default(),
    };
    let mut diagnostics = DiagnosticSet::empty();
    let resolver = SourceResolver::new(project, catalog);
    for (source_index, source) in project.config().data.iter().enumerate() {
        let configured = resolver.configured(source, Some(source_index));
        let resolved_sources = match resolver.resolve_for_load(source, &configured) {
            Ok(resolved_sources) => resolved_sources,
            Err(err) => {
                diagnostics.extend(err);
                continue;
            }
        };
        statistics.sources_resolved = statistics
            .sources_resolved
            .saturating_add(resolved_sources.len());

        diagnostics.extend(load_resolved_sources(
            project,
            schema,
            &mut state,
            resolved_sources,
            source_overrides,
        ));
    }

    if options.include_implicit_dimension_sources {
        match resolver.resolve_dimension_sources(dimension_plan) {
            Ok(resolved_sources) => {
                statistics.sources_resolved = statistics
                    .sources_resolved
                    .saturating_add(resolved_sources.len());
                for resolved_source in resolved_sources {
                    diagnostics.extend(load_resolved_dimension_source(
                        project,
                        schema,
                        catalog,
                        &mut state,
                        resolved_source,
                    ));
                }
            }
            Err(err) => diagnostics.extend(err),
        }
    }

    if !diagnostics.is_empty() {
        return Err(load_failure(diagnostics));
    }

    let origins: Vec<RecordOrigin> = origins_of(&state.records);
    let draft_record_count = state.records.len();
    let record_coordinates = state
        .records
        .iter()
        .map(|record| RecordCoordinate::try_new(&record.actual_type, &record.key).ok())
        .collect::<Vec<_>>();
    let mut builder = CfdDataModel::builder(schema);
    for record in state.records {
        builder.add_loaded_record(record);
    }
    for batch in &state.source_data.batches {
        builder.add_dimension_value_drafts(batch.dimension_values.iter().cloned());
    }
    let editable = match builder.build_editable() {
        Ok(output) => output,
        Err(err) => {
            let logical_locations = logical_locations_from_cfd(&err, |id| {
                record_coordinates.get(id.index()).cloned().flatten()
            });
            let diagnostics = map_diagnostics_with_origins(err, &origins);
            return Err(LoadDiagnostics {
                diagnostics,
                logical_locations,
            });
        }
    };
    let model = editable.model;
    let mut model_logical_locations = logical_locations_from_cfd(&editable.diagnostics, |id| {
        record_coordinates.get(id.index()).cloned().flatten()
    });
    let mut model_diagnostics = map_diagnostics_with_origins(editable.diagnostics, &origins);
    let check = if options.run_checks {
        run_full_project_checks(schema, &model, &origins)
    } else {
        ProjectCheckOutput {
            diagnostics: DiagnosticSet::empty(),
            logical_locations: BTreeMap::new(),
            state: CheckDiagnosticStore::default(),
            statistics: coflow_checker::CheckExecutionStats::default(),
        }
    };
    record_model_work(&mut statistics, draft_record_count, &model, &check);
    let check_offset = model_diagnostics.diagnostics.len();
    model_diagnostics.extend(check.diagnostics);
    model_logical_locations.extend(
        check
            .logical_locations
            .into_iter()
            .map(|(index, location)| (check_offset + index, location)),
    );
    Ok(ProjectLoadOutput {
        model,
        diagnostics: model_diagnostics,
        logical_locations: model_logical_locations,
        source_data: state.source_data,
        check_state: check.state,
        statistics,
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn reload_project_data_from_cache(
    project: &Project,
    schema: &CftSchema,
    dimension_plan: &dimensions::DimensionRuntimePlan,
    catalog: &CfdSourceCatalog,
    indexes: &mut SessionIndexBuilder,
    previous: &SourceDataCache,
    reload_paths: &BTreeSet<String>,
    options: ReloadProjectDataOptions<'_>,
) -> Result<ProjectLoadOutput, LoadDiagnostics> {
    let mut statistics = ProjectExecutionStats::default();
    let mut source_data = SourceDataCache {
        batches: previous
            .batches
            .iter()
            .filter(|batch| {
                options.load.include_implicit_dimension_sources || batch.dimension_field.is_none()
            })
            .cloned()
            .collect(),
    };
    if options.load.include_implicit_dimension_sources && options.refresh_implicit_dimension_sources
    {
        statistics.sources_resolved = refresh_dimension_source_plans(
            project,
            dimension_plan,
            catalog,
            previous,
            &mut source_data,
        )?;
    }

    let mut diagnostics = DiagnosticSet::empty();
    let ordinary_records = source_data
        .batches
        .iter()
        .flat_map(|batch| batch.records.iter().cloned())
        .collect::<Vec<_>>();
    let reload_indexes = source_data
        .batches
        .iter()
        .enumerate()
        .filter_map(|(index, batch)| {
            (reload_paths.contains(&batch.entry.display_path)
                || !previous.contains_source(&batch.entry, batch.dimension_field.as_ref()))
            .then_some(index)
        })
        .collect::<Vec<_>>();
    statistics.sources_reloaded = reload_indexes.len();

    diagnostics.extend(preflight_cached_sources(
        project,
        schema,
        catalog,
        &source_data,
        &reload_indexes,
    ));
    if !diagnostics.is_empty() {
        return Err(load_failure(diagnostics));
    }

    for index in reload_indexes {
        let batch = &mut source_data.batches[index];
        if let Some(field) = &batch.dimension_field {
            match load_dimension_batch(
                project,
                schema,
                catalog,
                &batch.entry.source,
                field,
                &ordinary_records,
            ) {
                Ok(values) => batch.dimension_values = values.into(),
                Err(err) => diagnostics.extend(err),
            }
            continue;
        }
        let Some(loader) = catalog.source_provider(&batch.entry.provider_id) else {
            diagnostics.push(missing_cached_provider(&batch.entry.provider_id));
            continue;
        };
        match loader.load(
            SourceLoadContext {
                project_root: project.root_dir(),
                schema,
                source_text: None,
            },
            &batch.entry.source,
        ) {
            Ok(source_data) => batch.records = source_data.records.into(),
            Err(err) => diagnostics.extend(err),
        }
    }
    if !diagnostics.is_empty() {
        return Err(LoadDiagnostics {
            diagnostics,
            logical_locations: BTreeMap::new(),
        });
    }

    build_output_from_cache(schema, indexes, source_data, &options, statistics)
}

fn preflight_cached_sources(
    project: &Project,
    schema: &CftSchema,
    catalog: &CfdSourceCatalog,
    source_data: &SourceDataCache,
    reload_indexes: &[usize],
) -> DiagnosticSet {
    let mut diagnostics = DiagnosticSet::empty();
    for index in reload_indexes {
        let batch = &source_data.batches[*index];
        if batch.dimension_field.is_some() {
            continue;
        }
        let Some(loader) = catalog.source_provider(&batch.entry.provider_id) else {
            diagnostics.push(missing_cached_provider(&batch.entry.provider_id));
            continue;
        };
        diagnostics.extend(loader.preflight(
            SourceLoadContext {
                project_root: project.root_dir(),
                schema,
                source_text: None,
            },
            &batch.entry.source,
        ));
    }
    diagnostics
}

const fn load_failure(diagnostics: DiagnosticSet) -> LoadDiagnostics {
    LoadDiagnostics {
        diagnostics,
        logical_locations: BTreeMap::new(),
    }
}

fn load_resolved_sources(
    project: &Project,
    schema: &CftSchema,
    state: &mut LoadState<'_>,
    resolved_sources: Vec<ResolvedLoaderSource>,
    source_overrides: &[DataSourceTextOverride],
) -> DiagnosticSet {
    let mut diagnostics = DiagnosticSet::empty();
    for resolved in &resolved_sources {
        diagnostics.extend(resolved.provider.preflight(
            SourceLoadContext {
                project_root: project.root_dir(),
                schema,
                source_text: source_override_text(&resolved.source, source_overrides),
            },
            &resolved.source,
        ));
    }
    if !diagnostics.is_empty() {
        return diagnostics;
    }

    for resolved in resolved_sources {
        let loader = resolved.provider;
        let spec = resolved.source;
        let display_path = display_path_for(project, &spec);
        let source_id = SourceId(state.indexes.sources.entries.len());
        state
            .indexes
            .files
            .add_source_file(display_path.clone(), source_id);
        let entry = ResolvedSourceEntry {
            provider_id: spec.provider_id.clone(),
            source: spec.clone(),
            display_path: display_path.clone(),
        };
        state.indexes.sources.push(entry.clone());
        match loader.load(
            SourceLoadContext {
                project_root: project.root_dir(),
                schema,
                source_text: source_override_text(&spec, source_overrides),
            },
            &spec,
        ) {
            Ok(batch) => {
                let cached_records: Arc<[LoadedRecordDraft]> = batch.records.into();
                push_loaded_records(
                    &mut state.records,
                    &mut state.indexes.records,
                    source_id,
                    &spec,
                    &display_path,
                    &cached_records,
                );
                state.source_data.batches.push(CachedSourceBatch {
                    entry,
                    records: cached_records,
                    dimension_values: Arc::default(),
                    dimension_field: None,
                });
            }
            Err(err) => diagnostics.extend(err),
        }
    }
    diagnostics
}

fn source_override_text<'a>(
    source: &ResolvedSource,
    overrides: &'a [DataSourceTextOverride],
) -> Option<&'a str> {
    let source_path = crate::project::normalize_path(source.location.path());
    overrides
        .iter()
        .rev()
        .find(|source_override| source_override.normalized_path == source_path)
        .map(|source_override| source_override.source.as_str())
}

fn load_resolved_dimension_source(
    project: &Project,
    schema: &CftSchema,
    catalog: &CfdSourceCatalog,
    state: &mut LoadState<'_>,
    resolved: ResolvedDimensionSource,
) -> DiagnosticSet {
    let mut diagnostics = DiagnosticSet::empty();
    let source = resolved.source;
    let display_path = display_path_for(project, &source);
    let entry = ResolvedSourceEntry {
        provider_id: source.provider_id.clone(),
        source: source.clone(),
        display_path,
    };
    let source_id = state.indexes.sources.get_or_insert_dimension(entry.clone());
    state
        .indexes
        .files
        .add_source_file(entry.display_path.clone(), source_id);
    for field in resolved.fields {
        match load_dimension_batch(project, schema, catalog, &source, &field, &state.records) {
            Ok(values) => state.source_data.batches.push(CachedSourceBatch {
                entry: entry.clone(),
                records: Arc::default(),
                dimension_values: values.into(),
                dimension_field: Some(field),
            }),
            Err(err) => diagnostics.extend(err),
        }
    }
    diagnostics
}

fn load_dimension_batch(
    project: &Project,
    schema: &CftSchema,
    catalog: &CfdSourceCatalog,
    source: &ResolvedSource,
    field: &dimensions::DimensionField,
    records: &[LoadedRecordDraft],
) -> Result<Vec<DimensionValueDraft>, DiagnosticSet> {
    let manager = catalog
        .dimension_source_manager(&source.provider_id)
        .ok_or_else(|| DiagnosticSet::one(missing_cached_provider(&source.provider_id)))?;
    let source_type = schema.resolve_type(&field.source_type).ok_or_else(|| {
        runtime_invariant(format!(
            "dimension source type `{}` disappeared before loading",
            field.source_type
        ))
    })?;
    let source_field = schema
        .field(&field.source_type, &field.source_field)
        .ok_or_else(|| {
            runtime_invariant(format!(
                "dimension source field `{}.{}` disappeared before loading",
                field.source_type, field.source_field
            ))
        })?;
    let dimension = schema.resolve_dimension(&field.dimension).ok_or_else(|| {
        runtime_invariant(format!(
            "dimension `{}` disappeared before loading",
            field.dimension
        ))
    })?;
    let mut values = manager
        .load_dimension_source(
            TableContext {
                project_root: project.root_dir(),
            },
            &DimensionSourceLoadRequest {
                source,
                schema: DimensionSourceSchema {
                    schema,
                    dimension,
                    source_type,
                    source_field,
                },
            },
        )?
        .values;
    if field.is_singleton {
        let key = records
            .iter()
            .find(|record| schema.is_assignable(&record.actual_type, &field.source_type))
            .and_then(|record| RecordKey::new(record.key.clone()).ok())
            .ok_or_else(|| {
                DiagnosticSet::one(Diagnostic::error(
                    "RUNTIME-DIMENSION-SINGLETON",
                    "RUNTIME",
                    format!(
                        "singleton dimension owner `{}` has no record",
                        field.source_type
                    ),
                ))
            })?;
        for value in &mut values {
            value.source_key = key.clone();
        }
    }
    Ok(values)
}

fn push_loaded_records(
    records: &mut Vec<LoadedRecordDraft>,
    records_index: &mut RecordIndexBuilder,
    source_id: SourceId,
    source: &ResolvedSource,
    display_path: &str,
    loaded_records: &[LoadedRecordDraft],
) {
    for record in loaded_records {
        records_index.push(PendingRecordRef {
            actual_type: record.actual_type.clone(),
            key: record.key.clone(),
            origin: record.origin.clone(),
            source_id,
            provider_id: source.provider_id.clone(),
            display_path: display_path.to_string(),
        });
        records.push(record.clone());
    }
}

impl SourceDataCache {
    fn contains_source(
        &self,
        entry: &ResolvedSourceEntry,
        dimension_field: Option<&dimensions::DimensionField>,
    ) -> bool {
        self.batches.iter().any(|batch| {
            batch.dimension_field.as_ref() == dimension_field
                && batch.entry.provider_id == entry.provider_id
                && batch.entry.source.location == entry.source.location
        })
    }
}

fn refresh_dimension_source_plans(
    project: &Project,
    dimension_plan: &dimensions::DimensionRuntimePlan,
    catalog: &CfdSourceCatalog,
    previous: &SourceDataCache,
    source_data: &mut SourceDataCache,
) -> Result<usize, LoadDiagnostics> {
    source_data
        .batches
        .retain(|batch| batch.dimension_field.is_none());
    let resolver = SourceResolver::new(project, catalog);
    let mut diagnostics = DiagnosticSet::empty();
    let mut resolved_count = 0;
    match resolver.resolve_dimension_sources(dimension_plan) {
        Ok(resolved_sources) => {
            resolved_count = resolved_sources.len();
            for resolved in resolved_sources {
                let source = resolved.source;
                let display_path = display_path_for(project, &source);
                let entry = ResolvedSourceEntry {
                    provider_id: source.provider_id.clone(),
                    source,
                    display_path,
                };
                for field in resolved.fields {
                    let dimension_values = previous
                        .batches
                        .iter()
                        .find(|batch| {
                            batch.dimension_field.as_ref() == Some(&field)
                                && batch.entry.provider_id == entry.provider_id
                                && batch.entry.source.location == entry.source.location
                        })
                        .map_or_else(Arc::default, |batch| Arc::clone(&batch.dimension_values));
                    source_data.batches.push(CachedSourceBatch {
                        entry: entry.clone(),
                        records: Arc::default(),
                        dimension_values,
                        dimension_field: Some(field),
                    });
                }
            }
        }
        Err(err) => diagnostics.extend(err),
    }
    if diagnostics.is_empty() {
        Ok(resolved_count)
    } else {
        Err(LoadDiagnostics {
            diagnostics,
            logical_locations: BTreeMap::new(),
        })
    }
}

fn build_output_from_cache(
    schema: &CftSchema,
    indexes: &mut SessionIndexBuilder,
    source_data: SourceDataCache,
    options: &ReloadProjectDataOptions<'_>,
    mut statistics: ProjectExecutionStats,
) -> Result<ProjectLoadOutput, LoadDiagnostics> {
    let mut records = Vec::new();
    for batch in &source_data.batches {
        let source_id = if batch.dimension_field.is_some() {
            indexes.sources.get_or_insert_dimension(batch.entry.clone())
        } else {
            let source_id = SourceId(indexes.sources.entries.len());
            indexes.sources.push(batch.entry.clone());
            source_id
        };
        indexes
            .files
            .add_source_file(batch.entry.display_path.clone(), source_id);
        if batch.dimension_field.is_none() {
            push_loaded_records(
                &mut records,
                &mut indexes.records,
                source_id,
                &batch.entry.source,
                &batch.entry.display_path,
                &batch.records,
            );
        }
    }
    let origins = origins_of(&records);
    let draft_record_count = records.len();
    let record_coordinates = records
        .iter()
        .map(|record| RecordCoordinate::try_new(&record.actual_type, &record.key).ok())
        .collect::<Vec<_>>();
    let mut builder = CfdDataModel::builder(schema);
    for record in records {
        builder.add_loaded_record(record);
    }
    for batch in &source_data.batches {
        builder.add_dimension_value_drafts(batch.dimension_values.iter().cloned());
    }
    let editable = builder.build_editable().map_err(|err| {
        let logical_locations = logical_locations_from_cfd(&err, |id| {
            record_coordinates.get(id.index()).cloned().flatten()
        });
        LoadDiagnostics {
            diagnostics: map_diagnostics_with_origins(err, &origins),
            logical_locations,
        }
    })?;
    let model = editable.model;
    let mut model_logical_locations = logical_locations_from_cfd(&editable.diagnostics, |id| {
        record_coordinates.get(id.index()).cloned().flatten()
    });
    let mut model_diagnostics = map_diagnostics_with_origins(editable.diagnostics, &origins);
    let check = if options.load.run_checks {
        run_cached_project_checks(
            schema,
            &model,
            &origins,
            options.previous_checks,
            options.check_impact,
            &mut statistics,
        )
    } else {
        ProjectCheckOutput {
            diagnostics: DiagnosticSet::empty(),
            logical_locations: BTreeMap::new(),
            state: CheckDiagnosticStore::default(),
            statistics: coflow_checker::CheckExecutionStats::default(),
        }
    };
    record_model_work(&mut statistics, draft_record_count, &model, &check);
    let check_offset = model_diagnostics.diagnostics.len();
    model_diagnostics.extend(check.diagnostics);
    model_logical_locations.extend(
        check
            .logical_locations
            .into_iter()
            .map(|(index, location)| (check_offset + index, location)),
    );
    Ok(ProjectLoadOutput {
        model,
        diagnostics: model_diagnostics,
        logical_locations: model_logical_locations,
        source_data,
        check_state: check.state,
        statistics,
    })
}

fn run_cached_project_checks(
    schema: &CftSchema,
    model: &CfdDataModel,
    origins: &[RecordOrigin],
    previous_checks: Option<&CheckDiagnosticStore>,
    check_impact: &CheckImpact,
    _statistics: &mut ProjectExecutionStats,
) -> ProjectCheckOutput {
    previous_checks.map_or_else(
        || run_full_project_checks(schema, model, origins),
        |previous| run_incremental_project_checks(schema, model, origins, previous, check_impact),
    )
}

fn record_model_work(
    statistics: &mut ProjectExecutionStats,
    draft_record_count: usize,
    model: &CfdDataModel,
    check: &ProjectCheckOutput,
) {
    statistics.draft_records_collected = statistics
        .draft_records_collected
        .saturating_add(draft_record_count);
    statistics.records_validated = statistics
        .records_validated
        .saturating_add(draft_record_count);
    statistics.records_materialized = statistics
        .records_materialized
        .saturating_add(model.record_count());
    statistics.ref_edges_rebuilt = statistics
        .ref_edges_rebuilt
        .saturating_add(model.ref_edges().count());
    statistics.check_roots_executed = statistics
        .check_roots_executed
        .saturating_add(check.statistics.executed_tasks);
    statistics.dimension_records_projected = statistics
        .dimension_records_projected
        .saturating_add(check.statistics.dimension_projected_records);
}

fn missing_cached_provider(provider_id: &str) -> Diagnostic {
    Diagnostic::error(
        "RUNTIME-SOURCE-CACHE",
        "RUNTIME",
        format!("cached source provider `{provider_id}` is no longer registered"),
    )
}

fn runtime_invariant(message: impl Into<String>) -> DiagnosticSet {
    DiagnosticSet::one(Diagnostic::error("RUNTIME-INTERNAL", "RUNTIME", message))
}

fn display_path_for(project: &Project, source: &ResolvedSource) -> String {
    let path = source.location.path();
    let relative = path
        .strip_prefix(project.root_dir())
        .unwrap_or(path.as_path());
    path_to_slash(relative)
}

pub(crate) fn logical_locations_from_cfd(
    diagnostics: &CfdDiagnostics,
    resolve_coordinate: impl Fn(CfdRecordId) -> Option<RecordCoordinate>,
) -> BTreeMap<usize, DiagnosticLogicalLocation> {
    diagnostics
        .diagnostics
        .iter()
        .enumerate()
        .filter_map(|(index, diagnostic)| {
            let primary = diagnostic.primary.as_ref()?;
            let coordinate = primary.record.and_then(&resolve_coordinate);
            let field_path =
                (!primary.path.segments.is_empty()).then(|| format_cfd_path(&primary.path));
            (coordinate.is_some() || field_path.is_some()).then_some((
                index,
                DiagnosticLogicalLocation {
                    actual_type: coordinate.as_ref().map(|c| c.actual_type.to_string()),
                    record_key: coordinate.map(|c| c.key.to_string()),
                    field_path,
                },
            ))
        })
        .collect()
}

/// Format a [`CfdPath`] as the dotted / bracketed string the editor uses
/// as a stable key.
///
/// Callers include the engine's own logical-location pipeline as well as
/// tauri graph-edge labels. Keep exactly one copy.
#[must_use]
pub fn format_cfd_path(path: &CfdPath) -> String {
    let mut out = String::new();
    for segment in &path.segments {
        match segment {
            CfdPathSegment::Field(name) => {
                if !out.is_empty() {
                    out.push('.');
                }
                out.push_str(name);
            }
            CfdPathSegment::Index(i) => {
                let _ = write!(out, "[{i}]");
            }
            CfdPathSegment::DictKey(key) => {
                let _ = write!(out, "[{key}]");
            }
        }
    }
    out
}

pub(crate) fn empty_model(schema: &CftSchema) -> Result<CfdDataModel, DiagnosticSet> {
    CfdDataModel::builder(schema)
        .build()
        .map_err(|_| runtime_invariant("empty model build failed"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn runtime_invariants_use_the_internal_diagnostic_family() {
        let diagnostics = super::runtime_invariant("injected invariant failure");
        assert_eq!(diagnostics.diagnostics[0].code, "RUNTIME-INTERNAL");
        assert_eq!(diagnostics.diagnostics[0].stage, "RUNTIME");
    }
}
