use coflow_api::{
    map_diagnostics_with_origins, origins_of, Diagnostic, DiagnosticSet,
    DimensionSourceLoadRequest, DimensionSourceSchema, ProviderRegistry, ResolvedSource,
    SourceLoadContext, SourceLocationSpec, TableContext,
};
use coflow_cft::{CftSchema, RecordKey};
use coflow_data_model::{
    CfdDataModel, CfdDiagnostics, CfdPath, CfdPathSegment, CfdRecordId, DimensionValueDraft,
    LoadedRecordDraft, RecordOrigin,
};
use coflow_project::{path_to_slash, Project};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::sync::Arc;

use crate::checks::{
    run_full_project_checks, run_incremental_project_checks, CheckState, ProjectCheckOutput,
};
use crate::dimensions;
use crate::indexes::{
    DiagnosticLogicalLocation, FileIndex, PendingRecordRef, RecordIndexBuilder,
    ResolvedSourceEntry, SessionIndexBuilder, SourceId, SourceIndex,
};
use crate::source_resolution::{ResolvedLoaderSource, SourceResolver};
use crate::RecordCoordinate;

#[derive(Debug, Clone)]
pub(crate) struct ProjectLoadOutput {
    pub(crate) model: CfdDataModel,
    pub(crate) diagnostics: DiagnosticSet,
    pub(crate) logical_locations: BTreeMap<usize, DiagnosticLogicalLocation>,
    pub(crate) source_data: SourceDataCache,
    pub(crate) check_state: CheckState,
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

pub(crate) fn empty_load_output(schema: &CftSchema) -> Result<ProjectLoadOutput, DiagnosticSet> {
    Ok(ProjectLoadOutput {
        model: empty_model(schema)?,
        diagnostics: DiagnosticSet::empty(),
        logical_locations: BTreeMap::new(),
        source_data: SourceDataCache::default(),
        check_state: CheckState::default(),
    })
}

pub(crate) fn load_project_data(
    project: &Project,
    schema: &CftSchema,
    registry: &ProviderRegistry,
    indexes: &mut SessionIndexBuilder,
    options: LoadProjectDataOptions,
) -> Result<ProjectLoadOutput, LoadDiagnostics> {
    let mut records: Vec<LoadedRecordDraft> = Vec::new();
    let mut source_data = SourceDataCache::default();
    let mut diagnostics = DiagnosticSet::empty();
    let resolver = SourceResolver::new(project, registry);
    for (source_index, source) in project.config.sources.iter().enumerate() {
        let configured = resolver.configured(source, Some(source_index));
        let resolved_sources = match resolver.resolve_for_load(source, &configured) {
            Ok(resolved_sources) => resolved_sources,
            Err(err) => {
                diagnostics.extend(err);
                continue;
            }
        };

        diagnostics.extend(load_resolved_sources(
            project,
            schema,
            &mut indexes.sources,
            &mut indexes.records,
            &mut indexes.files,
            &mut records,
            &mut source_data,
            resolved_sources,
        ));
    }

    if options.include_implicit_dimension_sources {
        let dimension_fields = dimensions::dimension_fields(schema);
        match resolver.resolve_dimension_sources(&dimension_fields) {
            Ok(resolved_sources) => {
                for (resolved_source, field) in resolved_sources {
                    diagnostics.extend(load_resolved_dimension_sources(
                        project,
                        schema,
                        registry,
                        &mut indexes.sources,
                        &mut indexes.files,
                        &records,
                        &mut source_data,
                        vec![resolved_source],
                        &field,
                    ));
                }
            }
            Err(err) => diagnostics.extend(err),
        }
    }

    if !diagnostics.is_empty() {
        return Err(LoadDiagnostics {
            diagnostics,
            logical_locations: BTreeMap::new(),
        });
    }

    let origins: Vec<RecordOrigin> = origins_of(&records);
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
    let model = match builder.build() {
        Ok(model) => model,
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
    let check = if options.run_checks {
        run_full_project_checks(schema, &model, &origins)
    } else {
        ProjectCheckOutput {
            diagnostics: DiagnosticSet::empty(),
            logical_locations: BTreeMap::new(),
            state: CheckState::default(),
        }
    };
    Ok(ProjectLoadOutput {
        model,
        diagnostics: check.diagnostics,
        logical_locations: check.logical_locations,
        source_data,
        check_state: check.state,
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn reload_project_data_from_cache(
    project: &Project,
    schema: &CftSchema,
    registry: &ProviderRegistry,
    indexes: &mut SessionIndexBuilder,
    previous: &SourceDataCache,
    reload_paths: &BTreeSet<String>,
    options: LoadProjectDataOptions,
    refresh_implicit_dimension_sources: bool,
    previous_checks: Option<&CheckState>,
    changed_records: &BTreeSet<RecordCoordinate>,
) -> Result<ProjectLoadOutput, LoadDiagnostics> {
    let mut source_data = SourceDataCache {
        batches: previous
            .batches
            .iter()
            .filter(|batch| {
                options.include_implicit_dimension_sources || batch.dimension_field.is_none()
            })
            .cloned()
            .collect(),
    };
    if options.include_implicit_dimension_sources && refresh_implicit_dimension_sources {
        refresh_dimension_source_plans(project, schema, registry, previous, &mut source_data)?;
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

    for index in &reload_indexes {
        let batch = &source_data.batches[*index];
        if batch.dimension_field.is_some() {
            continue;
        }
        let Some(loader) = registry.source_provider(&batch.entry.provider_id) else {
            diagnostics.push(missing_cached_provider(&batch.entry.provider_id));
            continue;
        };
        diagnostics.extend(loader.preflight(
            SourceLoadContext {
                project_root: &project.root_dir,
                schema,
            },
            &batch.entry.source,
        ));
    }
    if !diagnostics.is_empty() {
        return Err(LoadDiagnostics {
            diagnostics,
            logical_locations: BTreeMap::new(),
        });
    }

    for index in reload_indexes {
        let batch = &mut source_data.batches[index];
        if let Some(field) = &batch.dimension_field {
            match load_dimension_batch(
                project,
                schema,
                registry,
                &batch.entry.source,
                field,
                &ordinary_records,
            ) {
                Ok(values) => batch.dimension_values = values.into(),
                Err(err) => diagnostics.extend(err),
            }
            continue;
        }
        let Some(loader) = registry.source_provider(&batch.entry.provider_id) else {
            diagnostics.push(missing_cached_provider(&batch.entry.provider_id));
            continue;
        };
        match loader.load(
            SourceLoadContext {
                project_root: &project.root_dir,
                schema,
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

    build_output_from_cache(
        schema,
        indexes,
        source_data,
        options.run_checks,
        previous_checks,
        changed_records,
    )
}

#[allow(clippy::too_many_arguments)]
fn load_resolved_sources(
    project: &Project,
    schema: &CftSchema,
    sources: &mut SourceIndex,
    records_index: &mut RecordIndexBuilder,
    files: &mut FileIndex,
    records: &mut Vec<LoadedRecordDraft>,
    source_data: &mut SourceDataCache,
    resolved_sources: Vec<ResolvedLoaderSource>,
) -> DiagnosticSet {
    let mut diagnostics = DiagnosticSet::empty();
    for (loader, spec) in &resolved_sources {
        diagnostics.extend(loader.preflight(
            SourceLoadContext {
                project_root: &project.root_dir,
                schema,
            },
            spec,
        ));
    }
    if !diagnostics.is_empty() {
        return diagnostics;
    }

    for (loader, spec) in resolved_sources {
        let display_path = display_path_for(project, &spec);
        let source_id = SourceId(sources.entries.len());
        files.add_source_file(display_path.clone(), source_id);
        let entry = ResolvedSourceEntry {
            provider_id: spec.provider_id.clone(),
            source: spec.clone(),
            display_path: display_path.clone(),
        };
        sources.push(entry.clone());
        match loader.load(
            SourceLoadContext {
                project_root: &project.root_dir,
                schema,
            },
            &spec,
        ) {
            Ok(batch) => {
                let cached_records: Arc<[LoadedRecordDraft]> = batch.records.into();
                push_loaded_records(
                    records,
                    records_index,
                    source_id,
                    &spec,
                    &display_path,
                    &cached_records,
                );
                source_data.batches.push(CachedSourceBatch {
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

#[allow(clippy::too_many_arguments)]
fn load_resolved_dimension_sources(
    project: &Project,
    schema: &CftSchema,
    registry: &ProviderRegistry,
    sources: &mut SourceIndex,
    files: &mut FileIndex,
    records: &[LoadedRecordDraft],
    source_data: &mut SourceDataCache,
    resolved_sources: Vec<ResolvedLoaderSource>,
    field: &dimensions::DimensionField,
) -> DiagnosticSet {
    let mut diagnostics = DiagnosticSet::empty();
    for (_, source) in resolved_sources {
        let display_path = display_path_for(project, &source);
        let entry = ResolvedSourceEntry {
            provider_id: source.provider_id.clone(),
            source: source.clone(),
            display_path,
        };
        let source_id = sources.get_or_insert_dimension(entry.clone());
        files.add_source_file(entry.display_path.clone(), source_id);
        match load_dimension_batch(project, schema, registry, &source, field, records) {
            Ok(values) => source_data.batches.push(CachedSourceBatch {
                entry,
                records: Arc::default(),
                dimension_values: values.into(),
                dimension_field: Some(field.clone()),
            }),
            Err(err) => diagnostics.extend(err),
        }
    }
    diagnostics
}

fn load_dimension_batch(
    project: &Project,
    schema: &CftSchema,
    registry: &ProviderRegistry,
    source: &ResolvedSource,
    field: &dimensions::DimensionField,
    records: &[LoadedRecordDraft],
) -> Result<Vec<DimensionValueDraft>, DiagnosticSet> {
    let manager = registry
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
                project_root: &project.root_dir,
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
    schema: &CftSchema,
    registry: &ProviderRegistry,
    previous: &SourceDataCache,
    source_data: &mut SourceDataCache,
) -> Result<(), LoadDiagnostics> {
    source_data
        .batches
        .retain(|batch| batch.dimension_field.is_none());
    let resolver = SourceResolver::new(project, registry);
    let dimension_fields = dimensions::dimension_fields(schema);
    let mut diagnostics = DiagnosticSet::empty();
    match resolver.resolve_dimension_sources(&dimension_fields) {
        Ok(resolved_sources) => {
            for ((_, source), field) in resolved_sources {
                let display_path = display_path_for(project, &source);
                let entry = ResolvedSourceEntry {
                    provider_id: source.provider_id.clone(),
                    source,
                    display_path,
                };
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
                    entry,
                    records: Arc::default(),
                    dimension_values,
                    dimension_field: Some(field),
                });
            }
        }
        Err(err) => diagnostics.extend(err),
    }
    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(LoadDiagnostics {
            diagnostics,
            logical_locations: BTreeMap::new(),
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn build_output_from_cache(
    schema: &CftSchema,
    indexes: &mut SessionIndexBuilder,
    source_data: SourceDataCache,
    run_checks: bool,
    previous_checks: Option<&CheckState>,
    changed_records: &BTreeSet<RecordCoordinate>,
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
    let model = builder.build().map_err(|err| {
        let logical_locations = logical_locations_from_cfd(&err, |id| {
            record_coordinates.get(id.index()).cloned().flatten()
        });
        LoadDiagnostics {
            diagnostics: map_diagnostics_with_origins(err, &origins),
            logical_locations,
        }
    })?;
    let check = if run_checks {
        previous_checks
            .and_then(|previous| {
                run_incremental_project_checks(
                    schema,
                    &model,
                    &origins,
                    previous,
                    changed_records,
                )
            })
            .unwrap_or_else(|| run_full_project_checks(schema, &model, &origins))
    } else {
        ProjectCheckOutput {
            diagnostics: DiagnosticSet::empty(),
            logical_locations: BTreeMap::new(),
            state: CheckState::default(),
        }
    };
    Ok(ProjectLoadOutput {
        model,
        diagnostics: check.diagnostics,
        logical_locations: check.logical_locations,
        source_data,
        check_state: check.state,
    })
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
    match &source.location {
        SourceLocationSpec::Path(path) => {
            let relative = path
                .strip_prefix(&project.root_dir)
                .unwrap_or(path.as_path());
            path_to_slash(relative)
        }
    }
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
