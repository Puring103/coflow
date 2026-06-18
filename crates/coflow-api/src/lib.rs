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

pub mod export;
pub mod table;

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
    CfdLabel, CfdPath, CfdPathSegment, CfdRecord, CfdTable, CfdValue,
};
pub use export::{export_model_with_encoder, ExportEncoder, ExportError};

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

#[derive(Debug, Clone, PartialEq, Eq)]
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

    pub fn extend(&mut self, other: Self) {
        self.diagnostics.extend(other.diagnostics);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: String,
    pub stage: String,
    pub severity: Severity,
    pub message: String,
    pub primary: Option<Label>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    pub location: SourceLocation,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSpec {
    pub source_type: Option<String>,
    pub file: Option<PathBuf>,
    pub dir: Option<PathBuf>,
    pub uri: Option<String>,
    pub options: serde_json::Value,
}

impl SourceSpec {
    #[must_use]
    pub fn file(path: impl Into<PathBuf>, options: serde_json::Value) -> Self {
        Self {
            source_type: None,
            file: Some(path.into()),
            dir: None,
            uri: None,
            options,
        }
    }
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
    pub config_keys: &'static [&'static str],
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

#[derive(Debug, Clone)]
pub struct SourceRef<'a> {
    pub source_type: Option<&'a str>,
    pub path: Option<&'a Path>,
    pub uri: Option<&'a str>,
    pub config_keys: &'a [&'a str],
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
    pub origins: OriginMap,
}

#[derive(Debug, Clone, Default)]
pub struct OriginMap {
    records: Vec<RecordOrigin>,
}

impl OriginMap {
    #[must_use]
    pub fn record_count(&self) -> usize {
        self.records.len()
    }

    pub fn extend(&mut self, other: Self) {
        self.records.extend(other.records);
    }

    pub fn push_file_records(&mut self, path: impl Into<PathBuf>, count: usize) {
        let path = path.into();
        self.records.extend(
            std::iter::repeat_with(|| RecordOrigin::File { path: path.clone() }).take(count),
        );
    }

    pub(crate) fn push_table_record(
        &mut self,
        file: PathBuf,
        sheet: String,
        row: usize,
        id_column: usize,
        field_columns: BTreeMap<Vec<String>, usize>,
    ) {
        self.records.push(RecordOrigin::Table {
            file,
            sheet,
            row,
            id_column,
            field_columns,
        });
    }

    #[must_use]
    pub fn map_diagnostics(&self, diagnostics: CfdDiagnostics) -> DiagnosticSet {
        DiagnosticSet {
            diagnostics: diagnostics
                .diagnostics
                .into_iter()
                .map(|diagnostic| self.map_diagnostic(diagnostic))
                .collect(),
        }
    }

    fn map_diagnostic(&self, diagnostic: CfdDiagnostic) -> Diagnostic {
        Diagnostic {
            code: diagnostic.code.as_str().to_string(),
            stage: diagnostic.stage.to_string(),
            severity: Severity::Error,
            message: diagnostic.message,
            primary: diagnostic
                .primary
                .as_ref()
                .and_then(|label| self.map_label(label)),
            related: diagnostic
                .related
                .iter()
                .filter_map(|label| self.map_label(label))
                .collect(),
        }
    }

    fn map_label(&self, label: &CfdLabel) -> Option<Label> {
        let record = label.record?;
        let origin = self.records.get(record.index())?;
        Some(Label {
            location: origin.location_for_path(&label.path),
            message: label.message.clone(),
        })
    }
}

#[derive(Debug, Clone)]
enum RecordOrigin {
    Table {
        file: PathBuf,
        sheet: String,
        row: usize,
        id_column: usize,
        field_columns: BTreeMap<Vec<String>, usize>,
    },
    File {
        path: PathBuf,
    },
}

impl RecordOrigin {
    fn location_for_path(&self, path: &CfdPath) -> SourceLocation {
        match self {
            Self::Table {
                file,
                sheet,
                row,
                id_column,
                field_columns,
            } => SourceLocation::TableCell {
                path: file.clone(),
                sheet: Some(sheet.clone()),
                row: *row,
                column: path_column(path, field_columns)
                    .or_else(|| {
                        root_field(path).and_then(|field| (field == "id").then_some(*id_column))
                    })
                    .unwrap_or(*id_column),
            },
            Self::File { path } => SourceLocation::FileSpan {
                path: path.clone(),
                start_line: 0,
                start_character: 0,
                end_line: 0,
                end_character: 1,
            },
        }
    }
}

fn root_field(path: &CfdPath) -> Option<&str> {
    path.segments.iter().find_map(|segment| match segment {
        CfdPathSegment::Field(name) => Some(name.as_str()),
        CfdPathSegment::Index(_) | CfdPathSegment::DictKey(_) => None,
    })
}

fn path_column(path: &CfdPath, field_columns: &BTreeMap<Vec<String>, usize>) -> Option<usize> {
    let mut prefix = Vec::new();
    let mut column = None;
    for segment in &path.segments {
        let CfdPathSegment::Field(field) = segment else {
            break;
        };
        prefix.push(field.clone());
        if let Some(candidate) = field_columns.get(&prefix) {
            column = Some(*candidate);
        }
    }
    column
}

pub trait DataLoader: Send + Sync {
    fn descriptor(&self) -> &'static LoaderDescriptor;

    fn probe(&self, source: &SourceRef<'_>) -> ProbeResult;

    fn preflight(&self, _ctx: LoadContext<'_>, _source: &SourceSpec) -> DiagnosticSet {
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
        source: &SourceSpec,
    ) -> Result<LoadedRecords, DiagnosticSet>;
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
    exporters: BTreeMap<&'static str, Arc<dyn DataExporter>>,
    codegens: BTreeMap<&'static str, Arc<dyn CodeGenerator>>,
}

impl fmt::Debug for ProviderRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProviderRegistry")
            .field("loaders", &self.loaders.keys().collect::<Vec<_>>())
            .field("exporters", &self.exporters.keys().collect::<Vec<_>>())
            .field("codegens", &self.codegens.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl ProviderRegistry {
    pub fn register_loader<L>(&mut self, loader: L)
    where
        L: DataLoader + 'static,
    {
        self.loaders
            .insert(loader.descriptor().id, Arc::new(loader));
    }

    pub fn register_exporter<E>(&mut self, exporter: E)
    where
        E: DataExporter + 'static,
    {
        self.exporters
            .insert(exporter.descriptor().id, Arc::new(exporter));
    }

    pub fn register_codegen<C>(&mut self, codegen: C)
    where
        C: CodeGenerator + 'static,
    {
        self.codegens
            .insert(codegen.descriptor().id, Arc::new(codegen));
    }

    #[must_use]
    pub fn loader(&self, id: &str) -> Option<Arc<dyn DataLoader>> {
        self.loaders.get(id).cloned()
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

    /// Selects a loader by explicit source type or by provider probe result.
    ///
    /// # Errors
    ///
    /// Returns an error when no provider matches, the explicit provider id is
    /// unknown, or multiple providers report the same highest confidence.
    pub fn select_loader(
        &self,
        source: &SourceRef<'_>,
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
