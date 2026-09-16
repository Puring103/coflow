use crate::api::{
    map_diagnostics_with_origins, CfdLoadContext, CfdSource, Diagnostic, DiagnosticSet,
};
use crate::cfd_loader::CfdLoader;
use crate::data_model::{
    CfdDataModel, CfdDiagnostics, CfdPath, CfdPathSegment, CfdRecordId, LoadedRecordDraft,
    RecordOrigin,
};
use crate::project::Project;
use coflow_core::schema::CftSchema;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::Arc;

use crate::checks::{run_full_project_checks, ProjectCheckOutput};
use crate::indexes::{
    CfdSourceEntry, DiagnosticLogicalLocation, PendingRecordRef, RecordIndexBuilder,
    SessionIndexBuilder, SourceId,
};
use crate::source_resolution::{ResolvedLoaderSource, SourceResolver};
use crate::{ProjectExecutionStats, RecordCoordinate};

#[derive(Debug, Clone)]
pub(crate) struct ProjectLoadOutput {
    pub(crate) model: CfdDataModel,
    pub(crate) diagnostics: DiagnosticSet,
    pub(crate) logical_locations: BTreeMap<usize, DiagnosticLogicalLocation>,
    pub(crate) source_data: SourceDataCache,
    pub(crate) statistics: ProjectExecutionStats,
}

#[derive(Debug, Clone)]
pub struct DataSourceTextOverride {
    pub normalized_path: PathBuf,
    pub source: String,
    pub deleted: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SourceDataCache {
    batches: Vec<CachedSourceBatch>,
}

#[derive(Debug, Clone)]
struct CachedSourceBatch {
    entry: CfdSourceEntry,
    source: Arc<str>,
    records: Arc<[LoadedRecordDraft]>,
}

impl SourceDataCache {
    pub(crate) fn sources(&self) -> impl Iterator<Item = (&str, &str)> {
        self.batches
            .iter()
            .map(|batch| (batch.entry.display_path.as_str(), batch.source.as_ref()))
    }
}

#[derive(Debug)]
pub(crate) struct LoadDiagnostics {
    pub(crate) diagnostics: DiagnosticSet,
    pub(crate) logical_locations: BTreeMap<usize, DiagnosticLogicalLocation>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LoadProjectDataOptions {
    pub(crate) run_checks: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct ReloadProjectDataOptions<'a> {
    pub(crate) load: LoadProjectDataOptions,
    pub(crate) source_overrides: &'a [DataSourceTextOverride],
}

struct LoadState<'a> {
    indexes: &'a mut SessionIndexBuilder,
    records: Vec<LoadedRecordDraft>,
    source_data: SourceDataCache,
}

struct PartialModelBuild {
    model: CfdDataModel,
    diagnostics: DiagnosticSet,
    logical_locations: BTreeMap<usize, DiagnosticLogicalLocation>,
    accepted_origins: Vec<RecordOrigin>,
}

pub(crate) fn empty_load_output(schema: &CftSchema) -> Result<ProjectLoadOutput, DiagnosticSet> {
    Ok(ProjectLoadOutput {
        model: empty_model(schema)?,
        diagnostics: DiagnosticSet::empty(),
        logical_locations: BTreeMap::new(),
        source_data: SourceDataCache::default(),
        statistics: ProjectExecutionStats::default(),
    })
}

#[allow(clippy::too_many_lines)]
pub(crate) fn load_project_data(
    project: &Project,
    schema: &CftSchema,
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
    let resolver = SourceResolver::new(project);
    for source in &project.config().data {
        let configured = resolver.configured(source);
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

    let draft_record_count = state.records.len();
    let partial = build_partial_model(schema, &state.records)?;
    let model = partial.model;
    let origins = partial.accepted_origins;
    let mut model_logical_locations = partial.logical_locations;
    let mut model_diagnostics = diagnostics;
    let model_offset = model_diagnostics.diagnostics.len();
    model_diagnostics.extend(partial.diagnostics);
    model_logical_locations = model_logical_locations
        .into_iter()
        .map(|(index, location)| (model_offset + index, location))
        .collect();
    let check = if options.run_checks {
        run_full_project_checks(schema, &model, &origins)
    } else {
        ProjectCheckOutput {
            diagnostics: DiagnosticSet::empty(),
            logical_locations: BTreeMap::new(),
            statistics: coflow_core::check::CheckExecutionStats::default(),
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
        statistics,
    })
}

// 缓存重载需要统一维护来源批次、诊断与统计，保持单一事务流程便于验证状态一致性。
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub(crate) fn reload_project_data_from_cache(
    schema: &CftSchema,
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
                !is_deleted_override(batch.entry.source.location.path(), options.source_overrides)
            })
            .cloned()
            .collect(),
    };
    let mut diagnostics = DiagnosticSet::empty();
    let reload_indexes = source_data
        .batches
        .iter()
        .enumerate()
        .filter_map(|(index, batch)| {
            (reload_paths.contains(&batch.entry.display_path)
                || !previous.contains_source(&batch.entry))
            .then_some(index)
        })
        .collect::<Vec<_>>();
    statistics.sources_reloaded = reload_indexes.len();

    for index in reload_indexes {
        let batch = &mut source_data.batches[index];
        match CfdLoader::load_partial(
            CfdLoadContext {
                schema,
                source_text: source_override_text(&batch.entry.source, options.source_overrides),
            },
            &batch.entry.source,
        ) {
            Ok(loaded) => {
                diagnostics.extend(loaded.diagnostics);
                batch.source = loaded.source;
                batch.records = loaded.records.into();
            }
            Err(err) => {
                batch.records = Arc::default();
                diagnostics.extend(err);
            }
        }
    }

    build_output_from_cache(
        schema,
        indexes,
        source_data,
        &options,
        statistics,
        diagnostics,
    )
}

