use crate::{ArtifactSet, DiagnosticSet, OutputSpec};
use coflow_cft::CompiledSchema;
use coflow_data_model::CfdDataModel;

#[derive(Debug, Clone, Copy)]
pub struct CodegenContext<'a> {
    pub schema: &'a CompiledSchema,
    pub model: Option<&'a CfdDataModel>,
    pub data_format: &'a str,
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

pub trait CodeGenerator: Send + Sync {
    fn descriptor(&self) -> &'static CodegenDescriptor;

    /// Validates the request and generates code artifact files in memory.
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
