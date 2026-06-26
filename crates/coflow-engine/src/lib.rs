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

mod dimensions;
mod files;
mod records;
mod writes;

pub use dimensions::{
    builtin_display_name as dimension_builtin_display_name, dimensions_for_project,
    resolved_display_name as dimension_resolved_display_name, DimensionFieldInfo, DimensionInfo,
};
pub use files::{DimensionGroup, FileTreeNode, FileTreeOptions};
pub use records::{RecordTarget, RecordView, WriteOutcome};

use coflow_api::{
    map_diagnostics_with_origins, origins_of, CfdInputRecord, CftContainer, Diagnostic,
    DiagnosticSet, Label, LoadContext, LoadedRecords, LoaderSelectionError, ProjectSourceRef,
    ProviderRegistry, RecordOrigin, ResolvedSource, Severity, SourceLocation, SourceLocationSpec,
    SourceResolveContext,
};
use coflow_checker::run_checks_for_dimensions_with_deps;
use coflow_data_model::{CfdDataModel, CfdDiagnostics, CfdPath, CfdPathSegment, CfdRecordId};
use coflow_project::{
    compile_schema_project, dedupe_cft_diagnostics, diagnostic_set_from_cft, path_to_slash,
    Project, SchemaBuild, SourceConfig,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;
use std::sync::Arc;

/// Stable, wire-friendly coordinate of a top-level record: its actual type
/// name plus its record key. Top-level records always have a `(actual_type,
/// key)` pair that uniquely identifies them inside a single model build, even
/// when synthetic dimension records share keys with their source records.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../frontend/src/bindings/"
    )
)]
pub struct RecordCoordinate {
    pub actual_type: String,
    pub key: String,
}

impl RecordCoordinate {
    #[must_use]
    pub fn new(actual_type: impl Into<String>, key: impl Into<String>) -> Self {
        Self {
            actual_type: actual_type.into(),
            key: key.into(),
        }
    }
}

type ResolvedLoaderSource = (Arc<dyn coflow_api::DataLoader>, ResolvedSource);

#[derive(Debug)]
pub struct ProjectSession {
    pub project: Project,
    pub schema: CftContainer,
    pub model: CfdDataModel,
    pub diagnostics: DiagnosticsStore,
    pub sources: SourceIndex,
    pub records: RecordIndex,
    pub files: FileIndex,
    pub dependencies: DependencyIndex,
}

impl ProjectSession {
    #[must_use]
    pub fn has_diagnostics(&self) -> bool {
        !self.diagnostics.is_empty()
    }

    /// Resolve a wire `(actual_type, key)` coordinate to its internal model
    /// id. Returns `None` when no record matches — callers surface an
    /// `EditorError::NotFound` rather than panic.
    #[must_use]
    pub fn id_for_coordinate(&self, actual_type: &str, key: &str) -> Option<CfdRecordId> {
        self.records.id_for_coordinate(actual_type, key)
    }

    /// Inverse of [`Self::id_for_coordinate`]: given an internal record id,
    /// return the wire coordinate. Lives here so model id leakage stays
    /// confined to the engine boundary.
    #[must_use]
    pub fn coordinate_of(&self, id: CfdRecordId) -> Option<RecordCoordinate> {
        self.records.get(id).map(|r| r.coordinate.clone())
    }

    /// Look up the project-relative file that backs a record, addressed by
    /// its wire coordinate.
    #[must_use]
    pub fn file_for_record(&self, actual_type: &str, key: &str) -> Option<&str> {
        self.records.file_for_coordinate(actual_type, key)
    }

