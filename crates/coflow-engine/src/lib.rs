//! Shared project runtime for Coflow hosts.

#![cfg_attr(
    not(test),
    deny(
        clippy::dbg_macro,
        clippy::expect_used,
        clippy::panic,
        clippy::panic_in_result_fn,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable,
        clippy::unwrap_used
    )
)]
#![allow(clippy::multiple_crate_versions)]

mod data_files;
mod data_patch;
mod data_read;
mod dimensions;
mod files;
mod indexes;
mod mutation;
mod records;
mod schema_build;
mod schema_inspect;
mod session;
mod write_rules;
mod writes;

pub use data_files::{
    create_data_file, sync_data_header, DataCreateFileOptions, DataFileReport,
    DataSyncHeaderOptions,
};
pub use data_patch::{
    DataPatchAppliedOp, DataPatchFailedOp, DataPatchOp, DataPatchReport, DataPatchRequest,
    PatchPathSegment, PatchRecordSelector,
};
pub use data_read::{
    data_get, data_list, data_sources, DataGetQuery, DataGetReport, DataListQuery, DataListReport,
    DataRecordInfo, DataRecordSummary, DataSourceInfo, DataSourcesReport,
};
pub use dimensions::{
    builtin_display_name as dimension_builtin_display_name, dimensions_for_project,
    resolved_display_name as dimension_resolved_display_name, DimensionFieldInfo, DimensionInfo,
};
pub use files::{DimensionGroup, FileTreeNode, FileTreeOptions};
pub use indexes::{
    DependencyIndex, DiagnosticLogicalLocation, DiagnosticsStore, FileIndex, RecordIndex,
    RecordRef, ResolvedSourceEntry, SourceId, SourceIndex,
};
// Re-export helpers that hosts (tauri editor, CLI) call when translating
// engine data to a wire format so they don't diverge in path formatting.
pub use self::format_cfd_path as format_field_path;
pub use mutation::{
    DefaultMaterialization, MutationAppliedOp, MutationFailedOp, MutationFields, MutationOp,
    MutationReport, MutationRequest, MutationValue, PreparedMutation,
};
pub use records::{RecordTarget, RecordView, WriteOutcome};
pub use schema_build::build_project_schema_session;
pub use schema_inspect::{
    inspect_schema, schema_files, SchemaAnnotation, SchemaAnnotationValueInfo, SchemaConstInfo,
    SchemaConstValueInfo, SchemaDefaultValueInfo, SchemaEnumInfo, SchemaEnumVariantInfo,
    SchemaFieldInfo, SchemaFileInfo, SchemaFilesReport, SchemaInspectReport, SchemaTypeInfo,
    SchemaTypeRefInfo,
};
pub use session::{ProjectSchemaSession, ProjectSession, RecordCoordinate};

