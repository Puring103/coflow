use coflow_api::{
    map_diagnostics_with_origins, origins_of, CfdInputRecord, CftContainer, Diagnostic,
    DiagnosticSet, Label, LoadedSource, ProjectSourceRef, ProviderRegistry, RecordOrigin,
    ResolvedSource, Severity, SourceLoadContext, SourceLocation, SourceLocationSpec,
    SourceProviderSelectionError, SourceResolveContext,
};
use coflow_cft::CftSchemaView;
use coflow_checker::run_checks_for_dimensions;
use coflow_data_model::{CfdDataModel, CfdDiagnostics, CfdPath, CfdPathSegment, CfdRecordId};
use coflow_project::{path_to_slash, Project, SourceConfig};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;
use std::sync::Arc;

use crate::dimensions;
use crate::indexes::{
    DiagnosticLogicalLocation, FileIndex, PendingRecordRef, RecordIndex, SourceId, SourceIndex,
};
use crate::session::RecordCoordinate;

type ResolvedLoaderSource = (Arc<dyn coflow_api::SourceProvider>, ResolvedSource);

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
    registry: &ProviderRegistry,
    sources: &mut SourceIndex,
    records_index: &mut RecordIndex,
    files: &mut FileIndex,
    options: LoadProjectDataOptions,
) -> Result<ProjectLoadOutput, LoadDiagnostics> {
    let mut records: Vec<CfdInputRecord> = Vec::new();
    let mut diagnostics = DiagnosticSet::empty();

    for source in &project.config.sources {
        let configured = configured_source(project, source);
        let resolved_sources = match resolve_sources(project, schema, registry, source, &configured)
        {
            Ok(resolved_sources) => resolved_sources,
            Err(err) => {
                diagnostics.extend(err);
                continue;
            }
        };

        diagnostics.extend(load_resolved_sources(
            project,
            schema,
            sources,
            records_index,
            files,
            &mut records,
            resolved_sources,
        ));
    }

    if options.include_implicit_dimension_sources {
        let view = CftSchemaView::new(schema);
        let dimension_fields = dimensions::dimension_fields(&view);
        for configured in dimensions::dimension_sources(project, &dimension_fields) {
            let resolved_sources =
                match resolve_implicit_source(project, schema, registry, &configured) {
                    Ok(resolved_sources) => resolved_sources,
                    Err(err) => {
                        diagnostics.extend(err);
                        continue;
                    }
                };
            diagnostics.extend(load_resolved_sources(
                project,
                schema,
                sources,
                records_index,
                files,
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
            records_index.finalize_rejected_pending();
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
        run_project_checks(project, schema, &model, &origins)
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
    schema: &CftContainer,
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

    for (loader, mut spec) in resolved_sources {
        if spec.provider_id.is_empty() {
            spec.provider_id = loader.descriptor().id.to_string();
        }
        let display_path = display_path_for(project, &spec);
        let source_id = SourceId(sources.entries.len());
        files.add_source_file(display_path.clone(), source_id);
        sources.push(crate::indexes::ResolvedSourceEntry {
            id: source_id,
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

fn resolve_implicit_source(
    project: &Project,
    schema: &CftContainer,
    registry: &ProviderRegistry,
    configured: &ResolvedSource,
) -> Result<Vec<ResolvedLoaderSource>, DiagnosticSet> {
    let ctx = SourceResolveContext {
        project_root: &project.root_dir,
        schema,
    };
    let option_keys = source_option_keys(&configured.options);
    let source_type =
        (!configured.provider_id.is_empty()).then_some(configured.provider_id.as_str());
    let source_ref = source_ref(configured, source_type, &option_keys);
    let loader = match registry.select_source_provider(&source_ref) {
        Ok(loader) => loader,
        Err(err) => {
            let mut diagnostics = DiagnosticSet::empty();
            diagnostics.push(loader_selection_diagnostic(
                &project.config_path,
                configured,
                err,
            ));
            return Err(diagnostics);
        }
    };
    Ok(loader
        .resolve(ctx, configured)?
        .into_iter()
        .map(|source| (Arc::clone(&loader), source))
        .collect())
}

fn run_project_checks(
    project: &Project,
    schema: &CftContainer,
    model: &CfdDataModel,
    origins: &[RecordOrigin],
) -> CheckOutput {
    let check_result = run_checks_for_dimensions(schema, model, &project.config.dimensions);
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

fn resolve_sources(
    project: &Project,
    schema: &CftContainer,
    registry: &ProviderRegistry,
    source: &SourceConfig,
    configured: &ResolvedSource,
) -> Result<Vec<ResolvedLoaderSource>, DiagnosticSet> {
    let ctx = SourceResolveContext {
        project_root: &project.root_dir,
        schema,
    };
    if source.source_type.is_none()
        && matches!(configured.location, SourceLocationSpec::Path(ref path) if path.is_dir())
    {
        let mut resolved = Vec::new();
        for loader in registry.source_providers() {
            for source in loader.resolve(ctx, configured)? {
                resolved.push((Arc::clone(&loader), source));
            }
        }
        return Ok(resolved);
    }

    let option_keys = source_option_keys(&configured.options);
    let source_ref = source_ref(configured, source.source_type.as_deref(), &option_keys);
    let loader = match registry.select_source_provider(&source_ref) {
        Ok(loader) => loader,
        Err(err) => {
            let mut diagnostics = DiagnosticSet::empty();
            diagnostics.push(loader_selection_diagnostic(
                &project.config_path,
                configured,
                err,
            ));
            return Err(diagnostics);
        }
    };
    Ok(loader
        .resolve(ctx, configured)?
        .into_iter()
        .map(|source| (Arc::clone(&loader), source))
        .collect())
}

const fn source_ref<'a>(
    source: &'a ResolvedSource,
    source_type: Option<&'a str>,
    option_keys: &'a [&'a str],
) -> ProjectSourceRef<'a> {
    ProjectSourceRef {
        source_type,
        location: &source.location,
        option_keys,
    }
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

fn configured_source(project: &Project, source: &SourceConfig) -> ResolvedSource {
    let location = match source.location() {
        SourceLocationSpec::Path(path) => SourceLocationSpec::Path(project.resolve_path(path)),
        SourceLocationSpec::Uri(uri) => SourceLocationSpec::Uri(uri.clone()),
    };
    let display_name = match source.location() {
        SourceLocationSpec::Path(path) => path.display().to_string(),
        SourceLocationSpec::Uri(uri) => uri.clone(),
    };
    ResolvedSource {
        provider_id: source.source_type.clone().unwrap_or_default(),
        location,
        options: source.options().clone(),
        display_name,
    }
}

#[must_use]
pub fn configured_project_source(project: &Project, source: &SourceConfig) -> ResolvedSource {
    configured_source(project, source)
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

fn source_option_keys(options: &Value) -> Vec<&str> {
    options
        .as_object()
        .map(|object| object.keys().map(String::as_str).collect())
        .unwrap_or_default()
}

fn loader_selection_diagnostic(
    config_path: &Path,
    spec: &ResolvedSource,
    err: SourceProviderSelectionError,
) -> Diagnostic {
    let source = match &spec.location {
        SourceLocationSpec::Path(path) => path.display().to_string(),
        SourceLocationSpec::Uri(uri) => uri.clone(),
    };
    match err {
        SourceProviderSelectionError::UnknownSourceProvider { id } => project_diagnostic(
            config_path,
            format!("source `{source}` uses unknown source provider `{id}`"),
        ),
        SourceProviderSelectionError::NoSourceProvider => project_diagnostic(
            config_path,
            format!("source `{source}` has no matching source provider"),
        ),
        SourceProviderSelectionError::AmbiguousSourceProviders { ids } => project_diagnostic(
            config_path,
            format!(
                "source `{source}` matches multiple source providers {}; set source `type` explicitly",
                ids.join(", ")
            ),
        ),
    }
}

fn project_diagnostic(config_path: &Path, message: impl Into<String>) -> Diagnostic {
    Diagnostic {
        code: "PROJECT-001".to_string(),
        stage: "PROJECT".to_string(),
        severity: Severity::Error,
        message: message.into(),
        primary: Some(Label {
            location: SourceLocation::ProjectConfig {
                path: config_path.to_path_buf(),
                key_path: Vec::new(),
            },
            message: None,
        }),
        related: Vec::new(),
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
