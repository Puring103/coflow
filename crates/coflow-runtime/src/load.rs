use crate::api::{
    map_diagnostics_with_origins, CfdLoadContext, CfdSource, Diagnostic, DiagnosticSet,
};
use crate::cfd_loader::CfdLoader;
use crate::data_model::{
    CfdDataModel, CfdDiagnostics, CfdModelBuildStats, CfdPath, CfdPathSegment, CfdRecordHandle,
    DimensionValueDraft, LoadedRecordDraft, RecordOrigin,
};
use crate::project::Project;
use coflow_language::cft::{CftSchema, DimensionName, VariantName};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::Arc;

use crate::checks::impact::CheckImpact;
use crate::checks::{
    run_full_project_checks, run_incremental_project_checks,
    run_incremental_project_checks_with_changed_records, CheckDiagnosticStore, ProjectCheckOutput,
};
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
    pub(crate) check_state: CheckDiagnosticStore,
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
    model: Option<CfdDataModel>,
    record_index: Option<crate::indexes::RecordIndex>,
    changed_sources: BTreeSet<String>,
    diagnostic_order: Vec<CachedDiagnosticSlot>,
}

#[derive(Debug, Clone)]
enum CachedDiagnosticSlot {
    Discovery(DiagnosticSet),
    Source(String),
}

#[derive(Debug, Clone)]
struct CachedSourceBatch {
    entry: CfdSourceEntry,
    source: Arc<str>,
    records: Arc<[LoadedRecordDraft]>,
    dimension_values: Arc<[DimensionValueDraft]>,
    dimension_variants: Arc<[(DimensionName, VariantName)]>,
    diagnostics: Arc<DiagnosticSet>,
    loaded: bool,
}

impl SourceDataCache {
    pub(crate) fn cache_record_index(&mut self, index: &crate::indexes::RecordIndex) {
        self.record_index = Some(index.clone());
    }
    pub(crate) fn matches_overrides(&self, overrides: &[DataSourceTextOverride]) -> bool {
        overrides.iter().all(|source_override| {
            !source_override.deleted
                && self.batches.iter().any(|batch| {
                    batch.loaded
                        && crate::normalize_path(batch.entry.source.location.path())
                            == source_override.normalized_path
                        && batch.source.as_ref() == source_override.source
                })
        })
    }

    /// 新文件先进入候选缓存，再由常规重载解析；磁盘发布前即可校验完整模型。
    pub(crate) fn with_pending_sources(
        &self,
        project: &Project,
        overrides: &[DataSourceTextOverride],
    ) -> Self {
        let mut pending = self.clone();
        for source_override in overrides.iter().filter(|item| !item.deleted) {
            if pending.batches.iter().any(|batch| {
                crate::normalized_path_identity(batch.entry.source.location.path())
                    == crate::normalized_path_identity(&source_override.normalized_path)
            }) {
                continue;
            }
            let display_path =
                crate::project_path(project.root_dir(), &source_override.normalized_path);
            pending.batches.push(CachedSourceBatch {
                entry: CfdSourceEntry {
                    source: CfdSource {
                        location: crate::api::CfdSourcePath::new(
                            source_override.normalized_path.clone(),
                        ),
                        display_name: display_path.clone(),
                    },
                    display_path,
                },
                source: Arc::from(""),
                records: Arc::default(),
                dimension_values: Arc::default(),
                dimension_variants: Arc::default(),
                diagnostics: Arc::default(),
                loaded: false,
            });
            pending
                .diagnostic_order
                .push(CachedDiagnosticSlot::Source(crate::project_path(
                    project.root_dir(),
                    &source_override.normalized_path,
                )));
        }
        pending
    }

    pub(crate) fn sources(&self) -> impl Iterator<Item = (&str, &str)> {
        self.batches
            .iter()
            .map(|batch| (batch.entry.display_path.as_str(), batch.source.as_ref()))
    }

