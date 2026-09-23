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

mod config;
mod config_write;
mod diagnostics;
mod file_discovery;
mod init;
mod paths;
mod schema_path_policy;
mod schema_sources;
mod validation;

pub use crate::api::path_to_slash;
pub use config::{OutputConfig, ProjectConfig, SchemaConfig, SourceConfig};
pub use config_write::{
    add_project_input, create_project_file, delete_project_entry, ProjectInputKind,
};
pub use file_discovery::{discover_directory_files, DirectoryDiscoveryError};
pub use init::{init_project, InitOutcome, DEFAULT_PROJECT_YAML};
pub use paths::{
    canonicalize_path, normalize_path, normalized_path_identity, path_is_same_or_descendant,
    project_path, resolve_config_path, resolve_existing_or_future_path,
};
pub use schema_path_policy::SchemaFile;
pub use schema_sources::SchemaSource;

use validation::{
    validate_codegen_collecting, validate_project_config_schema_only_collecting,
    validate_sources_collecting,
};

use crate::api::DiagnosticSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct Project {
    config_path: PathBuf,
    source_store: Arc<crate::CfdSourceStore>,
    root_dir: PathBuf,
    pub(crate) config: ProjectConfig,
    pub(crate) config_source: Arc<str>,
}

impl Project {
    /// CFD 目录仅在文件事件后重新发现，语言请求期间复用同一来源目录。
    pub fn data_source_files(&self) -> Result<Arc<[PathBuf]>, DiagnosticSet> {
        let mut paths = self
            .source_store
            .paths
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(paths) = paths.as_ref() {
            return Ok(Arc::clone(paths));
        }
        let resolver = crate::source_resolution::SourceResolver::new(self);
        let mut files = std::collections::BTreeSet::new();
        for source in &self.config.data {
            for resolved in resolver.resolve_for_load(source, &resolver.configured(source))? {
                files.insert(normalize_path(resolved.source.location.path()));
            }
        }
        let files: Arc<[PathBuf]> = files.into_iter().collect::<Vec<_>>().into();
        *paths = Some(Arc::clone(&files));
        Ok(files)
    }

    /// 项目的磁盘与覆盖快照供加载器、编辑器和 LSP 共享。
    pub fn source_store(&self) -> &Arc<crate::CfdSourceStore> {
        &self.source_store
    }

    #[must_use]
    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    #[must_use]
    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }

    #[must_use]
    pub const fn config(&self) -> &ProjectConfig {
        &self.config
    }

    /// Returns the CFD paths from the CFD-only project contract.
    #[must_use]
    pub fn data_paths(&self) -> &[SourceConfig] {
        &self.config.data
    }

    /// Opens a Coflow project by resolving and parsing its config file.
    ///
    /// # Errors
    ///
    /// Returns an error when the config path cannot be found, read,
    /// canonicalized, or parsed as YAML.
    pub fn open(config_or_dir: Option<&Path>) -> Result<Self, DiagnosticSet> {
        let project = Self::open_schema_only(config_or_dir)?;
        let schema_diagnostics = project.schema_diagnostic_set();
        if !schema_diagnostics.is_empty() {
            return Err(schema_diagnostics);
        }
        let data_diagnostics = project.data_diagnostic_set();
        if !data_diagnostics.is_empty() {
            return Err(data_diagnostics);
        }
        Ok(project)
    }

    /// Opens a Coflow project without validating data-stage source files.
    ///
    /// # Errors
    ///
    /// Returns an error when the config path cannot be found, read,
    /// canonicalized, or parsed as YAML.
    pub fn open_schema_only(config_or_dir: Option<&Path>) -> Result<Self, DiagnosticSet> {
        let config_path = resolve_config_path(config_or_dir)?;
        let config_path = canonicalize_path(&config_path).map_err(|err| {
            diagnostics::file_error(
                &config_path,
                "PROJECT-CONFIG-PATH",
                "PROJECT",
                format!(
                    "failed to resolve config `{}`: {err}",
                    config_path.display()
                ),
            )
        })?;
        let root_dir = config_path.parent().ok_or_else(|| {
            diagnostics::file_error(
                &config_path,
                "PROJECT-CONFIG-PATH",
                "PROJECT",
                format!("config `{}` has no parent directory", config_path.display()),
            )
        })?;
        let root_dir = root_dir.to_path_buf();
        let source = fs::read_to_string(&config_path).map_err(|err| {
            diagnostics::file_error(
                &config_path,
                "PROJECT-CONFIG-READ",
                "PROJECT",
                format!("failed to read `{}`: {err}", config_path.display()),
            )
        })?;
        let config = serde_yaml::from_str(&source).map_err(|err| {
            diagnostics::file_error(
                &config_path,
                "PROJECT-CONFIG-PARSE",
                "PROJECT",
                format!("failed to parse `{}`: {err}", config_path.display()),
            )
        })?;
        Ok(Self {
            source_store: Arc::default(),
            config_path,
            root_dir,
            config,
            config_source: source.into(),
        })
    }

    #[must_use]
    pub fn schema_diagnostic_set(&self) -> DiagnosticSet {
        diagnostics::project_diagnostics_to_set(
            &self.config_path,
            validate_project_config_schema_only_collecting(&self.root_dir, &self.config),
        )
    }

    #[must_use]
    pub fn data_diagnostic_set(&self) -> DiagnosticSet {
        diagnostics::project_diagnostics_to_set(
            &self.config_path,
            validate_sources_collecting(&self.root_dir, &self.config.data),
        )
    }

    #[must_use]
    pub fn codegen_diagnostic_set(&self) -> DiagnosticSet {
        diagnostics::project_diagnostics_to_set(
            &self.config_path,
            validate_codegen_collecting(&self.config.codegen),
        )
    }

    #[must_use]
    pub fn resolve_path(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root_dir.join(path)
        }
    }

    /// Returns the configured source roots whose changes can affect the project.
    #[must_use]
    pub fn source_roots(&self) -> Vec<PathBuf> {
        let mut roots = self
            .config
            .schema
            .paths()
            .iter()
            .map(|path| self.resolve_path(path))
            .chain(
                self.config
                    .data
                    .iter()
                    .map(|source| self.resolve_path(source.path())),
            )
            .map(|path| normalize_path(&path))
            .collect::<Vec<_>>();
        roots.sort();
        roots.dedup();
        roots
    }

    /// Reports whether a path is the project config or belongs to a declared source root.
    #[must_use]
    pub fn tracks_path(&self, path: &Path) -> bool {
        let path = normalize_path(path);
        path == normalize_path(&self.config_path)
            || self
                .source_roots()
                .iter()
                .any(|root| path_is_same_or_descendant(&path, root))
    }

    /// Returns all schema files configured for this project.
    ///
    /// # Errors
    ///
    /// Returns an error when a configured schema path does not exist or a schema
    /// directory cannot be read.
    pub fn schema_files(&self) -> Result<Vec<SchemaFile>, DiagnosticSet> {
        schema_sources::schema_files(&self.config.schema, &self.root_dir)
    }

    /// Reads configured schema modules without compiling CFT semantics.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when schema discovery or source reads fail.
    pub fn schema_sources(&self) -> Result<Vec<SchemaSource>, DiagnosticSet> {
        schema_sources::schema_sources(&self.config.schema, &self.root_dir)
    }
}
