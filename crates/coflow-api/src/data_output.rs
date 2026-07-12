use crate::{ArtifactContentKind, ArtifactSet, DecodedOutputOptions, DiagnosticSet};
use coflow_cft::CompiledSchema;
use coflow_data_model::CfdDataModel;

#[derive(Debug, Clone, Copy)]
pub struct ExportContext<'a> {
    pub schema: &'a CompiledSchema,
    pub model: &'a CfdDataModel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExporterDescriptor {
    pub id: &'static str,
    pub display_name: &'static str,
    pub table_file_extension: &'static str,
    pub content_kind: ArtifactContentKind,
}

pub trait DataExporter: Send + Sync {
    fn descriptor(&self) -> &'static ExporterDescriptor;

    /// Decode and validate project-facing output options.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when an option is unknown or malformed.
    fn decode_options(
        &self,
        options: &serde_json::Value,
    ) -> Result<DecodedOutputOptions, DiagnosticSet>;

    /// Exports a validated data model into artifact files.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the model cannot be encoded for this exporter.
    fn export(
        &self,
        ctx: ExportContext<'_>,
        options: &DecodedOutputOptions,
    ) -> Result<ArtifactSet, DiagnosticSet>;
}