    pub(crate) fn dimension_files(
        &self,
        name: &str,
        directory: Option<&std::path::Path>,
    ) -> Vec<String> {
        self.batches
            .iter()
            .filter(|batch| {
                batch
                    .dimension_variants
                    .iter()
                    .any(|(dimension, _)| dimension.as_str() == name)
                    || directory.is_some_and(|dir| {
                        crate::path_is_same_or_descendant(batch.entry.source.location.path(), dir)
                    })
            })
            .map(|batch| batch.entry.display_path.clone())
            .collect()
    }

    pub(crate) fn dimension_only_files(&self, project: &Project) -> BTreeSet<&str> {
        self.batches
            .iter()
            .filter(|batch| {
                batch.records.is_empty()
                    && (!batch.dimension_variants.is_empty()
                        || project
                            .config()
                            .dimensions
                            .values()
                            .filter_map(|config| config.dir.as_ref())
                            .any(|dir| {
                                crate::path_is_same_or_descendant(
                                    batch.entry.source.location.path(),
                                    &project.resolve_path(dir),
                                )
                            }))
            })
            .map(|batch| batch.entry.display_path.as_str())
            .collect()
    }

    pub(crate) fn overlay_sources(&self) -> impl Iterator<Item = &CfdSourceEntry> {
        self.batches
            .iter()
            .filter(|batch| !batch.dimension_variants.is_empty())
            .map(|batch| &batch.entry)
    }

