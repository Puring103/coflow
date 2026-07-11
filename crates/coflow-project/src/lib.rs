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
mod diagnostics;
mod init;
mod paths;
mod schema_path_policy;
mod schema_sources;
mod validation;

pub use config::{
    DimensionConfig, OutputConfig, OutputsConfig, ProjectConfig, SchemaConfig, SourceConfig,
};
pub use diagnostics::{dedupe_cft_diagnostics, diagnostic_set_from_cft};
pub use init::{init_project, InitOutcome, DEFAULT_PROJECT_YAML};
pub use paths::{normalize_path, path_to_slash, resolve_config_path};
pub use schema_path_policy::SchemaFile;
pub use schema_sources::{SchemaSource, SchemaSourceSet};

use validation::{
    validate_for_codegen_collecting, validate_project_config_schema_only_collecting,
    validate_sources_collecting,
};

use coflow_api::DiagnosticSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Project {
    pub config_path: PathBuf,
    pub root_dir: PathBuf,
    pub config: ProjectConfig,
}

impl Project {
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
        project.validate_for_data()?;
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
        let config_path = fs::canonicalize(&config_path).map_err(|err| {
            diagnostics::file_error(
                &config_path,
                "PROJECT-CONFIG-PATH",
                "PROJECT",
                format!("failed to resolve config `{}`: {err}", config_path.display()),
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
            config_path,
            root_dir,
            config,
        })
    }

    /// Validates source settings required by data loading commands.
    ///
    /// # Errors
    ///
    /// Returns an error when a data source file or directory is missing or a
    /// data-stage source/sheet setting is invalid.
    pub fn validate_for_data(&self) -> Result<(), DiagnosticSet> {
        let diagnostics = self.data_diagnostic_set();
        if diagnostics.is_empty() {
            Ok(())
        } else {
            Err(diagnostics)
        }
    }

    /// Validates output settings required by C# code generation.
    ///
    /// # Errors
    ///
    /// Returns an error when code or data output settings are missing or have
    /// invalid shape.
    pub fn validate_for_codegen(&self) -> Result<(), DiagnosticSet> {
        let diagnostics = self.codegen_diagnostic_set();
        if diagnostics.is_empty() {
            Ok(())
        } else {
            Err(diagnostics)
        }
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
            validate_sources_collecting(&self.root_dir, &self.config.sources),
        )
    }

    #[must_use]
    pub fn codegen_diagnostic_set(&self) -> DiagnosticSet {
        diagnostics::project_diagnostics_to_set(
            &self.config_path,
            validate_for_codegen_collecting(&self.config.outputs),
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
    pub fn schema_sources(&self) -> Result<SchemaSourceSet, DiagnosticSet> {
        schema_sources::schema_sources(&self.config.schema, &self.root_dir)
    }
}

fn resolve_project_relative(root_dir: &Path, path: &Path) -> PathBuf {
    paths::resolve_project_relative(root_dir, path)
}
