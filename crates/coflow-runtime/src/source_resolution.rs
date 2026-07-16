use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use coflow_api::{
    Diagnostic, DiagnosticSet, Label, ProjectSourceRef, ProviderRegistry, ResolvedSource, Severity,
    SourceLocation, SourceLocationSpec, SourceProvider, SourceProviderSelectionError,
    SourceResolveContext,
};
use coflow_project::{discover_directory_files, path_is_same_or_descendant, Project, SourceConfig};
use serde_json::Value;

mod dimensions;

pub(crate) type ResolvedLoaderSource = (Arc<dyn SourceProvider>, ResolvedSource);

#[derive(Clone)]
pub(crate) struct ConfiguredSource {
    pub(crate) provider_id: String,
    pub(crate) location: SourceLocationSpec,
    pub(crate) options: Value,
    pub(crate) display_name: String,
    pub(crate) source_index: Option<usize>,
}

pub(crate) struct SourceResolver<'a> {
    project: &'a Project,
    registry: &'a ProviderRegistry,
}

impl<'a> SourceResolver<'a> {
    pub(crate) const fn new(project: &'a Project, registry: &'a ProviderRegistry) -> Self {
        Self { project, registry }
    }

    pub(crate) fn configured(
        &self,
        source: &SourceConfig,
        source_index: Option<usize>,
    ) -> ConfiguredSource {
        configured_source(self.project, source, source_index)
    }

    pub(crate) fn resolve_for_load(
        &self,
        source: &SourceConfig,
        configured: &ConfiguredSource,
    ) -> Result<Vec<ResolvedLoaderSource>, DiagnosticSet> {
        if source.source_type.is_none()
            && matches!(configured.location, SourceLocationSpec::Path(ref path) if path.is_dir())
        {
            return self.resolve_directory(configured);
        }
        let provider = self.select(configured, source.source_type.as_deref())?;
        self.decode_and_expand(&provider, configured)
    }

    pub(crate) fn resolve_implicit(
        &self,
        configured: &ConfiguredSource,
    ) -> Result<Vec<ResolvedLoaderSource>, DiagnosticSet> {
        let source_type =
            (!configured.provider_id.is_empty()).then_some(configured.provider_id.as_str());
        let provider = self.select(configured, source_type)?;
        self.decode_and_expand(&provider, configured)
    }

    pub(crate) fn resolve_dimension_sources(
        &self,
        plan: &crate::dimensions::DimensionRuntimePlan,
    ) -> Result<Vec<(ResolvedLoaderSource, crate::dimensions::DimensionField)>, DiagnosticSet> {
        dimensions::resolve_dimension_sources(self, plan)
    }

    pub(crate) fn resolve_exact_at(
        &self,
        source: &SourceConfig,
        forced_provider: Option<&str>,
        location: SourceLocationSpec,
        display_name: String,
    ) -> Result<ResolvedSource, DiagnosticSet> {
        let source_index = self
            .project
            .config
            .sources
            .iter()
            .position(|candidate| std::ptr::eq(candidate, source));
        let mut configured = self.configured(source, source_index);
        configured.location = location;
        configured.display_name = display_name;
        self.resolve_exact_configured(
            &configured,
            forced_provider.or(source.source_type.as_deref()),
        )
    }

    pub(crate) fn resolve_unconfigured(
        &self,
        provider_id: &str,
        location: SourceLocationSpec,
        display_name: String,
    ) -> Result<ResolvedSource, DiagnosticSet> {
        self.resolve_exact_configured(
            &ConfiguredSource {
                provider_id: provider_id.to_string(),
                location,
                options: Value::Null,
                display_name,
                source_index: None,
            },
            Some(provider_id),
        )
    }

    fn resolve_exact_configured(
        &self,
        configured: &ConfiguredSource,
        forced_provider: Option<&str>,
    ) -> Result<ResolvedSource, DiagnosticSet> {
        let provider = match forced_provider {
            Some(provider_id) => self.registry.source_provider(provider_id).ok_or_else(|| {
                DiagnosticSet::one(project_diagnostic(
                    &self.project.config_path,
                    format!("source provider `{provider_id}` is not registered"),
                ))
            })?,
            None => self.select(configured, None)?,
        };
        decode_configured_source(provider.as_ref(), configured, &self.project.config_path)
    }