use coflow_api::{
    map_diagnostics_with_origins, origins_of, CfdInputRecord, CftContainer, Diagnostic,
    DiagnosticSet, Label, LoadContext, LoadedRecords, LoaderSelectionError, ProjectSourceRef,
    ProviderRegistry, RecordOrigin, ResolvedSource, Severity, SourceLocation, SourceLocationSpec,
    SourceResolveContext,
};
use coflow_checker::run_checks_for_dimensions_with_deps;
use coflow_cft::CftSchemaView;
use coflow_data_model::{CfdDataModel, CfdDiagnostics, CfdPath, CfdPathSegment, CfdRecordId};
use coflow_project::{
    path_to_slash, Project, SourceConfig,
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;
use std::sync::Arc;

use indexes::{dependency_index_from_checker_graph, PendingRecordRef};
use schema_build::build_project_schema_with_diagnostics;

type ResolvedLoaderSource = (Arc<dyn coflow_api::DataLoader>, ResolvedSource);

/// Opens, loads, builds, and checks a project into a reusable runtime session.
///
/// # Errors
///
/// Returns unrecoverable project/config/schema I/O errors. User-fixable
/// project, schema, loader, model, and check problems are captured in the
/// returned session diagnostics.
pub fn build_project_session(
    project: Project,
    registry: &ProviderRegistry,
) -> Result<ProjectSession, String> {
    build_project_session_with_dimension_mode(project, registry, DimensionBuildMode::Generate)
}

/// Opens, loads, and checks a project without writing generated dimension
/// sources or other derived files.
///
/// # Errors
///
/// Returns unrecoverable project/config/schema I/O errors. User-fixable
/// project, schema, loader, model, and check problems are captured in the
/// returned session diagnostics.
pub fn build_project_session_read_only(
    project: Project,
    registry: &ProviderRegistry,
) -> Result<ProjectSession, String> {
    build_project_session_with_dimension_mode(project, registry, DimensionBuildMode::ReadOnly)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DimensionBuildMode {
    Generate,
    ReadOnly,
}

fn build_project_session_with_dimension_mode(
    project: Project,
    registry: &ProviderRegistry,
    dimension_mode: DimensionBuildMode,
) -> Result<ProjectSession, String> {
    let mut initial_diagnostics = project.schema_diagnostic_set();
    initial_diagnostics.extend(project.data_diagnostic_set());
    let schema_session = build_project_schema_with_diagnostics(project, initial_diagnostics)?;
    let ProjectSchemaSession {
        project,
        schema,
        mut diagnostics,
    } = schema_session;

    let mut sources = SourceIndex::default();
    let mut records = RecordIndex::default();
    let mut files = FileIndex::default();
    let schema_view = CftSchemaView::new(&schema);
    let dimension_fields = dimensions::dimension_fields(&schema_view);
    let (model, dependencies) = if diagnostics.is_empty() {
        match load_project_data(
            &project,
            &schema,
            registry,
            &mut sources,
            &mut records,
            &mut files,
            LoadProjectDataOptions {
                include_implicit_dimension_sources: false,
                run_checks: dimension_fields.is_empty(),
            },
        ) {
            Ok(mut output) => {
                let has_dimension_fields = !dimension_fields.is_empty();
                let should_generate_dimensions =
                    dimension_mode == DimensionBuildMode::Generate && has_dimension_fields;
                let mut dimension_transaction = None;
                if should_generate_dimensions {
                    let dimension_result = dimensions::regenerate_dimension_sources(
                        &project,
                        &output.model,
                        &dimension_fields,
                        registry,
                    );
                    diagnostics.extend(dimension_result.diagnostics);
                    if diagnostics.is_empty() && !dimension_result.transaction.is_empty() {
                        dimension_transaction = Some(dimension_result.transaction);
                    }
                }
                if diagnostics.is_empty() && has_dimension_fields {
                    sources = SourceIndex::default();
                    records = RecordIndex::default();
                    files = FileIndex::default();
                    match load_project_data(
                        &project,
                        &schema,
                        registry,
                        &mut sources,
                        &mut records,
                        &mut files,
                        LoadProjectDataOptions {
                            include_implicit_dimension_sources: true,
                            run_checks: true,
                        },
                    ) {
                        Ok(reloaded) => output = reloaded,
                        Err(load_diagnostics) => {
                            diagnostics.extend_with_logical_locations(
                                load_diagnostics.diagnostics,
                                load_diagnostics.logical_locations,
                            );
                            output = empty_load_output()?;
                        }
                    }
                }
                records.finalize_with_model(&output.model);
                diagnostics
                    .extend_with_logical_locations(output.diagnostics, output.logical_locations);
                if !diagnostics.is_empty() {
                    if let Some(transaction) = dimension_transaction.take() {
                        diagnostics.extend(transaction.rollback(&project.config_path));
                    }
                }
                (output.model, output.dependencies)
            }
            Err(load_diagnostics) => {
                diagnostics.extend_with_logical_locations(
                    load_diagnostics.diagnostics,
                    load_diagnostics.logical_locations,
                );
                (empty_model()?, DependencyIndex::default())
            }
        }
    } else {
        (empty_model()?, DependencyIndex::default())
    };

    Ok(ProjectSession {
        project,
        schema,
        model,
        diagnostics,
        sources,
        records,
        files,
        dependencies,
        loader_extensions: loader_extensions(registry),
    })
}

fn loader_extensions(registry: &ProviderRegistry) -> BTreeSet<String> {
    let mut extensions = BTreeSet::new();
    for loader in registry.loaders() {
        for ext in loader.descriptor().extensions {
            extensions.insert((*ext).to_string());
        }
    }
    extensions
}

#[derive(Debug, Clone)]
struct ProjectLoadOutput {
    model: CfdDataModel,
    dependencies: DependencyIndex,
    diagnostics: DiagnosticSet,
    logical_locations: BTreeMap<usize, DiagnosticLogicalLocation>,
}

#[derive(Debug)]
struct CheckOutput {
    dependencies: DependencyIndex,
    diagnostics: DiagnosticSet,
    logical_locations: BTreeMap<usize, DiagnosticLogicalLocation>,
}

#[derive(Debug)]
struct LoadDiagnostics {
    diagnostics: DiagnosticSet,
    logical_locations: BTreeMap<usize, DiagnosticLogicalLocation>,
}

#[derive(Debug, Clone, Copy)]
struct LoadProjectDataOptions {
    include_implicit_dimension_sources: bool,
    run_checks: bool,
}

fn empty_load_output() -> Result<ProjectLoadOutput, String> {
    Ok(ProjectLoadOutput {
        model: empty_model()?,
        dependencies: DependencyIndex::default(),
        diagnostics: DiagnosticSet::empty(),
        logical_locations: BTreeMap::new(),
    })
}

fn load_project_data(
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
            dependencies: DependencyIndex::default(),
            diagnostics: DiagnosticSet::empty(),
            logical_locations: BTreeMap::new(),
        }
    };
    Ok(ProjectLoadOutput {
        model,
        dependencies: check.dependencies,
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
            LoadContext {
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
        sources.push(ResolvedSourceEntry {
            id: source_id,
            provider_id: spec.provider_id.clone(),
            source: spec.clone(),
            display_path: display_path.clone(),
        });
        match loader.load(
            LoadContext {
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
    let loader = match registry.select_loader(&source_ref) {
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
    let (check_result, dependencies) =
        run_checks_for_dimensions_with_deps(schema, model, &project.config.dimensions);
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
        dependencies: dependency_index_from_checker_graph(dependencies),
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
        for loader in registry.loaders() {
            for source in loader.resolve(ctx, configured)? {
                resolved.push((Arc::clone(&loader), source));
            }
        }
        return Ok(resolved);
    }

    let option_keys = source_option_keys(&configured.options);
    let source_ref = source_ref(configured, source.source_type.as_deref(), &option_keys);
    let loader = match registry.select_loader(&source_ref) {
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
    loaded: LoadedRecords,
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
    err: LoaderSelectionError,
) -> Diagnostic {
    let source = match &spec.location {
        SourceLocationSpec::Path(path) => path.display().to_string(),
        SourceLocationSpec::Uri(uri) => uri.clone(),
    };
    match err {
        LoaderSelectionError::UnknownLoader { id } => project_diagnostic(
            config_path,
            format!("source `{source}` uses unknown loader `{id}`"),
        ),
        LoaderSelectionError::NoLoader => project_diagnostic(
            config_path,
            format!("source `{source}` has no matching loader"),
        ),
        LoaderSelectionError::AmbiguousLoaders { ids } => project_diagnostic(
            config_path,
            format!(
                "source `{source}` matches multiple loaders {}; set source `type` explicitly",
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

fn empty_model() -> Result<CfdDataModel, String> {
    CfdDataModel::builder(&CftContainer::new())
        .build()
        .map_err(|_| "empty model build failed".to_string())
}