fn load_resolved_sources(
    project: &Project,
    schema: &CftSchema,
    state: &mut LoadState<'_>,
    resolved_sources: Vec<ResolvedLoaderSource>,
    source_overrides: &[DataSourceTextOverride],
) -> DiagnosticSet {
    let mut diagnostics = DiagnosticSet::empty();
    for resolved in resolved_sources {
        let spec = resolved.source;
        if is_deleted_override(spec.location.path(), source_overrides) {
            continue;
        }
        let display_path = display_path_for(project, &spec);
        let source_id = SourceId(state.indexes.sources.entries.len());
        state
            .indexes
            .files
            .add_source_file(display_path.clone(), source_id);
        let entry = CfdSourceEntry {
            source: spec.clone(),
            display_path: display_path.clone(),
        };
        state.indexes.sources.push(entry.clone());
        match CfdLoader::load_partial(
            CfdLoadContext {
                schema,
                source_text: source_override_text(&spec, source_overrides),
            },
            &spec,
        ) {
            Ok(batch) => {
                diagnostics.extend(batch.diagnostics);
                let cached_records: Arc<[LoadedRecordDraft]> = batch.records.into();
                push_loaded_records(
                    &mut state.records,
                    &mut state.indexes.records,
                    source_id,
                    &display_path,
                    &cached_records,
                );
                state.source_data.batches.push(CachedSourceBatch {
                    entry,
                    source: batch.source,
                    records: cached_records,
                });
            }
            Err(err) => diagnostics.extend(err),
        }
    }
    diagnostics
}

fn source_override_text<'a>(
    source: &CfdSource,
    overrides: &'a [DataSourceTextOverride],
) -> Option<&'a str> {
    let source_path = crate::project::normalize_path(source.location.path());
    overrides
        .iter()
        .rev()
        .find(|source_override| source_override.normalized_path == source_path)
        .map(|source_override| source_override.source.as_str())
}

fn is_deleted_override(path: &std::path::Path, overrides: &[DataSourceTextOverride]) -> bool {
    let normalized_path = crate::normalize_path(path);
    overrides.iter().rev().any(|source_override| {
        source_override.normalized_path == normalized_path && source_override.deleted
    })
}

fn push_loaded_records(
    records: &mut Vec<LoadedRecordDraft>,
    records_index: &mut RecordIndexBuilder,
    source_id: SourceId,
    display_path: &str,
    loaded_records: &[LoadedRecordDraft],
) {
    for record in loaded_records {
        records_index.push(PendingRecordRef {
            actual_type: record.actual_type.clone(),
            key: record.key.clone(),
            origin: record.origin.clone(),
            source_id,
            display_path: display_path.to_string(),
        });
        records.push(record.clone());
    }
}

impl SourceDataCache {
    fn contains_source(&self, entry: &CfdSourceEntry) -> bool {
        self.batches
            .iter()
            .any(|batch| batch.entry.source.location == entry.source.location)
    }

    /// 返回与给定规范化路径匹配的批次 display path。
    ///
    /// 用于按“宿主覆盖了哪些文件”精确选择需要重载的批次，其余文件复用缓存。
    pub(crate) fn display_paths_for_paths(
        &self,
        normalized_paths: &BTreeSet<PathBuf>,
    ) -> BTreeSet<String> {
        self.batches
            .iter()
            .filter(|batch| {
                normalized_paths.contains(&crate::project::normalize_path(
                    batch.entry.source.location.path(),
                ))
            })
            .map(|batch| batch.entry.display_path.clone())
            .collect()
    }
}

