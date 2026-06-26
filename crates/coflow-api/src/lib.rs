//! Public provider API for Coflow loaders, exporters, and code generators.

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
#![allow(clippy::missing_const_for_fn)]

use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use coflow_cft::{
    CftAnnotation, CftAnnotationValue, CftContainer, CftSchemaEnum, CftSchemaField, CftSchemaType,
    CftSchemaTypeRef, ModuleId,
};
pub use coflow_data_model::{
    CfdDataModel, CfdDiagnostic, CfdDiagnostics, CfdDictKey, CfdInputRecord, CfdInputValue,
    CfdLabel, CfdPath, CfdPathSegment, CfdRecord, CfdRecordId, CfdTable, CfdValue, RecordOrigin,
    SourceDocument, TextSpan,
};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactContentKind {
    Text,
    Bytes,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactSet {
    pub files: Vec<ArtifactFile>,
    pub metadata: BTreeMap<String, String>,
}

impl ArtifactSet {
    #[must_use]
    pub fn new(files: Vec<ArtifactFile>) -> Self {
        Self {
            files,
            metadata: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactFile {
    pub relative_path: PathBuf,
    pub content: ArtifactContent,
}

impl ArtifactFile {
    #[must_use]
    pub fn text(relative_path: impl Into<PathBuf>, contents: impl Into<String>) -> Self {
        Self {
            relative_path: relative_path.into(),
            content: ArtifactContent::Text(contents.into()),
        }
    }

    #[must_use]
    pub fn bytes(relative_path: impl Into<PathBuf>, contents: Vec<u8>) -> Self {
        Self {
            relative_path: relative_path.into(),
            content: ArtifactContent::Bytes(contents),
        }
    }

    #[must_use]
    pub fn json(relative_path: impl Into<PathBuf>, value: serde_json::Value) -> Self {
        Self {
            relative_path: relative_path.into(),
            content: ArtifactContent::Json(value),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtifactContent {
    Text(String),
    Bytes(Vec<u8>),
    Json(serde_json::Value),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticSet {
    pub diagnostics: Vec<Diagnostic>,
}

impl DiagnosticSet {
    #[must_use]
    pub fn empty() -> Self {
        Self {
            diagnostics: Vec::new(),
        }
    }

    #[must_use]
    pub fn one(diagnostic: Diagnostic) -> Self {
        Self {
            diagnostics: vec![diagnostic],
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Diagnostic> {
        self.diagnostics.iter()
    }

    pub fn extend(&mut self, other: Self) {
        self.diagnostics.extend(other.diagnostics);
    }

    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }
}

impl From<Vec<Diagnostic>> for DiagnosticSet {
    fn from(diagnostics: Vec<Diagnostic>) -> Self {
        Self { diagnostics }
    }
}

impl<'a> IntoIterator for &'a DiagnosticSet {
    type Item = &'a Diagnostic;
    type IntoIter = std::slice::Iter<'a, Diagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub code: String,
    pub stage: String,
    pub severity: Severity,
    pub message: String,
    pub primary: Option<Label>,
    #[serde(default)]
    pub related: Vec<Label>,
}

impl Diagnostic {
    #[must_use]
    pub fn error(
        code: impl Into<String>,
        stage: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            stage: stage.into(),
            severity: Severity::Error,
            message: message.into(),
            primary: None,
            related: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_primary(mut self, label: Label) -> Self {
        self.primary = Some(label);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Label {
    pub location: SourceLocation,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceLocation {
    FileSpan {
        path: PathBuf,
        start_line: usize,
        start_character: usize,
        end_line: usize,
        end_character: usize,
    },
    TableCell {
        path: PathBuf,
        sheet: Option<String>,
        row: usize,
        column: usize,
    },
    RemoteCell {
        document: String,
        sheet: Option<String>,
        row: usize,
        column: usize,
    },
    ProjectConfig {
        path: PathBuf,
        key_path: Vec<String>,
    },
    Artifact {
        path: PathBuf,
    },
}

impl From<coflow_data_model::SourceLocation> for SourceLocation {
    fn from(loc: coflow_data_model::SourceLocation) -> Self {
        match loc {
            coflow_data_model::SourceLocation::FileSpan {
                path,
                start_line,
                start_character,
                end_line,
                end_character,
            } => Self::FileSpan {
                path,
                start_line,
                start_character,
                end_line,
                end_character,
            },
            coflow_data_model::SourceLocation::TableCell {
                path,
                sheet,
                row,
                column,
            } => Self::TableCell {
                path,
                sheet,
                row,
                column,
            },
            coflow_data_model::SourceLocation::RemoteCell {
                document,
                sheet,
                row,
                column,
            } => Self::RemoteCell {
                document,
                sheet,
                row,
                column,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceLocationSpec {
    Path(PathBuf),
    Uri(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectSourceRef<'a> {
    pub source_type: Option<&'a str>,
    pub location: &'a SourceLocationSpec,
    pub option_keys: &'a [&'a str],
}

#[derive(Debug, Clone, Copy)]
pub struct SourceResolveContext<'a> {
    pub project_root: &'a Path,
    pub schema: &'a CftContainer,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedSource {
    pub provider_id: String,
    pub location: SourceLocationSpec,
    pub options: serde_json::Value,
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputSpec {
    pub output_type: String,
    pub dir: PathBuf,
    pub options: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoaderDescriptor {
    pub id: &'static str,
    pub display_name: &'static str,
    pub extensions: &'static [&'static str],
    pub uri_schemes: &'static [&'static str],
    pub option_keys: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExporterDescriptor {
    pub id: &'static str,
    pub display_name: &'static str,
    pub table_file_extension: &'static str,
    pub content_kind: ArtifactContentKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodegenDescriptor {
    pub id: &'static str,
    pub display_name: &'static str,
    pub language: &'static str,
    pub file_extensions: &'static [&'static str],
    pub supported_data_formats: &'static [&'static str],
    pub needs_model_for_build: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProbeConfidence {
    None,
    Possible,
    Likely,
    Certain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProbeResult {
    pub confidence: ProbeConfidence,
}

impl ProbeResult {
    #[must_use]
    pub const fn none() -> Self {
        Self {
            confidence: ProbeConfidence::None,
        }
    }

    #[must_use]
    pub const fn likely() -> Self {
        Self {
            confidence: ProbeConfidence::Likely,
        }
    }

    #[must_use]
    pub const fn certain() -> Self {
        Self {
            confidence: ProbeConfidence::Certain,
        }
    }

    #[must_use]
    pub const fn is_match(self) -> bool {
        !matches!(self.confidence, ProbeConfidence::None)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LoadContext<'a> {
    pub project_root: &'a Path,
    pub schema: &'a CftContainer,
}

#[derive(Debug, Clone, Copy)]
pub struct ExportContext<'a> {
    pub schema: &'a CftContainer,
    pub model: &'a CfdDataModel,
}

#[derive(Debug, Clone, Copy)]
pub struct CodegenContext<'a> {
    pub schema: &'a CftContainer,
    pub model: Option<&'a CfdDataModel>,
    pub data_format: &'a str,
}

#[derive(Debug, Clone)]
pub struct LoadedRecords {
    pub records: Vec<CfdInputRecord>,
}

/// Map [`CfdDiagnostics`] to [`DiagnosticSet`] using a record→origin lookup.
///
/// Loaders no longer maintain a separate [`coflow_data_model::origin::RecordOrigin`]
/// map: each [`CfdInputRecord`] carries its own origin. Callers that need to
/// produce wire diagnostics from compiler/check failures pass either a slice
/// of records (or their extracted origins) and let this helper resolve labels.
#[must_use]
pub fn map_diagnostics_with_origins(
    diagnostics: CfdDiagnostics,
    origins: &[RecordOrigin],
) -> DiagnosticSet {
    let mapped =
        coflow_data_model::map_diagnostics(diagnostics, |id| origins.get(id.index()).cloned());
    DiagnosticSet {
        diagnostics: mapped
            .into_iter()
            .map(|d| Diagnostic {
                code: d.code,
                stage: d.stage,
                severity: Severity::Error,
                message: d.message,
                primary: d.primary.map(|l| Label {
                    location: l.location.into(),
                    message: l.message,
                }),
                related: d
                    .related
                    .into_iter()
                    .map(|l| Label {
                        location: l.location.into(),
                        message: l.message,
                    })
                    .collect(),
            })
            .collect(),
    }
}

/// Convenience helper: extract origins from a slice of input records.
#[must_use]
pub fn origins_of(records: &[CfdInputRecord]) -> Vec<RecordOrigin> {
    records.iter().map(|r| r.origin.clone()).collect()
}

pub trait DataLoader: Send + Sync {
    fn descriptor(&self) -> &'static LoaderDescriptor;

    fn probe(&self, source: &ProjectSourceRef<'_>) -> ProbeResult;

    /// Resolves a project source into concrete provider sources to load.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the configured source cannot be expanded into
    /// concrete sources for this provider.
    fn resolve(
        &self,
        _ctx: SourceResolveContext<'_>,
        source: &ResolvedSource,
    ) -> Result<Vec<ResolvedSource>, DiagnosticSet> {
        Ok(vec![source.clone()])
    }

    fn preflight(&self, _ctx: LoadContext<'_>, _source: &ResolvedSource) -> DiagnosticSet {
        DiagnosticSet::empty()
    }

    /// Loads source data into source-neutral input records.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the source cannot be read, parsed, or converted
    /// into schema-guided input records.
    fn load(
        &self,
        ctx: LoadContext<'_>,
        source: &ResolvedSource,
    ) -> Result<LoadedRecords, DiagnosticSet>;
}

/// One step in a field path used by writers and the wire protocol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteFieldPathSegment {
    Field(String),
    Index(usize),
    DictKey(String),
}

/// Static description of a writer provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriterDescriptor {
    pub id: &'static str,
    pub display_name: &'static str,
    pub capabilities: WriterCapabilities,
}

/// Editing capabilities exposed to the front-end so the UI can grey out
/// disabled actions per source.
///
/// Lower-bounded by the writer's actual implementation; the front-end must
/// not assume a writer can do more than these flags claim.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct WriterCapabilities {
    pub provider_id: String,
    pub can_edit_field: bool,
    pub can_edit_key: bool,
    pub can_insert_record: bool,
    pub can_delete_record: bool,
    pub requires_full_refresh_after_write: bool,
    pub is_remote: bool,
}

impl WriterCapabilities {
    #[must_use]
    pub fn read_only() -> Self {
        Self {
            provider_id: String::new(),
            can_edit_field: false,
            can_edit_key: false,
            can_insert_record: false,
            can_delete_record: false,
            requires_full_refresh_after_write: false,
            is_remote: false,
        }
    }

    #[must_use]
    pub fn local_full() -> Self {
        Self {
            provider_id: String::new(),
            can_edit_field: true,
            can_edit_key: true,
            can_insert_record: true,
            can_delete_record: true,
            requires_full_refresh_after_write: true,
            is_remote: false,
        }
    }

    #[must_use]
    pub fn remote_field_edit() -> Self {
        Self {
            provider_id: String::new(),
            can_edit_field: true,
            can_edit_key: true,
            can_insert_record: false,
            can_delete_record: false,
            requires_full_refresh_after_write: true,
            is_remote: true,
        }
    }

    #[must_use]
    pub fn with_provider_id(mut self, provider_id: impl Into<String>) -> Self {
        self.provider_id = provider_id.into();
        self
    }
}

/// Request describing a single field write.
#[derive(Debug, Clone)]
pub struct WriteCellRequest<'a> {
    pub origin: &'a RecordOrigin,
    pub record_key: &'a str,
    pub actual_type: &'a str,
    pub field_path: &'a [WriteFieldPathSegment],
    /// Source-neutral new value, serialized to the source format by the writer.
    pub new_value: &'a CfdValue,
    /// Optional pre-resolved schema type for the record. Writers that produce
    /// typed source representations (e.g. CFD) use this for serialization.
    pub schema: &'a CftContainer,
    /// Original `ResolvedSource` that produced the record. Writers consult
    /// `source.options` to retrieve provider-specific configuration (Lark
    /// app credentials, alternate Excel sheet mappings, etc.).
    pub source: &'a ResolvedSource,
}

/// Request describing a new top-level record insertion.
#[derive(Debug, Clone)]
pub struct InsertRecordRequest<'a> {
    /// Target source that should receive the new record.
    pub source: &'a ResolvedSource,
    /// Target sheet/table name for table sources. Text writers may ignore it.
    pub sheet: Option<&'a str>,
    pub record_key: &'a str,
    pub actual_type: &'a str,
    pub fields: &'a BTreeMap<String, CfdValue>,
    pub schema: &'a CftContainer,
}

/// Request describing a top-level record deletion.
#[derive(Debug, Clone)]
pub struct DeleteRecordRequest<'a> {
    pub origin: &'a RecordOrigin,
    pub record_key: &'a str,
    pub actual_type: &'a str,
    pub source: &'a ResolvedSource,
}

/// Request describing a top-level record key rename.
#[derive(Debug, Clone)]
pub struct RenameRecordRequest<'a> {
    pub origin: &'a RecordOrigin,
    pub old_key: &'a str,
    pub new_key: &'a str,
    pub actual_type: &'a str,
    pub source: &'a ResolvedSource,
    pub schema: &'a CftContainer,
}

/// Request to rewrite reference tokens inside one source after a record key
/// rename.
///
/// Engines use this for source syntax that compiles away before the runtime
/// model is built, such as `@Type.old.path` path refs or `...spread` entries.
#[derive(Debug, Clone)]
pub struct RewriteRecordReferencesRequest<'a> {
    pub source: &'a ResolvedSource,
    pub target_type_names: &'a [String],
    pub old_key: &'a str,
    pub new_key: &'a str,
    pub rewrite_direct_refs: bool,
    pub schema: &'a CftContainer,
}

/// Outcome of a writer call: which records were actually touched (so the
/// session can recompute checks) and any informational diagnostics.
#[derive(Debug, Clone, Default)]
pub struct WriteOutcome {
    /// Origins of records whose backing source changed. The session uses these
    /// to re-load specific records and run incremental checks; an empty vec
    /// means the writer made no observable change.
    pub touched_record_origins: Vec<RecordOrigin>,
    pub inserted_record_origin: Option<RecordOrigin>,
    pub deleted_record_origin: Option<RecordOrigin>,
    /// Optional non-fatal diagnostics surfaced to the user.
    pub diagnostics: DiagnosticSet,
}

/// Context passed to writers. Mirrors [`LoadContext`] but for writes.
#[derive(Debug, Clone, Copy)]
pub struct WriteContext<'a> {
    pub project_root: &'a Path,
    pub schema: &'a CftContainer,
    /// The current data model. Writers use it to resolve [`CfdRecordId`]s
    /// inside the request value (e.g. for ref serialization). May be `None`
    /// when running pre-flight on a value that hasn't been merged into the
    /// model yet.
    pub model: Option<&'a CfdDataModel>,
}

/// Trait for source-specific writers that persist field edits.
///
/// Implementations dispatch on [`RecordOrigin`] to locate the cell/span, write
/// the new value to the source (file, remote API, ...), and report which
/// records were touched so the session can run incremental checks.
pub trait DataWriter: Send + Sync {
    fn descriptor(&self) -> &'static WriterDescriptor;

    /// Cheap pre-flight check: type matches, target file exists, etc. The
    /// default implementation does nothing.
    fn preflight(&self, _ctx: WriteContext<'_>, _request: &WriteCellRequest<'_>) -> DiagnosticSet {
        DiagnosticSet::empty()
    }

    /// Persist a single field change.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the write cannot be performed (origin
    /// mismatch, missing file, transport error, schema-invalid value, etc.).
    fn write_field(
        &self,
        ctx: WriteContext<'_>,
        request: &WriteCellRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet>;

    /// Persist a new top-level record.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the writer cannot insert records for this
    /// source or when the request cannot be represented by the source format.
    fn insert_record(
        &self,
        _ctx: WriteContext<'_>,
        _request: &InsertRecordRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        Err(DiagnosticSet::one(Diagnostic::error(
            "WRITE-UNSUPPORTED",
            "WRITE",
            "writer does not support inserting records",
        )))
    }

    /// Rename a top-level record key.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the writer cannot rename keys for this source
    /// or when the existing source no longer matches the requested old key.
    fn rename_record(
        &self,
        _ctx: WriteContext<'_>,
        _request: &RenameRecordRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        Err(DiagnosticSet::one(Diagnostic::error(
            "WRITE-UNSUPPORTED",
            "WRITE",
            "writer does not support renaming record keys",
        )))
    }

    /// Rewrite source-level references to a renamed record key.
    ///
    /// The default implementation is a no-op because ordinary `CfdValue::Ref`
    /// locations are updated via [`DataWriter::write_field`]. Providers should
    /// override this when their source syntax contains references that do not
    /// survive as runtime refs.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the source cannot be read or updated.
    fn rewrite_record_references(
        &self,
        _ctx: WriteContext<'_>,
        _request: &RewriteRecordReferencesRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        Ok(WriteOutcome::default())
    }

    /// Delete a top-level record.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the writer cannot delete records for this
    /// source or when the target no longer matches the requested record.
    fn delete_record(
        &self,
        _ctx: WriteContext<'_>,
        _request: &DeleteRecordRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        Err(DiagnosticSet::one(Diagnostic::error(
            "WRITE-UNSUPPORTED",
            "WRITE",
            "writer does not support deleting records",
        )))
    }
}

pub trait DataExporter: Send + Sync {
    fn descriptor(&self) -> &'static ExporterDescriptor;

    fn preflight(&self, _ctx: ExportContext<'_>, _output: &OutputSpec) -> DiagnosticSet {
        DiagnosticSet::empty()
    }

    /// Exports a validated data model into artifact files.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the model cannot be encoded for this exporter.
    fn export(
        &self,
        ctx: ExportContext<'_>,
        output: &OutputSpec,
    ) -> Result<ArtifactSet, DiagnosticSet>;
}

pub trait CodeGenerator: Send + Sync {
    fn descriptor(&self) -> &'static CodegenDescriptor;

    fn preflight(&self, _ctx: CodegenContext<'_>, _output: &OutputSpec) -> DiagnosticSet {
        DiagnosticSet::empty()
    }

    /// Generates code artifact files.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the schema/options cannot be rendered for this
    /// target.
    fn generate(
        &self,
        ctx: CodegenContext<'_>,
        output: &OutputSpec,
    ) -> Result<ArtifactSet, DiagnosticSet>;
}

#[derive(Default, Clone)]
pub struct ProviderRegistry {
    loaders: BTreeMap<&'static str, Arc<dyn DataLoader>>,
    writers: BTreeMap<&'static str, Arc<dyn DataWriter>>,
    exporters: BTreeMap<&'static str, Arc<dyn DataExporter>>,
    codegens: BTreeMap<&'static str, Arc<dyn CodeGenerator>>,
}

impl fmt::Debug for ProviderRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProviderRegistry")
            .field("loaders", &self.loaders.keys().collect::<Vec<_>>())
            .field("writers", &self.writers.keys().collect::<Vec<_>>())
            .field("exporters", &self.exporters.keys().collect::<Vec<_>>())
            .field("codegens", &self.codegens.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl ProviderRegistry {
    /// Registers a loader provider.
    ///
    /// # Errors
    ///
    /// Returns an error when another loader with the same provider id has
    /// already been registered.
    pub fn register_loader<L>(&mut self, loader: L) -> Result<(), ProviderRegistrationError>
    where
        L: DataLoader + 'static,
    {
        let id = loader.descriptor().id;
        if self.loaders.contains_key(id) {
            return Err(ProviderRegistrationError::duplicate("loader", id));
        }
        self.loaders.insert(id, Arc::new(loader));
        Ok(())
    }

    /// Registers a writer provider.
    ///
    /// # Errors
    ///
    /// Returns an error when another writer with the same provider id has
    /// already been registered.
    pub fn register_writer<W>(&mut self, writer: W) -> Result<(), ProviderRegistrationError>
    where
        W: DataWriter + 'static,
    {
        let id = writer.descriptor().id;
        if self.writers.contains_key(id) {
            return Err(ProviderRegistrationError::duplicate("writer", id));
        }
        self.writers.insert(id, Arc::new(writer));
        Ok(())
    }

    /// Registers an exporter provider.
    ///
    /// # Errors
    ///
    /// Returns an error when another exporter with the same provider id has
    /// already been registered.
    pub fn register_exporter<E>(&mut self, exporter: E) -> Result<(), ProviderRegistrationError>
    where
        E: DataExporter + 'static,
    {
        let id = exporter.descriptor().id;
        if self.exporters.contains_key(id) {
            return Err(ProviderRegistrationError::duplicate("exporter", id));
        }
        self.exporters.insert(id, Arc::new(exporter));
        Ok(())
    }

    /// Registers a code generator provider.
    ///
    /// # Errors
    ///
    /// Returns an error when another code generator with the same provider id
    /// has already been registered.
    pub fn register_codegen<C>(&mut self, codegen: C) -> Result<(), ProviderRegistrationError>
    where
        C: CodeGenerator + 'static,
    {
        let id = codegen.descriptor().id;
        if self.codegens.contains_key(id) {
            return Err(ProviderRegistrationError::duplicate("codegen", id));
        }
        self.codegens.insert(id, Arc::new(codegen));
        Ok(())
    }

    #[must_use]
    pub fn loader(&self, id: &str) -> Option<Arc<dyn DataLoader>> {
        self.loaders.get(id).cloned()
    }

    #[must_use]
    pub fn writer(&self, id: &str) -> Option<Arc<dyn DataWriter>> {
        self.writers.get(id).cloned()
    }

    #[must_use]
    pub fn writers(&self) -> Vec<Arc<dyn DataWriter>> {
        self.writers.values().cloned().collect()
    }

    #[must_use]
    pub fn writer_descriptors(&self) -> Vec<&'static WriterDescriptor> {
        self.writers
            .values()
            .map(|writer| writer.descriptor())
            .collect()
    }

    #[must_use]
    pub fn exporter(&self, id: &str) -> Option<Arc<dyn DataExporter>> {
        self.exporters.get(id).cloned()
    }

    #[must_use]
    pub fn codegen(&self, id: &str) -> Option<Arc<dyn CodeGenerator>> {
        self.codegens.get(id).cloned()
    }

    #[must_use]
    pub fn loader_descriptors(&self) -> Vec<&'static LoaderDescriptor> {
        self.loaders
            .values()
            .map(|loader| loader.descriptor())
            .collect()
    }

    #[must_use]
    pub fn loaders(&self) -> Vec<Arc<dyn DataLoader>> {
        self.loaders.values().cloned().collect()
    }

    #[must_use]
    pub fn exporter_descriptors(&self) -> Vec<&'static ExporterDescriptor> {
        self.exporters
            .values()
            .map(|exporter| exporter.descriptor())
            .collect()
    }

    #[must_use]
    pub fn codegen_descriptors(&self) -> Vec<&'static CodegenDescriptor> {
        self.codegens
            .values()
            .map(|codegen| codegen.descriptor())
            .collect()
    }

    /// Selects a loader by explicit source type or by provider probe result.
    ///
    /// # Errors
    ///
    /// Returns an error when no provider matches, the explicit provider id is
    /// unknown, or multiple providers report the same highest confidence.
    pub fn select_loader(
        &self,
        source: &ProjectSourceRef<'_>,
    ) -> Result<Arc<dyn DataLoader>, LoaderSelectionError> {
        if let Some(source_type) = source.source_type {
            return self
                .loader(source_type)
                .ok_or_else(|| LoaderSelectionError::UnknownLoader {
                    id: source_type.to_string(),
                });
        }

        let mut matches = self
            .loaders
            .values()
            .filter_map(|loader| {
                let probe = loader.probe(source);
                probe.is_match().then(|| (probe.confidence, loader.clone()))
            })
            .collect::<Vec<_>>();
        matches.sort_by_key(|(confidence, _)| Reverse(*confidence));

        let Some((confidence, loader)) = matches.first().cloned() else {
            return Err(LoaderSelectionError::NoLoader);
        };
        let tied = matches
            .iter()
            .filter(|(candidate_confidence, _)| *candidate_confidence == confidence)
            .map(|(_, candidate)| candidate.descriptor().id.to_string())
            .collect::<Vec<_>>();
        if tied.len() > 1 {
            return Err(LoaderSelectionError::AmbiguousLoaders { ids: tied });
        }
        Ok(loader)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoaderSelectionError {
    UnknownLoader { id: String },
    NoLoader,
    AmbiguousLoaders { ids: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRegistrationError {
    provider_kind: &'static str,
    id: String,
}

impl ProviderRegistrationError {
    #[must_use]
    pub fn duplicate(provider_kind: &'static str, id: impl Into<String>) -> Self {
        Self {
            provider_kind,
            id: id.into(),
        }
    }

    #[must_use]
    pub const fn provider_kind(&self) -> &'static str {
        self.provider_kind
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }
}

impl fmt::Display for ProviderRegistrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "duplicate {} provider id `{}`",
            self.provider_kind, self.id
        )
    }
}

impl std::error::Error for ProviderRegistrationError {}

/// Wire-friendly flat view of a [`Diagnostic`].
///
/// Editor hosts use this as a single severity/code/message tuple anchored to
/// a file/record/field. Heavier-weight callers can keep the structured form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct FlatDiagnostic {
    pub severity: String,
    pub code: String,
    pub stage: String,
    pub message: String,
    pub file_path: Option<String>,
    pub actual_type: Option<String>,
    pub record_key: Option<String>,
    pub field_path: Option<String>,
}

impl Diagnostic {
    /// Flatten a diagnostic into the wire shape consumed by editor hosts.
    /// `actual_type` / `record_key` / `field_path` are not derivable from the structured
    /// diagnostic alone — hosts that know the record id of the diagnostic's
    /// label populate them out-of-band.
    #[must_use]
    pub fn flat_view(
        &self,
        actual_type: Option<String>,
        record_key: Option<String>,
        field_path: Option<String>,
    ) -> FlatDiagnostic {
        let file_path = self
            .primary
            .as_ref()
            .map(|label| source_location_display_path(&label.location));
        FlatDiagnostic {
            severity: severity_str(self.severity).to_string(),
            code: self.code.clone(),
            stage: self.stage.clone(),
            message: self.message.clone(),
            file_path,
            actual_type,
            record_key,
            field_path,
        }
    }
}

fn severity_str(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "info",
    }
}

fn source_location_display_path(location: &SourceLocation) -> String {
    let path_to_slash = |path: &Path| path.to_string_lossy().replace('\\', "/");
    match location {
        SourceLocation::FileSpan { path, .. }
        | SourceLocation::TableCell { path, .. }
        | SourceLocation::ProjectConfig { path, .. }
        | SourceLocation::Artifact { path } => path_to_slash(path),
        SourceLocation::RemoteCell { document, .. } => document.clone(),
    }
}
