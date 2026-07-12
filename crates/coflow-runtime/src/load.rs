use coflow_api::{
    map_diagnostics_with_origins, origins_of, Diagnostic, DiagnosticSet, LoadedSource,
    ProviderRegistry, ResolvedSource, SourceLoadContext, SourceLocationSpec,
};
use coflow_cft::{CftContainer, CompiledSchema};
use coflow_checker::{run_checks_for_dimensions, DimensionCheckPlan, DimensionCheckRound};
use coflow_data_model::{
    CfdDataModel, CfdDiagnostics, CfdInputRecord, CfdPath, CfdPathSegment, CfdRecordId,
    RecordOrigin,
};
use coflow_project::{path_to_slash, DimensionConfig, Project};
use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::dimensions;
use crate::indexes::{
    DiagnosticLogicalLocation, FileIndex, PendingRecordRef, RecordIndex, SessionIndexes, SourceId,
    SourceIndex,
};
use crate::session::RecordCoordinate;
use crate::source_resolution::{ResolvedLoaderSource, SourceResolver};

#[derive(Debug, Clone)]
pub(crate) struct ProjectLoadOutput {
    pub(crate) model: CfdDataModel,
    pub(crate) diagnostics: DiagnosticSet,
    pub(crate) logical_locations: BTreeMap<usize, DiagnosticLogicalLocation>,
}

#[derive(Debug)]
struct CheckOutput {
    diagnostics: DiagnosticSet,
    logical_locations: BTreeMap<usize, DiagnosticLogicalLocation>,
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

pub(crate) fn empty_load_output() -> Result<ProjectLoadOutput, DiagnosticSet> {
    Ok(ProjectLoadOutput {
        model: empty_model()?,
        diagnostics: DiagnosticSet::empty(),
        logical_locations: BTreeMap::new(),
    })
}

pub(crate) fn load_project_data(
    project: &Project,
    schema: &CftContainer,
    compiled_schema: &CompiledSchema,
    registry: &ProviderRegistry,
    indexes: &mut SessionIndexes,
    options: LoadProjectDataOptions,
) -> Result<ProjectLoadOutput, LoadDiagnostics> {
    let mut records: Vec<CfdInputRecord> = Vec::new();
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
            compiled_schema,
            &mut indexes.sources,
            &mut indexes.records,
            &mut indexes.files,
            &mut records,
            resolved_sources,
        ));
    }

    if options.include_implicit_dimension_sources {
        let dimension_fields = dimensions::dimension_fields(compiled_schema);
        for configured in dimensions::dimension_sources(project, &dimension_fields) {
            let resolved_sources = match resolver.resolve_implicit(&configured) {
                Ok(resolved_sources) => resolved_sources,
                Err(err) => {
                    diagnostics.extend(err);
                    continue;
                }
            };
            diagnostics.extend(load_resolved_sources(
                project,
                compiled_schema,
                &mut indexes.sources,
                &mut indexes.records,
                &mut indexes.files,
                &mut records,
                resolved_sources,
            ));
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
        .map(|record| RecordCoordinate::new(record.actual_type.clone(), record.key.clone()))
        .collect::<Vec<_>>();
    let mut builder = CfdDataModel::builder(schema);
    for record in records {
        builder.add_input_record(record);
    }
    let model = match builder.build() {
        Ok(model) => model,
        Err(err) => {
            indexes.records.finalize_rejected_pending();
            let logical_locations =
                logical_locations_from_cfd(&err, |id| record_coordinates.get(id.index()).cloned());
            let diagnostics = map_diagnostics_with_origins(err, &origins);
            return Err(LoadDiagnostics {
                diagnostics,
                logical_locations,
            });
        }
    };
    let check = if options.run_checks {
        run_project_checks(project, compiled_schema, &model, &origins)
    } else {
        CheckOutput {
            diagnostics: DiagnosticSet::empty(),
            logical_locations: BTreeMap::new(),
        }
    };
    Ok(ProjectLoadOutput {
        model,
        diagnostics: check.diagnostics,
        logical_locations: check.logical_locations,
    })
}

fn load_resolved_sources(
    project: &Project,
    schema: &CompiledSchema,
    sources: &mut SourceIndex,
    records_index: &mut RecordIndex,
    files: &mut FileIndex,
    records: &mut Vec<CfdInputRecord>,
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
        sources.push(crate::indexes::ResolvedSourceEntry {
            provider_id: spec.provider_id.clone(),
            source: spec.clone(),
            display_path: display_path.clone(),
        });
        match loader.load(
            SourceLoadContext {
                project_root: &project.root_dir,
                schema,
            },
            &spec,
        ) {
            Ok(batch) => push_loaded_records(
                records,
                records_index,
                source_id,
                &spec,
                &display_path,
                batch,
            ),
            Err(err) => diagnostics.extend(err),
        }
    }
    diagnostics
}

fn run_project_checks(
    project: &Project,
    schema: &CompiledSchema,
    model: &CfdDataModel,
    origins: &[RecordOrigin],
) -> CheckOutput {
    let plan = dimension_check_plan(&project.config.dimensions);
    let check_result = run_checks_for_dimensions(schema, model, &plan);
    let (diagnostics, logical_locations) = if let Err(checks) = check_result {
        let logical_locations = logical_locations_from_cfd(&checks, |id| {
            model
                .record(id)
                .map(|record| RecordCoordinate::new(record.actual_type(), record.key.clone()))
        });
        let diagnostics = map_diagnostics_with_origins(checks, origins);
        (diagnostics, logical_locations)
    } else {
        (DiagnosticSet::empty(), BTreeMap::new())
    };
    CheckOutput {
        diagnostics,
        logical_locations,
    }
}

fn dimension_check_plan(dimensions: &BTreeMap<String, DimensionConfig>) -> DimensionCheckPlan {
    DimensionCheckPlan::new(dimensions.iter().flat_map(|(dimension, config)| {
        config
            .variants
            .iter()
            .map(|variant| DimensionCheckRound::new(dimension.clone(), variant.clone()))
    }))
}

fn push_loaded_records(
    records: &mut Vec<CfdInputRecord>,
    records_index: &mut RecordIndex,
    source_id: SourceId,
    source: &ResolvedSource,
    display_path: &str,
    loaded: LoadedSource,
) {
    for record in loaded.records {
        records_index.push_pending(PendingRecordRef {
            coordinate: RecordCoordinate::new(record.actual_type.clone(), record.key.clone()),
            origin: record.origin.clone(),
            source_id,
            provider_id: source.provider_id.clone(),
            display_path: display_path.to_string(),
        });
        records.push(record);
    }
}

fn display_path_for(project: &Project, source: &ResolvedSource) -> String {
    match &source.location {
        SourceLocationSpec::Path(path) => {
            let relative = path
                .strip_prefix(&project.root_dir)
                .unwrap_or(path.as_path());
            path_to_slash(relative)
        }
        SourceLocationSpec::Uri(uri) => uri.clone(),
    }
}

fn logical_locations_from_cfd(
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
                    actual_type: coordinate.as_ref().map(|c| c.actual_type.clone()),
                    record_key: coordinate.map(|c| c.key),
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

pub(crate) fn empty_model() -> Result<CfdDataModel, DiagnosticSet> {
    CfdDataModel::builder(&CftContainer::new())
        .build()
        .map_err(|_| {
            DiagnosticSet::one(Diagnostic::error(
                "RUNTIME-INTERNAL",
                "RUNTIME",
                "empty model build failed",
            ))
        })
}
