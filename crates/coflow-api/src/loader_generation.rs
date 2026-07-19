use crate::{ArtifactSet, DecodedOutputOptions, DiagnosticSet};
use coflow_cft::CftSchema;
use coflow_data_model::CfdDataModel;

#[derive(Debug, Clone, Copy)]
pub struct LoaderGenerationContext<'a> {
    pub schema: &'a CftSchema,
    pub model: Option<&'a CfdDataModel>,
    pub code_options: &'a DecodedOutputOptions,
    pub data_options: &'a DecodedOutputOptions,
    pub id_as_enum_variants: &'a serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoaderDescriptor {
    pub id: &'static str,
    pub code: &'static str,
    pub data: &'static str,
}

pub trait LoaderGenerator: Send + Sync {
    fn descriptor(&self) -> &'static LoaderDescriptor;

    /// Decode and validate loader-specific options.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when an option is unknown or malformed.
    fn decode_options(
        &self,
        options: &serde_json::Value,
    ) -> Result<DecodedOutputOptions, DiagnosticSet>;

    /// Generate target-language data-loading artifacts.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when loader code cannot be generated.
    fn generate(
        &self,
        ctx: LoaderGenerationContext<'_>,
        options: &DecodedOutputOptions,
    ) -> Result<ArtifactSet, DiagnosticSet>;

    /// Combines common code and loader artifacts using the layout emitted by
    /// legacy output configuration. Providers may override this when the old
    /// layout embedded loader members into common files.
    fn merge_legacy_artifacts(
        &self,
        common: ArtifactSet,
        loader: ArtifactSet,
    ) -> Result<ArtifactSet, DiagnosticSet> {
        let mut files = common.into_files();
        files.extend(loader.into_files());
        ArtifactSet::new(files).map_err(|error| {
            DiagnosticSet::one(crate::Diagnostic::error(
                "LOADER-ARTIFACT",
                "ARTIFACT",
                error.to_string(),
            ))
        })
    }
}
