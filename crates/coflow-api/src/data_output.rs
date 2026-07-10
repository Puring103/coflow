use crate::{ArtifactContentKind, ArtifactSet, DiagnosticSet, OutputSpec};
use coflow_cft::CftContainer;
use coflow_data_model::CfdDataModel;

#[derive(Debug, Clone, Copy)]
pub struct ExportContext<'a> {
    pub schema: &'a CftContainer,
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