    /// Iterate the coordinates of every top-level record in `file`. Used by
    /// hosts that render per-file record lists without exposing internal ids.
    pub fn coordinates_in_file<'a>(
        &'a self,
        file: &str,
    ) -> impl Iterator<Item = &'a RecordCoordinate> + 'a {
        self.records.coordinates_in_file(file)
    }

    /// Integer value of an enum variant declared in the project schema.
    /// Returns `None` for unknown enum names or variants.
    #[must_use]
    pub fn enum_int_value(&self, enum_name: &str, variant: &str) -> Option<i64> {
        let resolved = self.schema.resolve_enum(enum_name)?;
        resolved
            .variants
            .iter()
            .find(|v| v.name == variant)
            .map(|v| v.value)
    }

    /// Resolved dimension metadata for the project.
    #[must_use]
    pub fn dimensions(&self) -> Vec<DimensionInfo> {
        let fields = dimensions::language_dimension_fields(&self.schema);
        dimensions_for_project(&self.project, &fields)
    }

    /// Lookup a single dimension by name.
    #[must_use]
    pub fn dimension(&self, name: &str) -> Option<DimensionInfo> {
        self.dimensions().into_iter().find(|d| d.name == name)
    }

    /// Compose a read-only [`RecordView`] for a coordinate. Returns `None`
    /// when no record matches — typically a stale coordinate after a rename.
    #[must_use]
    pub fn record_view(&self, actual_type: &str, key: &str) -> Option<RecordView<'_>> {
        let record_ref = self.records.get_by_coordinate(actual_type, key)?;
        let record = self.model.record(record_ref.id)?;
        Some(RecordView {
            coordinate: record_ref.coordinate.clone(),
            display_path: record_ref.display_path.as_str(),
            record,
            origin: &record_ref.origin,
            source_id: record_ref.source_id,
            provider_id: record_ref.provider_id.as_str(),
        })
    }

    /// Iterate read-only views of every record backed by `file`.
    pub fn record_views_in_file<'a>(
        &'a self,
        file: &str,
    ) -> impl Iterator<Item = RecordView<'a>> + 'a {
        self.records
            .ids_in_file(file)
            .iter()
            .filter_map(move |id| {
                let record_ref = self.records.get(*id)?;
                let record = self.model.record(*id)?;
                Some(RecordView {
                    coordinate: record_ref.coordinate.clone(),
                    display_path: record_ref.display_path.as_str(),
                    record,
                    origin: &record_ref.origin,
                    source_id: record_ref.source_id,
                    provider_id: record_ref.provider_id.as_str(),
                })
            })
    }

    /// File-tree view of the project using default options (every
    /// loader-registered extension is walked, dimension out_dirs become
    /// virtual subtrees).
    #[must_use]
    pub fn file_tree(&self, ext_whitelist: BTreeSet<String>) -> Vec<FileTreeNode> {
        let mut options = FileTreeOptions {
            extra_extensions: ext_whitelist.into_iter().collect(),
            dimension_groups: Vec::new(),
            in_sources: BTreeSet::new(),
        };
        for source in self.files.source_files() {
            options
                .in_sources
                .insert(path_to_slash(Path::new(source)));
        }
        for info in self.dimensions() {
            if let Some(out_dir) = info.out_dir.as_ref() {
                let absolute = self.project.resolve_path(Path::new(out_dir));
                options.dimension_groups.push(DimensionGroup {
                    display_name: info.display_name.clone(),
                    dir: absolute,
                });
            }
        }
        self.file_tree_with(options)
    }

    /// File-tree view using caller-supplied options. The options carry the
    /// extension whitelist and any dimension groups that should be lifted to
    /// the top of the tree.
    #[must_use]
    pub fn file_tree_with(&self, options: FileTreeOptions) -> Vec<FileTreeNode> {
        let ext_whitelist: BTreeSet<String> = options.extra_extensions.into_iter().collect();
        let mut skip: BTreeSet<String> = BTreeSet::new();
        for group in &options.dimension_groups {
            if let Ok(rel) = group.dir.strip_prefix(&self.project.root_dir) {
                let slash = path_to_slash(rel);
                if !slash.is_empty() {
                    skip.insert(slash);
                }
            }
        }
        let mut tree = files::build_file_tree(
            &self.project.root_dir,
            &options.in_sources,
            &ext_whitelist,
            &skip,
        );
        for group in options.dimension_groups.iter().rev() {
            if let Some(node) = files::build_dimension_subtree(
                &self.project.root_dir,
                group.display_name.clone(),
                &group.dir,
                &options.in_sources,
                &ext_whitelist,
            ) {
                tree.insert(0, node);
            }
        }
        tree
    }
}