    pub(crate) fn dimension_source(
        &self,
        schema: &CftSchema,
        actual_type: &str,
        id: &str,
        field: &str,
        dimension: &str,
        variant: &str,
    ) -> Option<&CfdSourceEntry> {
        self.batches.iter().find_map(|batch| {
            batch
                .dimension_values
                .iter()
                .any(|value| {
                    value.source_id.as_str() == id
                        && value.field.as_str() == field
                        && value.dimension.as_str() == dimension
                        && value.variant.as_str() == variant
                        && schema.inheritance_root(value.source_type.as_str())
                            == schema.inheritance_root(actual_type)
                })
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
    pub(crate) run_checks: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct ReloadProjectDataOptions<'a> {
    pub(crate) load: LoadProjectDataOptions,
    pub(crate) previous_checks: Option<&'a CheckDiagnosticStore>,
    pub(crate) check_impact: &'a CheckImpact,
    pub(crate) source_overrides: &'a [DataSourceTextOverride],
}

struct LoadState<'a> {
    indexes: &'a mut SessionIndexBuilder,
    source_data: SourceDataCache,
}

struct PartialModelBuild {
    model: CfdDataModel,
    diagnostics: DiagnosticSet,
    logical_locations: BTreeMap<usize, DiagnosticLogicalLocation>,
    accepted_origins: Vec<RecordOrigin>,
    statistics: CfdModelBuildStats,
    changed_records: Option<Vec<CfdRecordHandle>>,
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
    indexes: &mut SessionIndexBuilder,
    options: LoadProjectDataOptions,
    source_overrides: &[DataSourceTextOverride],
) -> Result<ProjectLoadOutput, LoadDiagnostics> {
    let mut statistics = ProjectExecutionStats::default();
    let mut state = LoadState {
        indexes,
        source_data: SourceDataCache::default(),
    };
    let mut diagnostics = DiagnosticSet::empty();
    let resolver = SourceResolver::new(project);
    let mut resolved_paths = BTreeSet::new();
    for path in project.cfd_input_paths() {
        let is_dimension_dir = project
            .config()
            .dimensions
            .values()
            .any(|dimension| dimension.dir.as_deref() == Some(path));
        if is_dimension_dir && !project.resolve_path(path).exists() {
            continue;
        }
        let source = crate::SourceConfig::from_path(path.to_path_buf());
        let configured = resolver.configured(&source);
        let resolved_sources = match resolver.resolve_for_load(&source, &configured) {
            Ok(resolved_sources) => resolved_sources,
            Err(err) => {
                state
                    .source_data
                    .diagnostic_order
                    .push(CachedDiagnosticSlot::Discovery(err.clone()));
                diagnostics.extend(err);
                continue;
            }
        };
        // 维度目录可以同时被 data 目录包含，同一实际来源只加载一次。
        let resolved_sources = resolved_sources
            .into_iter()
            .filter(|resolved| {
                resolved_paths.insert(crate::normalized_path_identity(
                    resolved.source.location.path(),
                ))
            })
            .collect::<Vec<_>>();
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

    // 源批次已经拥有草稿；汇总只借用记录，不再复制完整值树。
    let records = state.source_data.record_drafts();
    let draft_record_count = records.len();
    let partial = build_partial_model(schema, &records, &state.source_data)?;
    let model_healthy = partial.diagnostics.is_empty();
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
            state: CheckDiagnosticStore::default(),
            statistics: coflow_checker::CheckExecutionStats::default(),
        }
    };
    record_model_work(
        &mut statistics,
        draft_record_count,
        partial.statistics,
        &check,
    );
    state.source_data.model = model_healthy.then(|| model.clone());
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

// 缓存重载需要统一维护来源批次、诊断与统计，保持单一事务流程便于验证状态一致性。
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub(crate) fn reload_project_data_from_cache(
    schema: &CftSchema,
    indexes: &mut SessionIndexBuilder,
    previous: &SourceDataCache,
    reload_paths: &BTreeSet<String>,
    options: ReloadProjectDataOptions<'_>,
) -> Result<ProjectLoadOutput, LoadDiagnostics> {
    reload_project_data_from_cache_with_preloaded(
        schema,
        indexes,
        previous,
        reload_paths,
        options,
        &mut BTreeMap::new(),
    )
}

pub(crate) fn reload_project_data_from_cache_with_preloaded(
    schema: &CftSchema,
    indexes: &mut SessionIndexBuilder,
    previous: &SourceDataCache,
    reload_paths: &BTreeSet<String>,
    options: ReloadProjectDataOptions<'_>,
    preloaded: &mut BTreeMap<PathBuf, crate::api::LoadedCfdSource>,
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
        model: previous.model.clone(),
        record_index: previous.record_index.clone(),
        changed_sources: BTreeSet::new(),
        diagnostic_order: previous.diagnostic_order.clone(),
    };

    let reload_indexes = source_data
        .batches
        .iter()
        .enumerate()
        .filter_map(|(index, batch)| {
            (reload_paths.contains(&batch.entry.display_path) || !batch.loaded).then_some(index)
        })
        .collect::<Vec<_>>();
    statistics.sources_reloaded = reload_indexes.len();

    for index in reload_indexes {
        let batch = &mut source_data.batches[index];
        source_data
            .changed_sources
            .insert(batch.entry.display_path.clone());

        let source_text = source_override_text(&batch.entry.source, options.source_overrides);
        let cached = preloaded
            .remove(&crate::normalize_path(batch.entry.source.location.path()))
            .filter(|loaded| source_text.is_some_and(|text| loaded.source.as_ref() == text));
        let loaded = cached.map_or_else(
            || {
                CfdLoader::load_partial(
                    CfdLoadContext {
                        schema,
                        source_text,
                    },
                    &batch.entry.source,
                )
            },
            Ok,
        );
        match loaded {
            Ok(loaded) => {
                batch.diagnostics = Arc::new(loaded.diagnostics);
                batch.source = loaded.source;
                batch.records = loaded.records.into();
                batch.dimension_values = loaded.dimensions.into();
                batch.dimension_variants = loaded.dimension_variants.into();
                batch.loaded = true;
            }
            Err(err) => {
                batch.records = Arc::default();
                batch.dimension_values = Arc::default();
                batch.dimension_variants = Arc::default();
                batch.diagnostics = Arc::new(err);
                batch.loaded = false;
            }
        }
    }

    // 诊断与源版本一起缓存，未变化的错误文件不能在其他文件修改后失去诊断。
    let mut diagnostics = DiagnosticSet::empty();
    let by_path = source_data
        .batches
        .iter()
        .map(|batch| (batch.entry.display_path.as_str(), batch))
        .collect::<BTreeMap<_, _>>();
    for slot in &source_data.diagnostic_order {
        match slot {
            CachedDiagnosticSlot::Discovery(error) => diagnostics.extend(error.clone()),
            CachedDiagnosticSlot::Source(path) => {
                if let Some(batch) = by_path.get(path.as_str()) {
                    diagnostics.extend((*batch.diagnostics).clone());
                }
            }
        }
    }
    let changed_records = source_data.changed_record_inputs(schema, previous);
    build_output_from_cache(
        schema,
        indexes,
        source_data,
        &options,
        statistics,
        diagnostics,
        changed_records.as_deref(),
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
        state
            .source_data
            .diagnostic_order
            .push(CachedDiagnosticSlot::Source(display_path.clone()));
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
                diagnostics.extend(batch.diagnostics.clone());
                let cached_records: Arc<[LoadedRecordDraft]> = batch.records.into();
                push_loaded_records(
                    &mut state.indexes.records,
                    source_id,
                    &display_path,
                    &cached_records,
                );
                state.source_data.batches.push(CachedSourceBatch {
                    entry,
                    source: batch.source,
                    records: cached_records,
                    dimension_values: batch.dimensions.into(),
                    dimension_variants: batch.dimension_variants.into(),
                    diagnostics: Arc::new(batch.diagnostics),
                    loaded: true,
                });
            }
            Err(err) => {
                diagnostics.extend(err.clone());
                state.source_data.batches.push(CachedSourceBatch {
                    entry,
                    source: Arc::from(""),
                    records: Arc::default(),
                    dimension_values: Arc::default(),
                    dimension_variants: Arc::default(),
                    diagnostics: Arc::new(err),
                    loaded: false,
                });
            }
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
    records_index: &mut RecordIndexBuilder,
    source_id: SourceId,
    display_path: &str,
    loaded_records: &[LoadedRecordDraft],
) {
    for record in loaded_records {
        records_index.push(PendingRecordRef {
            actual_type: record.actual_type.clone(),
            id: record.id.clone(),
            origin: record.origin.clone(),
            source_id,
            display_path: display_path.to_string(),
        });
    }
}

impl SourceDataCache {
    /// 身份与顺序稳定时，仅比较重新解析批次的业务值；未变批次直接共享。
    fn changed_record_inputs(
        &self,
        schema: &CftSchema,
        previous: &Self,
    ) -> Option<Vec<CfdRecordHandle>> {
        let model = previous.model.as_ref()?;
        if self.batches.len() != previous.batches.len() {
            return None;
        }
        let mut changed = BTreeSet::new();
        let mut offset = 0;
        for (current, old) in self.batches.iter().zip(&previous.batches) {
            if current.entry.display_path != old.entry.display_path
                || current.records.len() != old.records.len()
            {
                return None;
            }
            if !Arc::ptr_eq(&current.records, &old.records) {
                for (index, (input, prior)) in
                    current.records.iter().zip(old.records.iter()).enumerate()
                {
                    if input.id != prior.id || input.actual_type != prior.actual_type {
                        return None;
                    }
                    if !input.same_values(prior) {
                        let handle = model.record_by_type_id(&input.actual_type, &input.id)?;
                        if handle.index() != offset + index {
                            return None;
                        }
                        changed.insert(handle);
                    }
                }
            }
            offset += current.records.len();
            if !Arc::ptr_eq(&current.dimension_values, &old.dimension_values) {
                // 覆盖的增删和目标变化同时影响旧、新宿主；来源位置由模型独立刷新。
                let values_equal = current.dimension_values.len() == old.dimension_values.len()
                    && current
                        .dimension_values
                        .iter()
                        .zip(old.dimension_values.iter())
                        .all(|(a, b)| {
                            a.source_type == b.source_type
                                && a.source_id == b.source_id
                                && a.field == b.field
                                && a.dimension == b.dimension
                                && a.variant == b.variant
                                && a.value.same_value(&b.value)
                        });
                if !values_equal {
                    for value in current
                        .dimension_values
                        .iter()
                        .chain(old.dimension_values.iter())
                    {
                        changed.insert(model.lookup_assignable(
                            schema,
                            &value.source_type,
                            &value.source_id,
                        )?);
                    }
                }
            }
        }
        (offset == model.record_count()).then(|| changed.into_iter().collect())
    }