fn build_output_from_cache(
    schema: &CftSchema,
    indexes: &mut SessionIndexBuilder,
    source_data: SourceDataCache,
    options: &ReloadProjectDataOptions<'_>,
    mut statistics: ProjectExecutionStats,
    source_diagnostics: DiagnosticSet,
) -> Result<ProjectLoadOutput, LoadDiagnostics> {
    let mut records = Vec::new();
    for batch in &source_data.batches {
        let source_id = SourceId(indexes.sources.entries.len());
        indexes.sources.push(batch.entry.clone());
        indexes
            .files
            .add_source_file(batch.entry.display_path.clone(), source_id);
        push_loaded_records(
            &mut records,
            &mut indexes.records,
            source_id,
            &batch.entry.display_path,
            &batch.records,
        );
    }
    let draft_record_count = records.len();
    let partial = build_partial_model(schema, &records)?;
    let model = partial.model;
    let origins = partial.accepted_origins;
    let mut model_logical_locations = partial.logical_locations;
    let mut model_diagnostics = source_diagnostics;
    let model_offset = model_diagnostics.diagnostics.len();
    model_diagnostics.extend(partial.diagnostics);
    model_logical_locations = model_logical_locations
        .into_iter()
        .map(|(index, location)| (model_offset + index, location))
        .collect();
    let check = if options.load.run_checks {
        run_project_checks(schema, &model, &origins, &mut statistics)
    } else {
        ProjectCheckOutput {
            diagnostics: DiagnosticSet::empty(),
            logical_locations: BTreeMap::new(),
            statistics: coflow_core::check::CheckExecutionStats::default(),
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
        statistics,
    })
}

fn build_partial_model(
    schema: &CftSchema,
    records: &[LoadedRecordDraft],
) -> Result<PartialModelBuild, LoadDiagnostics> {
    // 只保留候选下标，成功路径下每条草稿仅克隆一次送入构建器；失败重试时
    // 按诊断剔除候选，不必先整体克隆一遍记录。
    let mut candidates = (0..records.len()).collect::<Vec<usize>>();
    let mut diagnostics = DiagnosticSet::empty();
    let mut logical_locations = BTreeMap::new();

    loop {
        let candidate_origins = candidates
            .iter()
            .map(|&index| records[index].origin.clone())
            .collect::<Vec<_>>();
        let candidate_coordinates = candidates
            .iter()
            .map(|&index| {
                RecordCoordinate::try_new(&records[index].actual_type, &records[index].key).ok()
            })
            .collect::<Vec<_>>();
        let mut builder = CfdDataModel::builder(schema)
            .with_structural_limits(crate::limits::RuntimeLimits::default().structural);
        for &index in &candidates {
            builder.add_loaded_record(records[index].clone());
        }
        match builder.build_partial() {
            Ok(output) => {
                let offset = diagnostics.diagnostics.len();
                let current_locations = logical_locations_from_cfd(&output.diagnostics, |id| {
                    candidate_coordinates.get(id.index()).cloned().flatten()
                });
                logical_locations.extend(
                    current_locations
                        .into_iter()
                        .map(|(index, location)| (offset + index, location)),
                );
                diagnostics.extend(map_diagnostics_with_origins(
                    output.diagnostics,
                    &candidate_origins,
                ));
                return Ok(PartialModelBuild {
                    model: output.model,
                    diagnostics,
                    logical_locations,
                    accepted_origins: candidate_origins,
                });
            }
            Err(error) => {
                let rejected = error
                    .diagnostics
                    .iter()
                    .flat_map(|diagnostic| {
                        diagnostic
                            .primary
                            .iter()
                            .chain(&diagnostic.related)
                            .filter_map(|label| label.record.map(CfdRecordId::index))
                    })
                    .collect::<BTreeSet<_>>();
                if rejected.is_empty() {
                    return Err(LoadDiagnostics {
                        diagnostics: map_diagnostics_with_origins(error, &candidate_origins),
                        logical_locations: BTreeMap::new(),
                    });
                }
                let offset = diagnostics.diagnostics.len();
                let current_locations = logical_locations_from_cfd(&error, |id| {
                    candidate_coordinates.get(id.index()).cloned().flatten()
                });
                logical_locations.extend(
                    current_locations
                        .into_iter()
                        .map(|(index, location)| (offset + index, location)),
                );
                diagnostics.extend(map_diagnostics_with_origins(error, &candidate_origins));
                let previous_len = candidates.len();
                candidates = candidates
                    .into_iter()
                    .enumerate()
                    .filter_map(|(index, candidate)| {
                        (!rejected.contains(&index)).then_some(candidate)
                    })
                    .collect();
                if candidates.len() == previous_len {
                    diagnostics.extend(runtime_invariant(
                        "partial model diagnostics did not identify a candidate record",
                    ));
                    return Err(LoadDiagnostics {
                        diagnostics,
                        logical_locations,
                    });
                }
            }
        }
    }
}

fn run_project_checks(
    schema: &CftSchema,
    model: &CfdDataModel,
    origins: &[RecordOrigin],
    _statistics: &mut ProjectExecutionStats,
) -> ProjectCheckOutput {
    run_full_project_checks(schema, model, origins)
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

fn runtime_invariant(message: impl Into<String>) -> DiagnosticSet {
    DiagnosticSet::one(Diagnostic::error("RUNTIME-INTERNAL", "RUNTIME", message))
}

fn display_path_for(project: &Project, source: &CfdSource) -> String {
    crate::project_path(project.root_dir(), source.location.path())
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
        .with_structural_limits(crate::limits::RuntimeLimits::default().structural)
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
