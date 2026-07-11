use crate::DiagnosticSet;
use coflow_cft::CompiledSchema;
use coflow_data_model::CfdInputRecord;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

mod options;

pub use options::DecodedSourceOptions;

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
}

#[derive(Debug, Clone)]
pub struct ResolvedSource {
    pub provider_id: String,
    pub location: SourceLocationSpec,
    pub options: DecodedSourceOptions,
    pub display_name: String,
}

impl ResolvedSource {
    /// Return the provider-owned typed source options.
    ///
    /// # Errors
    ///
    /// Returns a provider contract diagnostic when the source was paired with
    /// options decoded by another provider or with an unexpected option type.
    pub fn options<T>(&self, provider_id: &str) -> Result<&T, DiagnosticSet>
    where
        T: std::fmt::Debug + Send + Sync + 'static,
    {
        self.options.require(provider_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputSpec {
    pub output_type: String,
    pub dir: PathBuf,
    pub options: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceProviderDescriptor {
    pub id: &'static str,
    pub display_name: &'static str,
    pub extensions: &'static [&'static str],
    pub uri_schemes: &'static [&'static str],
    pub option_keys: &'static [&'static str],
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
pub struct SourceLoadContext<'a> {
    pub project_root: &'a Path,
    pub schema: &'a CompiledSchema,
}

#[derive(Debug, Clone)]
pub struct LoadedSource {
    pub records: Vec<CfdInputRecord>,
}

pub trait SourceProvider: Send + Sync {
    fn descriptor(&self) -> &'static SourceProviderDescriptor;

    fn probe(&self, source: &ProjectSourceRef<'_>) -> ProbeResult;

    /// Decode project-facing JSON options into a provider-owned typed value.
    ///
    /// Raw JSON is confined to the project/provider adapter. Resolve, load,
    /// write, and table operations consume the decoded value stored in the
    /// [`ResolvedSource`].
    ///
    /// # Errors
    ///
    /// Returns diagnostics when an option is missing, unknown, or malformed.
    fn decode_options(&self, options: &Value) -> Result<DecodedSourceOptions, DiagnosticSet>;

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

    fn preflight(&self, _ctx: SourceLoadContext<'_>, _source: &ResolvedSource) -> DiagnosticSet {
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
        ctx: SourceLoadContext<'_>,
        source: &ResolvedSource,
    ) -> Result<LoadedSource, DiagnosticSet>;
}