    fn record_drafts(&self) -> Vec<&LoadedRecordDraft> {
        self.batches
            .iter()
            .flat_map(|batch| batch.records.iter())
            .collect()
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

fn model_builder<'a>(
    schema: &'a CftSchema,
    records: &[&'a LoadedRecordDraft],
    source_data: &SourceDataCache,
) -> crate::data_model::CfdModelBuilder<'a> {
    let mut builder = CfdDataModel::builder(schema)
        .with_structural_limits(crate::limits::RuntimeLimits::default().structural);
    for record in records {
        builder.add_borrowed_record(record);
    }
    for batch in &source_data.batches {
        builder.add_dimension_value_drafts(batch.dimension_values.iter().cloned());
        for (dimension, variant) in batch.dimension_variants.iter() {
            builder.add_dimension_variant(dimension.clone(), variant.clone());
        }
    }
    builder
}

fn build_output_from_cache(
    schema: &CftSchema,
    indexes: &mut SessionIndexBuilder,
    mut source_data: SourceDataCache,
    options: &ReloadProjectDataOptions<'_>,
    mut statistics: ProjectExecutionStats,
    source_diagnostics: DiagnosticSet,
    changed_records: Option<&[CfdRecordHandle]>,
) -> Result<ProjectLoadOutput, LoadDiagnostics> {
    for batch in &source_data.batches {
        let source_id = SourceId(indexes.sources.entries.len());
        indexes.sources.push(batch.entry.clone());
        indexes
            .files
            .add_source_file(batch.entry.display_path.clone(), source_id);
    }
    let records = source_data.record_drafts();
    let draft_record_count = records.len();
    let build = || -> Result<PartialModelBuild, LoadDiagnostics> {
        Ok(match source_data.model.as_ref().zip(changed_records) {
            Some((previous, changed)) => {
                let builder = model_builder(schema, &records, &source_data);
                match builder.try_build_partial_incremental(previous, changed) {
                    Ok(Some(output)) => PartialModelBuild {
                        // 增量模型自带最新来源，诊断按实际标签查找，避免复制全项目路径。
                        accepted_origins: Vec::new(),
                        model: output.model,
                        diagnostics: DiagnosticSet::empty(),
                        logical_locations: BTreeMap::new(),
                        statistics: output.statistics,
                        changed_records: Some(output.changed_records),
                    },
                    // 完整容错构建负责诊断归并和被拒绝记录的恢复。
                    Ok(None) | Err(_) => build_partial_model(schema, &records, &source_data)?,
                }
            }
            None => build_partial_model(schema, &records, &source_data)?,
        })
    };
    let partial = match build() {
        Ok(partial) => partial,
        Err(error) => {
            // 全局构建失败也保留全部可定位记录，供容错会话展示被拒绝项。
            for (index, batch) in source_data.batches.iter().enumerate() {
                push_loaded_records(
                    &mut indexes.records,
                    SourceId(index),
                    &batch.entry.display_path,
                    &batch.records,
                );
            }
            return Err(error);
        }
    };
    let model_healthy = partial.diagnostics.is_empty();
    if partial.changed_records.is_some() {
        indexes.previous_records = source_data.record_index.clone();
    }
    for (index, batch) in source_data.batches.iter().enumerate() {
        if indexes.previous_records.is_none()
            || source_data
                .changed_sources
                .contains(&batch.entry.display_path)
        {
            push_loaded_records(
                &mut indexes.records,
                SourceId(index),
                &batch.entry.display_path,
                &batch.records,
            );
        }
    }
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
        run_cached_project_checks(
            schema,
            &model,
            &origins,
            options.previous_checks,
            options.check_impact,
            partial.changed_records.as_deref(),
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
    record_model_work(
        &mut statistics,
        draft_record_count,
        partial.statistics,
        &check,
    );
    source_data.model = model_healthy.then(|| model.clone());
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

fn build_partial_model(
    schema: &CftSchema,
    records: &[&LoadedRecordDraft],
    source_data: &SourceDataCache,
) -> Result<PartialModelBuild, LoadDiagnostics> {
    // 候选只保存下标；失败重试按诊断剔除记录，始终借用源批次的草稿。
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
                RecordCoordinate::try_new(&records[index].actual_type, &records[index].id).ok()
            })
            .collect::<Vec<_>>();
        let mut builder = CfdDataModel::builder(schema)
            .with_structural_limits(crate::limits::RuntimeLimits::default().structural);
        for &index in &candidates {
            builder.add_borrowed_record(records[index]);
        }
        for batch in &source_data.batches {
            builder.add_dimension_value_drafts(batch.dimension_values.iter().cloned());
            for (dimension, variant) in batch.dimension_variants.iter() {
                builder.add_dimension_variant(dimension.clone(), variant.clone());
            }
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
                    statistics: output.statistics,
                    changed_records: None,
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
                            .filter_map(|label| label.record.map(CfdRecordHandle::index))
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

fn run_cached_project_checks(
    schema: &CftSchema,
    model: &CfdDataModel,
    origins: &[RecordOrigin],
    previous_checks: Option<&CheckDiagnosticStore>,
    check_impact: &CheckImpact,
    changed_records: Option<&[CfdRecordHandle]>,
    _statistics: &mut ProjectExecutionStats,
) -> ProjectCheckOutput {
    previous_checks.map_or_else(
        || run_full_project_checks(schema, model, origins),
        |previous| match changed_records {
            Some(changed) => run_incremental_project_checks_with_changed_records(
                schema,
                model,
                origins,
                previous,
                check_impact,
                changed,
            ),
            None => run_incremental_project_checks(schema, model, origins, previous, check_impact),
        },
    )
}

fn record_model_work(
    statistics: &mut ProjectExecutionStats,
    draft_record_count: usize,
    model: CfdModelBuildStats,
    check: &ProjectCheckOutput,
) {
    statistics.draft_records_collected = statistics
        .draft_records_collected
        .saturating_add(draft_record_count);
    statistics.records_validated = statistics
        .records_validated
        .saturating_add(model.validated_records);
    statistics.records_materialized = statistics
        .records_materialized
        .saturating_add(model.materialized_records);
    statistics.records_reused = statistics
        .records_reused
        .saturating_add(model.reused_records);
    statistics.ref_edges_rebuilt = statistics
        .ref_edges_rebuilt
        .saturating_add(model.rebuilt_ref_edges);
    statistics.check_roots_executed = statistics
        .check_roots_executed
        .saturating_add(check.statistics.executed_tasks);
}

fn runtime_invariant(message: impl Into<String>) -> DiagnosticSet {
    DiagnosticSet::one(Diagnostic::error("RUNTIME-INTERNAL", "RUNTIME", message))
}

fn display_path_for(project: &Project, source: &CfdSource) -> String {
    crate::project_path(project.root_dir(), source.location.path())
}

pub(crate) fn logical_locations_from_cfd(
    diagnostics: &CfdDiagnostics,
    resolve_coordinate: impl Fn(CfdRecordHandle) -> Option<RecordCoordinate>,
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
                    record_id: coordinate.map(|c| c.id.to_string()),
                    field_path,
                },
            ))
        })
        .collect()
}

/// Format a [`CfdPath`] as the dotted / bracketed string the editor uses
/// as a stable id.
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
            CfdPathSegment::DictKey(id) => {
                let _ = write!(out, "[{id}]");
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