#[derive(Debug)]
pub struct ProjectSchemaSession {
    pub project: Project,
    pub schema: CftContainer,
    pub diagnostics: DiagnosticsStore,
}

impl ProjectSchemaSession {
    #[must_use]
    pub fn has_diagnostics(&self) -> bool {
        !self.diagnostics.is_empty()
    }
}

#[derive(Debug, Clone, Default)]
pub struct DiagnosticsStore {
    diagnostics: DiagnosticSet,
    by_stage: BTreeMap<String, Vec<usize>>,
    by_file: BTreeMap<String, Vec<usize>>,
    by_record: BTreeMap<String, Vec<usize>>,
    logical_locations: BTreeMap<usize, DiagnosticLogicalLocation>,
}

impl DiagnosticsStore {
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn from_set(diagnostics: DiagnosticSet) -> Self {
        Self::from_parts(diagnostics, BTreeMap::new())
    }

    #[must_use]
    pub fn from_parts(
        diagnostics: DiagnosticSet,
        logical_locations: BTreeMap<usize, DiagnosticLogicalLocation>,
    ) -> Self {
        let mut store = Self {
            diagnostics,
            by_stage: BTreeMap::new(),
            by_file: BTreeMap::new(),
            by_record: BTreeMap::new(),
            logical_locations,
        };
        store.rebuild_indexes();
        store
    }

    pub fn extend(&mut self, diagnostics: DiagnosticSet) {
        self.diagnostics.extend(diagnostics);
        self.rebuild_indexes();
    }

    pub fn extend_with_logical_locations(
        &mut self,
        diagnostics: DiagnosticSet,
        logical_locations: BTreeMap<usize, DiagnosticLogicalLocation>,
    ) {
        let offset = self.diagnostics.diagnostics.len();
        self.diagnostics.extend(diagnostics);
        for (index, location) in logical_locations {
            self.logical_locations.insert(offset + index, location);
        }
        self.rebuild_indexes();
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }

    #[must_use]
    pub const fn as_set(&self) -> &DiagnosticSet {
        &self.diagnostics
    }

    #[must_use]
    pub fn into_set(self) -> DiagnosticSet {
        self.diagnostics
    }

    #[must_use]
    pub fn logical_location(&self, index: usize) -> Option<&DiagnosticLogicalLocation> {
        self.logical_locations.get(&index)
    }

    #[must_use]
    pub fn by_stage(&self, stage: &str) -> &[usize] {
        self.by_stage.get(stage).map_or(&[], Vec::as_slice)
    }

    #[must_use]
    pub fn by_file(&self, file: &str) -> &[usize] {
        self.by_file.get(file).map_or(&[], Vec::as_slice)
    }

    #[must_use]
    pub fn by_record(&self, record_key: &str) -> &[usize] {
        self.by_record.get(record_key).map_or(&[], Vec::as_slice)
    }

