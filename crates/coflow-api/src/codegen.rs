use crate::{ArtifactSet, DecodedOutputOptions, DiagnosticSet};
use coflow_cft::CompiledSchema;
use coflow_data_model::CfdDataModel;

#[derive(Debug, Clone, Copy)]
pub struct CodegenContext<'a> {
    pub schema: &'a CompiledSchema,
    pub model: Option<&'a CfdDataModel>,
    pub data_format: &'a str,
    pub id_as_enum_variants: &'a serde_json::Value,
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

    /// Decode and validate project-facing output options.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when an option is unknown or malformed.
    fn decode_options(
        &self,
        options: &serde_json::Value,
    ) -> Result<DecodedOutputOptions, DiagnosticSet>;

    /// Validates the request and generates code artifact files in memory.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the schema/options cannot be rendered for this
    /// target.
    fn generate(
        &self,
        ctx: CodegenContext<'_>,
        options: &DecodedOutputOptions,
    ) -> Result<ArtifactSet, DiagnosticSet>;
}
