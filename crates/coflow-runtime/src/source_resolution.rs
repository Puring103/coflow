use std::path::Path;

use crate::api::{
    CfdSource, CfdSourceCatalog, CfdSourcePath, Diagnostic, DiagnosticSet, Label, Severity,
    SourceLocation,
};
use crate::project::{discover_directory_files, path_is_same_or_descendant, Project, SourceConfig};

mod dimensions;

pub(crate) struct ResolvedLoaderSource {
    pub(crate) source: CfdSource,
}

pub(crate) struct ResolvedDimensionSource {
    pub(crate) source: CfdSource,
    pub(crate) fields: Vec<crate::dimensions::DimensionField>,
}

#[derive(Clone)]
pub(crate) struct ConfiguredSource {
    pub(crate) location: CfdSourcePath,
    pub(crate) display_name: String,
}

pub(crate) struct SourceResolver<'a> {
    project: &'a Project,
    catalog: &'a CfdSourceCatalog,
}

impl<'a> SourceResolver<'a> {
    pub(crate) const fn new(project: &'a Project, catalog: &'a CfdSourceCatalog) -> Self {
        Self { project, catalog }
    }

    pub(crate) fn configured(&self, source: &SourceConfig) -> ConfiguredSource {
        configured_source(self.project, source)
    }

    pub(crate) fn resolve_for_load(
        &self,
        _source: &SourceConfig,
        configured: &ConfiguredSource,
    ) -> Result<Vec<ResolvedLoaderSource>, DiagnosticSet> {
        if configured.location.path().is_dir() {
            return self.resolve_directory(configured);
        }
        self.resolve_file(configured)
    }

    pub(crate) fn resolve_implicit(
        &self,
        configured: &ConfiguredSource,
    ) -> Result<Vec<ResolvedLoaderSource>, DiagnosticSet> {
        if configured.location.path().is_dir() {
            return self.resolve_directory(configured);
        }
        self.resolve_file(configured)
    }

    pub(crate) fn resolve_dimension_sources(
        &self,
        plan: &crate::dimensions::DimensionRuntimePlan,
    ) -> Result<Vec<ResolvedDimensionSource>, DiagnosticSet> {
        dimensions::resolve_dimension_sources(self, plan)
    }

    fn resolve_directory(
        &self,
        configured: &ConfiguredSource,
    ) -> Result<Vec<ResolvedLoaderSource>, DiagnosticSet> {
        let directory = configured.location.path();
        let files = discover_directory_files(directory).map_err(|error| {
            DiagnosticSet::one(project_diagnostic(
                self.project.config_path(),
                error.to_string(),
            ))
        })?;
        let managed_dimension_dirs = self
            .project
            .config()
            .dimensions
            .values()
            .filter_map(|config| config.out_dir.as_ref())
            .map(|out_dir| self.project.resolve_path(out_dir))
            .collect::<Vec<_>>();
        let mut resolved = Vec::new();
        for path in files.into_iter().filter(|path| {
            !managed_dimension_dirs
                .iter()
                .any(|out_dir| path_is_same_or_descendant(path, out_dir))
        }) {
            if path.extension().and_then(|extension| extension.to_str()) != Some("cfd") {
                continue;
            }
            let file_source = ConfiguredSource {
                display_name: path.display().to_string(),
                location: CfdSourcePath::new(path.clone()),
            };
            resolved.extend(self.resolve_file(&file_source)?);
        }
        Ok(resolved)
    }

    fn resolve_file(
        &self,
        configured: &ConfiguredSource,
    ) -> Result<Vec<ResolvedLoaderSource>, DiagnosticSet> {
        let source = CfdSource {
            location: configured.location.clone(),
            display_name: configured.display_name.clone(),
        };
        self.catalog
            .loader()
            .resolve(&source)
            .map(|source| vec![ResolvedLoaderSource { source }])
    }
}

fn configured_source(project: &Project, source: &SourceConfig) -> ConfiguredSource {
    let path = source.location();
    let location = CfdSourcePath::new(project.resolve_path(path));
    let display_name = path.display().to_string();
    ConfiguredSource {
        location,
        display_name,
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
        contexts: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn configured_source_keeps_project_relative_display_name() {
        use super::configured_source;
        use crate::project::{Project, SourceConfig};
        use std::path::PathBuf;

        let config =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/cfd/coflow.yaml");
        let project =
            Project::open_schema_only(Some(config.as_path())).expect("example project should open");
        let configured = configured_source(&project, &SourceConfig::from_path("data.cfd".into()));
        assert_eq!(configured.display_name, "data.cfd");
        assert!(configured.location.path().ends_with("data.cfd"));
    }
}