    fn resolve_directory(
        &self,
        configured: &ConfiguredSource,
    ) -> Result<Vec<ResolvedLoaderSource>, DiagnosticSet> {
        let SourceLocationSpec::Path(directory) = &configured.location;
        let files = discover_directory_files(directory).map_err(|error| {
            DiagnosticSet::one(project_diagnostic(
                &self.project.config_path,
                error.to_string(),
            ))
        })?;
        let managed_dimension_dirs = self
            .project
            .config
            .dimensions
            .values()
            .filter_map(|config| config.out_dir.as_ref())
            .map(|out_dir| self.project.resolve_path(out_dir))
            .collect::<Vec<_>>();
        let mut selected = Vec::new();
        for path in files.into_iter().filter(|path| {
            !managed_dimension_dirs
                .iter()
                .any(|out_dir| path_is_same_or_descendant(path, out_dir))
        }) {
            let file_source = ConfiguredSource {
                provider_id: String::new(),
                display_name: path.display().to_string(),
                location: SourceLocationSpec::Path(path.clone()),
                options: configured.options.clone(),
                source_index: configured.source_index,
            };
            let Some(provider) = self.select_optional(&file_source)? else {
                continue;
            };
            selected.push((path, provider));
        }
        validate_directory_options(
            &configured.options,
            selected.iter().map(|(_, provider)| provider.as_ref()),
            &self.project.config_path,
            configured.source_index,
        )?;

        let mut resolved = Vec::new();
        for (path, provider) in selected {
            let mut file_source = ConfiguredSource {
                provider_id: String::new(),
                display_name: path.display().to_string(),
                location: SourceLocationSpec::Path(path),
                options: configured.options.clone(),
                source_index: configured.source_index,
            };
            file_source.options =
                options_for_provider(&file_source.options, provider.descriptor().option_keys);
            resolved.extend(self.decode_and_expand(&provider, &file_source)?);
        }
        Ok(resolved)
    }

    fn select_optional(
        &self,
        configured: &ConfiguredSource,
    ) -> Result<Option<Arc<dyn SourceProvider>>, DiagnosticSet> {
        let option_keys = source_option_keys(&configured.options);
        match self
            .registry
            .select_source_provider(&source_ref(configured, None, &option_keys))
        {
            Ok(provider) => Ok(Some(provider)),
            Err(SourceProviderSelectionError::NoSourceProvider) => Ok(None),
            Err(error) => Err(DiagnosticSet::one(loader_selection_diagnostic(
                &self.project.config_path,
                configured,
                error,
            ))),
        }
    }

    fn select(
        &self,
        configured: &ConfiguredSource,
        source_type: Option<&str>,
    ) -> Result<Arc<dyn SourceProvider>, DiagnosticSet> {
        let option_keys = source_option_keys(&configured.options);
        self.registry
            .select_source_provider(&source_ref(configured, source_type, &option_keys))
            .map_err(|error| {
                DiagnosticSet::one(loader_selection_diagnostic(
                    &self.project.config_path,
                    configured,
                    error,
                ))
            })
    }

    fn decode_and_expand(
        &self,
        provider: &Arc<dyn SourceProvider>,
        configured: &ConfiguredSource,
    ) -> Result<Vec<ResolvedLoaderSource>, DiagnosticSet> {
        let decoded =
            decode_configured_source(provider.as_ref(), configured, &self.project.config_path)?;
        let context = SourceResolveContext {
            project_root: &self.project.root_dir,
        };
        provider
            .resolve(context, &decoded)?
            .into_iter()
            .map(|source| {
                validate_resolved_source(provider.as_ref(), &source)?;
                Ok((Arc::clone(provider), source))
            })
            .collect()
    }
}

pub(crate) fn validate_resolved_source(
    provider: &dyn SourceProvider,
    source: &ResolvedSource,
) -> Result<(), DiagnosticSet> {
    let expected = provider.descriptor().id;
    if source.provider_id == expected && source.options.provider_id() == expected {
        return Ok(());
    }
    Err(DiagnosticSet::one(Diagnostic::error(
        "PROVIDER-SOURCE-CONTRACT",
        "PROVIDER",
        format!(
            "provider `{expected}` resolved source `{}` with provider id `{}` and options owner `{}`",
            source.display_name,
            source.provider_id,
            source.options.provider_id()
        ),
    )))
}