    fn rebuild_indexes(&mut self) {
        self.by_stage.clear();
        self.by_file.clear();
        self.by_record.clear();
        for (index, diagnostic) in self.diagnostics.diagnostics.iter().enumerate() {
            self.by_stage
                .entry(diagnostic.stage.clone())
                .or_default()
                .push(index);
            if let Some(file) = diagnostic
                .primary
                .as_ref()
                .map(|label| source_location_display_path(&label.location))
            {
                self.by_file.entry(file).or_default().push(index);
            }
            if let Some(location) = self.logical_locations.get(&index) {
                if let Some(record_key) = &location.record_key {
                    self.by_record
                        .entry(record_key.clone())
                        .or_default()
                        .push(index);
                }
            }
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiagnosticLogicalLocation {
    pub record_key: Option<String>,
    pub field_path: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceIndex {
    entries: Vec<ResolvedSourceEntry>,
}

impl SourceIndex {
    #[must_use]
    pub fn entries(&self) -> &[ResolvedSourceEntry] {
        &self.entries
    }

    fn push(&mut self, entry: ResolvedSourceEntry) {
        self.entries.push(entry);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSourceEntry {
    pub id: SourceId,
    pub provider_id: String,
    pub source: ResolvedSource,
    pub display_path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceId(usize);

impl SourceId {
    #[must_use]
    pub const fn index(self) -> usize {
        self.0
    }
}

/// Index of every top-level record in the project. The authoritative key is
/// `(actual_type, key)` so synthetic records that share a key with their
/// source record (dimension variants of a same-key source row) don't collide.
///
/// Loaders push `PendingRecordRef` entries during the load pass; after
/// `model.build()` returns, [`RecordIndex::finalize_with_model`] walks
/// `model.records()` and matches each `CfdRecord` back to its pending entry
/// by `(actual_type, key)`, producing a fully-populated [`RecordRef`] per id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecordIndex {
    by_id: BTreeMap<CfdRecordId, RecordRef>,
    by_coordinate: BTreeMap<RecordCoordinate, CfdRecordId>,
    files: BTreeMap<String, Vec<CfdRecordId>>,
    pending: Vec<PendingRecordRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingRecordRef {
    coordinate: RecordCoordinate,
    origin: RecordOrigin,
    source_id: SourceId,
    provider_id: String,
    display_path: String,
}

impl RecordIndex {
    #[must_use]
    pub fn get(&self, id: CfdRecordId) -> Option<&RecordRef> {
        self.by_id.get(&id)
    }

    #[must_use]
    pub fn get_by_coordinate(
        &self,
        actual_type: &str,
        key: &str,
    ) -> Option<&RecordRef> {
        let id = self
            .by_coordinate
            .get(&RecordCoordinate::new(actual_type, key))?;
        self.by_id.get(id)
    }

    #[must_use]
    pub fn id_for_coordinate(
        &self,
        actual_type: &str,
        key: &str,
    ) -> Option<CfdRecordId> {
        self.by_coordinate
            .get(&RecordCoordinate::new(actual_type, key))
            .copied()
    }

    #[must_use]
    pub fn ids_in_file(&self, file: &str) -> &[CfdRecordId] {
        self.files.get(file).map_or(&[], Vec::as_slice)
    }

    pub fn coordinates_in_file<'a>(
        &'a self,
        file: &str,
    ) -> impl Iterator<Item = &'a RecordCoordinate> + 'a {
        self.ids_in_file(file)
            .iter()
            .filter_map(move |id| self.by_id.get(id).map(|r| &r.coordinate))
    }

    #[must_use]
    pub fn file_for_id(&self, id: CfdRecordId) -> Option<&str> {
        self.by_id.get(&id).map(|r| r.display_path.as_str())
    }

    #[must_use]
    pub fn file_for_coordinate(&self, actual_type: &str, key: &str) -> Option<&str> {
        self.get_by_coordinate(actual_type, key)
            .map(|r| r.display_path.as_str())
    }

    #[must_use]
    pub const fn by_id(&self) -> &BTreeMap<CfdRecordId, RecordRef> {
        &self.by_id
    }

    #[must_use]
    pub const fn by_file(&self) -> &BTreeMap<String, Vec<CfdRecordId>> {
        &self.files
    }

    fn push_pending(&mut self, pending: PendingRecordRef) {
        self.pending.push(pending);
    }

    /// After `model.build()` succeeds, match each model record back to a
    /// pending entry by `(actual_type, key)`. Pending entries that don't
    /// match a model record (because the loader produced a record that was
    /// rejected during model build) are silently dropped.
    fn finalize_with_model(&mut self, model: &CfdDataModel) {
        self.by_id.clear();
        self.by_coordinate.clear();
        self.files.clear();
        // Index pending by coordinate, popping each entry as it's matched so
        // duplicate loader output (theoretically impossible since model
        // build rejects duplicates) doesn't reuse the same metadata twice.
        let mut pending_by_coordinate: BTreeMap<RecordCoordinate, PendingRecordRef> =
            BTreeMap::new();
        for pending in std::mem::take(&mut self.pending) {
            pending_by_coordinate.insert(pending.coordinate.clone(), pending);
        }
        for (id, record) in model.records() {
            let coordinate = RecordCoordinate::new(record.actual_type.clone(), record.key.clone());
            let Some(pending) = pending_by_coordinate.remove(&coordinate) else {
                continue;
            };
            self.files
                .entry(pending.display_path.clone())
                .or_default()
                .push(id);
            self.by_coordinate.insert(coordinate.clone(), id);
            self.by_id.insert(
                id,
                RecordRef {
                    id,
                    coordinate,
                    origin: pending.origin,
                    source_id: pending.source_id,
                    provider_id: pending.provider_id,
                    display_path: pending.display_path,
                },
            );
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordRef {
    pub id: CfdRecordId,
    pub coordinate: RecordCoordinate,
    pub origin: RecordOrigin,
    pub source_id: SourceId,
    pub provider_id: String,
    pub display_path: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileIndex {
    source_files: BTreeSet<String>,
    display_to_source: BTreeMap<String, SourceId>,
}

impl FileIndex {
    #[must_use]
    pub const fn source_files(&self) -> &BTreeSet<String> {
        &self.source_files
    }

    #[must_use]
    pub fn source_for_display(&self, display_path: &str) -> Option<SourceId> {
        self.display_to_source.get(display_path).copied()
    }

    fn add_source_file(&mut self, display_path: String, source_id: SourceId) {
        self.source_files.insert(display_path.clone());
        self.display_to_source.insert(display_path, source_id);
    }
}

/// Engine-owned dependency view captured during a full check run.
///
/// `reads_from[a]` is the set of records `a` reads while evaluating its own
/// check blocks. The session can invert this graph to compute which records'
/// checks may need to re-run after edits without exposing checker internals to
/// hosts.
#[derive(Debug, Clone, Default)]
pub struct DependencyIndex {
    reads_from: BTreeMap<CfdRecordId, BTreeSet<CfdRecordId>>,
}

impl DependencyIndex {
    #[must_use]
    pub const fn reads_from(&self) -> &BTreeMap<CfdRecordId, BTreeSet<CfdRecordId>> {
        &self.reads_from
    }

    #[must_use]
    pub fn affected_by(&self, changed: &[CfdRecordId]) -> Vec<CfdRecordId> {
        let mut out: BTreeSet<CfdRecordId> = changed.iter().copied().collect();
        let changed_set: BTreeSet<CfdRecordId> = changed.iter().copied().collect();
        for (reader, reads) in &self.reads_from {
            if reads.iter().any(|id| changed_set.contains(id)) {
                out.insert(*reader);
            }
        }
        out.into_iter().collect()
    }
}

fn dependency_index_from_checker_graph(graph: coflow_checker::DependencyGraph) -> DependencyIndex {
    DependencyIndex {
        reads_from: graph.reads_from,
    }
}

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
    let dimension_fields = dimensions::language_dimension_fields(&schema);
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
                let dimension_diags = dimensions::regenerate_dimension_sources(
                    &project,
                    &output.model,
                    &dimension_fields,
                );
                diagnostics.extend(dimension_diags);
                if diagnostics.is_empty() && !dimension_fields.is_empty() {
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
    })
}

/// Opens and compiles a project schema without validating or loading data
/// sources.
///
/// # Errors
///
/// Returns unrecoverable project/schema I/O errors. User-fixable project and
/// schema diagnostics are captured in the returned session diagnostics.
pub fn build_project_schema_session(project: Project) -> Result<ProjectSchemaSession, String> {
    let diagnostics = project.schema_diagnostic_set();
    build_project_schema_with_diagnostics(project, diagnostics)
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

fn build_project_schema_with_diagnostics(
    project: Project,
    diagnostics: DiagnosticSet,
) -> Result<ProjectSchemaSession, String> {
    let mut diagnostics = DiagnosticsStore::from_set(diagnostics);
    let schema = if diagnostics.is_empty() {
        match compile_project_schema(&project)? {
            Ok(mut schema) => {
                diagnostics.extend(validate_dimension_schema_config(&project, &schema));
                if diagnostics.is_empty() {
                    if let Some(config) = project.config.dimensions.get("language") {
                        if let Err(err) =
                            dimensions::inject_language_dimension_types(&mut schema, config)
                        {
                            diagnostics.extend(diagnostic_set_from_cft(
                                err.diagnostics,
                                &BTreeMap::new(),
                                &BTreeMap::new(),
                            ));
                        }
                    }
                }
                schema
            }
            Err(schema_diagnostics) => {
                diagnostics.extend(schema_diagnostics);
                CftContainer::new()
            }
        }
    } else {
        CftContainer::new()
    };
    Ok(ProjectSchemaSession {
        project,
        schema,
        diagnostics,
    })
}

fn validate_dimension_schema_config(project: &Project, schema: &CftContainer) -> DiagnosticSet {
    let mut diagnostics = DiagnosticSet::empty();
    if !dimensions::language_dimension_fields(schema).is_empty()
        && !project.config.dimensions.contains_key("language")
    {
        diagnostics.push(Diagnostic {
            code: "DIM-CONFIG-001".to_string(),
            stage: "PROJECT".to_string(),
            severity: Severity::Error,
            message: "schema contains @localized fields but dimensions.language is not configured"
                .to_string(),
            primary: Some(Label {
                location: SourceLocation::ProjectConfig {
                    path: project.config_path.clone(),
                    key_path: vec!["dimensions".to_string(), "language".to_string()],
                },
                message: None,
            }),
            related: Vec::new(),
        });
    }
    diagnostics
}

fn compile_project_schema(
    project: &Project,
) -> Result<Result<CftContainer, DiagnosticSet>, String> {
    let project_diagnostics = project.schema_diagnostic_set();
    if !project_diagnostics.is_empty() {
        return Ok(Err(project_diagnostics));
    }
    let build = compile_schema_project(project, None)?;
    let diagnostics = diagnostics_from_schema_build(&build);
    if diagnostics.is_empty() {
        build
            .container
            .ok_or_else(|| "schema compilation did not produce a container".to_string())
            .map(Ok)
    } else {
        Ok(Err(diagnostics))
    }
}

fn diagnostics_from_schema_build(build: &SchemaBuild) -> DiagnosticSet {
    diagnostic_set_from_cft(
        dedupe_cft_diagnostics(build.diagnostics.clone()),
        &build.sources,
        &build.paths,
    )
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
        let dimension_fields = dimensions::language_dimension_fields(schema);
        for configured in dimensions::language_dimension_sources(project, &dimension_fields) {
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
    let record_keys = records
        .iter()
        .map(|record| record.key.clone())
        .collect::<Vec<_>>();
    let mut builder = CfdDataModel::builder(schema);
    for record in records {
        builder.add_input_record(record);
    }
    let model = match builder.build() {
        Ok(model) => model,
        Err(err) => {
            let logical_locations =
                logical_locations_from_cfd(&err, |id| record_keys.get(id.index()).cloned());
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
            model.record(id).map(|record| record.key.clone())
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
    resolve_record_key: impl Fn(CfdRecordId) -> Option<String>,
) -> BTreeMap<usize, DiagnosticLogicalLocation> {
    diagnostics
        .diagnostics
        .iter()
        .enumerate()
        .filter_map(|(index, diagnostic)| {
            let primary = diagnostic.primary.as_ref()?;
            let record_key = primary.record.and_then(&resolve_record_key);
            let field_path =
                (!primary.path.segments.is_empty()).then(|| format_cfd_path(&primary.path));
            (record_key.is_some() || field_path.is_some()).then_some((
                index,
                DiagnosticLogicalLocation {
                    record_key,
                    field_path,
                },
            ))
        })
        .collect()
}

fn format_cfd_path(path: &CfdPath) -> String {
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

fn source_location_display_path(location: &SourceLocation) -> String {
    match location {
        SourceLocation::FileSpan { path, .. }
        | SourceLocation::TableCell { path, .. }
        | SourceLocation::ProjectConfig { path, .. }
        | SourceLocation::Artifact { path } => path_to_slash(path),
        SourceLocation::RemoteCell { document, .. } => document.clone(),
    }
}

fn empty_model() -> Result<CfdDataModel, String> {
    CfdDataModel::builder(&CftContainer::new())
        .build()
        .map_err(|_| "empty model build failed".to_string())
}