fn configured_source(
    project: &Project,
    source: &SourceConfig,
    source_index: Option<usize>,
) -> ConfiguredSource {
    let SourceLocationSpec::Path(path) = source.location();
    let location = SourceLocationSpec::Path(project.resolve_path(path));
    let display_name = path.display().to_string();
    ConfiguredSource {
        provider_id: source.source_type.clone().unwrap_or_default(),
        location,
        options: source.options().clone(),
        display_name,
        source_index,
    }
}

fn decode_configured_source(
    provider: &dyn SourceProvider,
    source: &ConfiguredSource,
    config_path: &Path,
) -> Result<ResolvedSource, DiagnosticSet> {
    let options = provider
        .decode_options(&source.options)
        .map_err(|diagnostics| source_option_diagnostics(diagnostics, config_path, source))?;
    if options.provider_id() != provider.descriptor().id {
        return Err(DiagnosticSet::one(Diagnostic::error(
            "PROVIDER-OPTIONS-CONTRACT",
            "PROVIDER",
            format!(
                "provider `{}` decoded source options owned by `{}`",
                provider.descriptor().id,
                options.provider_id()
            ),
        )));
    }
    Ok(ResolvedSource {
        provider_id: provider.descriptor().id.to_string(),
        location: source.location.clone(),
        options,
        display_name: source.display_name.clone(),
    })
}

fn source_option_diagnostics(
    mut diagnostics: DiagnosticSet,
    config_path: &Path,
    source: &ConfiguredSource,
) -> DiagnosticSet {
    for diagnostic in &mut diagnostics.diagnostics {
        let mut option_path = match diagnostic.primary.take() {
            Some(Label {
                location: SourceLocation::ProjectConfig { key_path, .. },
                ..
            }) => key_path,
            Some(primary) => {
                diagnostic.primary = Some(primary);
                continue;
            }
            None => Vec::new(),
        };
        let mut key_path = Vec::new();
        if let Some(index) = source.source_index {
            key_path.extend(["sources".to_string(), index.to_string()]);
        }
        key_path.append(&mut option_path);
        diagnostic.primary = Some(Label {
            location: SourceLocation::ProjectConfig {
                path: config_path.to_path_buf(),
                key_path,
            },
            message: None,
        });
    }
    diagnostics
}

fn options_for_provider(options: &Value, keys: &[&str]) -> Value {
    let Some(object) = options.as_object() else {
        return options.clone();
    };
    Value::Object(
        object
            .iter()
            .filter(|(key, _)| keys.contains(&key.as_str()))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
    )
}

fn validate_directory_options<'a>(
    options: &Value,
    providers: impl IntoIterator<Item = &'a dyn SourceProvider>,
    config_path: &Path,
    source_index: Option<usize>,
) -> Result<(), DiagnosticSet> {
    let Some(object) = options.as_object() else {
        return Ok(());
    };
    let allowed = providers
        .into_iter()
        .flat_map(|provider| provider.descriptor().option_keys.iter().copied())
        .collect::<BTreeSet<_>>();
    let mut diagnostics = DiagnosticSet::empty();
    for key in object.keys().filter(|key| !allowed.contains(key.as_str())) {
        diagnostics.push(directory_option_diagnostic(config_path, source_index, key));
    }
    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(diagnostics)
    }
}

fn directory_option_diagnostic(
    config_path: &Path,
    source_index: Option<usize>,
    key: &str,
) -> Diagnostic {
    let mut key_path = Vec::new();
    if let Some(index) = source_index {
        key_path.extend(["sources".to_string(), index.to_string()]);
    }
    key_path.push(key.to_string());
    Diagnostic {
        code: "PROJECT-001".to_string(),
        stage: "PROJECT".to_string(),
        severity: Severity::Error,
        message: format!("unknown directory source option `{key}`"),
        primary: Some(Label {
            location: SourceLocation::ProjectConfig {
                path: PathBuf::from(config_path),
                key_path,
            },
            message: None,
        }),
        related: Vec::new(),
    }
}

const fn source_ref<'a>(
    source: &'a ConfiguredSource,
    source_type: Option<&'a str>,
    option_keys: &'a [&'a str],
) -> ProjectSourceRef<'a> {
    ProjectSourceRef {
        source_type,
        location: &source.location,
        option_keys,
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
    spec: &ConfiguredSource,
    error: SourceProviderSelectionError,
) -> Diagnostic {
    let SourceLocationSpec::Path(path) = &spec.location;
    let source = path.display().to_string();
    match error {
        SourceProviderSelectionError::UnknownSourceProvider { id } => project_diagnostic(
            config_path,
            format!("source `{source}` uses unknown source provider `{id}`"),
        ),
        SourceProviderSelectionError::NoSourceProvider => project_diagnostic(
            config_path,
            format!("source `{source}` has no matching source provider"),
        ),
        SourceProviderSelectionError::AmbiguousSourceProviders { ids } => project_diagnostic(
            config_path,
            format!(
                "source `{source}` matches multiple source providers {}; set source `type` explicitly",
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

#[cfg(test)]
mod tests {
    use super::{decode_configured_source, validate_resolved_source, ConfiguredSource};
    use coflow_api::{
        DecodedSourceOptions, DiagnosticSet, LoadedSource, ProbeResult, ProjectSourceRef,
        ResolvedSource, SourceLoadContext, SourceLocationSpec, SourceProvider,
        SourceProviderDescriptor,
    };
    use serde_json::Value;
    use std::path::Path;

    static DESCRIPTOR: SourceProviderDescriptor = SourceProviderDescriptor {
        id: "contract-test",
        display_name: "Contract test",
        extensions: &[],
        option_keys: &[],
    };

    #[derive(Debug)]
    struct ContractProvider {
        decoded_provider_id: &'static str,
    }

    impl SourceProvider for ContractProvider {
        fn descriptor(&self) -> &'static SourceProviderDescriptor {
            &DESCRIPTOR
        }

        fn probe(&self, _source: &ProjectSourceRef<'_>) -> ProbeResult {
            ProbeResult::certain()
        }

        fn decode_options(&self, _options: &Value) -> Result<DecodedSourceOptions, DiagnosticSet> {
            Ok(DecodedSourceOptions::new(self.decoded_provider_id, ()))
        }

        fn load(
            &self,
            _ctx: SourceLoadContext<'_>,
            _source: &ResolvedSource,
        ) -> Result<LoadedSource, DiagnosticSet> {
            Ok(LoadedSource {
                records: Vec::new(),
            })
        }
    }

    #[test]
    fn decoded_options_must_belong_to_selected_provider() {
        let provider = ContractProvider {
            decoded_provider_id: "other-provider",
        };
        let configured = ConfiguredSource {
            provider_id: DESCRIPTOR.id.to_string(),
            location: SourceLocationSpec::Path("contract.source".into()),
            options: Value::Null,
            display_name: "contract.source".to_string(),
            source_index: Some(0),
        };
        let result = decode_configured_source(&provider, &configured, Path::new("coflow.yaml"));
        assert!(result.is_err(), "foreign decoded options must be rejected");
        let Err(diagnostics) = result else {
            return;
        };
        assert_eq!(diagnostics.diagnostics[0].code, "PROVIDER-OPTIONS-CONTRACT");
    }

    #[test]
    fn resolved_source_identity_must_match_selected_provider() {
        let provider = ContractProvider {
            decoded_provider_id: DESCRIPTOR.id,
        };
        let source = ResolvedSource {
            provider_id: "other-provider".to_string(),
            location: SourceLocationSpec::Path("contract.source".into()),
            options: DecodedSourceOptions::new(DESCRIPTOR.id, ()),
            display_name: "contract.source".to_string(),
        };
        let result = validate_resolved_source(&provider, &source);
        assert!(result.is_err(), "foreign resolved source must be rejected");
        let Err(diagnostics) = result else {
            return;
        };
        assert_eq!(diagnostics.diagnostics[0].code, "PROVIDER-SOURCE-CONTRACT");
    }
}
