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
mod paths;
mod schema_path_policy;
mod schema_sources;
mod validation;

pub use config::{
    DimensionConfig, OutputConfig, OutputsConfig, ProjectConfig, SchemaConfig, SourceConfig,
};
pub use diagnostics::{dedupe_cft_diagnostics, diagnostic_set_from_cft};
pub use paths::{normalize_path, path_to_slash, resolve_config_path};
pub use schema_path_policy::SchemaFile;

use validation::{
    validate_for_codegen_collecting, validate_project_config_schema_only_collecting,
    validate_sources_collecting,
};

use coflow_api::DiagnosticSet;
use coflow_cft::{CftContainer, CftDiagnostic, ModuleId};
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read};
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
}

fn resolve_project_relative(root_dir: &Path, path: &Path) -> PathBuf {
    paths::resolve_project_relative(root_dir, path)
}

#[derive(Debug)]
pub struct SchemaBuild {
    pub container: Option<CftContainer>,
    pub diagnostics: Vec<CftDiagnostic>,
    pub sources: BTreeMap<String, String>,
    pub paths: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct SchemaSourceOverride {
    pub requested_module: Option<String>,
    pub normalized_path: PathBuf,
    pub source: String,
}

/// Compiles the project's configured CFT schema files.
///
/// # Errors
///
/// Default `coflow.yaml` template installed by [`init_project`]. Kept as a
/// constant so the CLI and the editor-side init command share the exact
/// same project layout.
pub const DEFAULT_PROJECT_YAML: &str = r"schema: schema/

sources: []

outputs:
  data:
    type: json
    dir: generated/data
  code:
    type: csharp
    dir: generated/csharp
    namespace: Game.Config
";

/// Outcome of [`init_project`]: where the new `coflow.yaml` lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitOutcome {
    pub config_path: PathBuf,
}

/// Create a minimal Coflow project rooted at `dir`. Identical to the CLI's
/// `coflow init` so the editor can offer "新建工程" without spawning a
/// subprocess.
///
/// Layout:
/// - `coflow.yaml` with the default template (see [`DEFAULT_PROJECT_YAML`]),
/// - `schema/` directory for `.cft` files,
/// - `data/` directory for source data,
/// - `generated/data/` and `generated/csharp/` directories for build
///   artefacts.
///
/// # Errors
/// Returns a human-readable error when `coflow.yaml` already exists in
/// `dir` (refuses to overwrite) or when any directory or file cannot be
/// created.
pub fn init_project(dir: impl AsRef<Path>) -> Result<InitOutcome, DiagnosticSet> {
    let dir = dir.as_ref();
    let config_path = dir.join("coflow.yaml");
    if config_path.exists() {
        return Err(diagnostics::file_error(
            &config_path,
            "PROJECT-INIT-IO",
            "PROJECT",
            format!("`{}` already exists", config_path.display()),
        ));
    }
    fs::create_dir_all(dir.join("schema")).map_err(|err| {
        diagnostics::file_error(
            &dir.join("schema"),
            "PROJECT-INIT-IO",
            "PROJECT",
            format!("failed to create `{}`: {err}", dir.join("schema").display()),
        )
    })?;
    fs::create_dir_all(dir.join("data")).map_err(|err| {
        diagnostics::file_error(
            &dir.join("data"),
            "PROJECT-INIT-IO",
            "PROJECT",
            format!("failed to create `{}`: {err}", dir.join("data").display()),
        )
    })?;
    fs::create_dir_all(dir.join("generated").join("data")).map_err(|err| {
        diagnostics::file_error(
            &dir.join("generated").join("data"),
            "PROJECT-INIT-IO",
            "PROJECT",
            format!(
            "failed to create `{}`: {err}",
            dir.join("generated").join("data").display()
            ),
        )
    })?;
    fs::create_dir_all(dir.join("generated").join("csharp")).map_err(|err| {
        diagnostics::file_error(
            &dir.join("generated").join("csharp"),
            "PROJECT-INIT-IO",
            "PROJECT",
            format!(
            "failed to create `{}`: {err}",
            dir.join("generated").join("csharp").display()
            ),
        )
    })?;
    fs::write(&config_path, DEFAULT_PROJECT_YAML).map_err(|err| {
        diagnostics::file_error(
            &config_path,
            "PROJECT-INIT-IO",
            "PROJECT",
            format!("failed to write `{}`: {err}", config_path.display()),
        )
    })?;
    Ok(InitOutcome { config_path })
}

/// Compile the schema for a project.
///
/// # Errors
///
/// Returns an error when project schema paths cannot be read or when stdin
/// schema input cannot be consumed.
pub fn compile_schema_project(
    project: &Project,
    stdin_path: Option<&Path>,
) -> Result<SchemaBuild, DiagnosticSet> {
    let overrides = if let Some(path) = stdin_path {
        let mut source = String::new();
        io::stdin()
            .read_to_string(&mut source)
            .map_err(|err| {
                diagnostics::plain_error(
                    "CLI-STDIN",
                    "CLI",
                    format!("failed to read stdin: {err}"),
                )
            })?;
        let requested = path_to_slash(path);
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            project.root_dir.join(path)
        };
        vec![SchemaSourceOverride {
            requested_module: Some(requested),
            normalized_path: normalize_path(&absolute),
            source,
        }]
    } else {
        Vec::new()
    };
    compile_schema_project_with_overrides(project, &overrides)
}

/// Compiles the project's schema files with in-memory source overrides.
///
/// # Errors
///
/// Returns an error when schema files cannot be discovered/read, an override
/// does not match any schema module, or schema compilation reports diagnostics
/// without a previously compiled container.
pub fn compile_schema_project_with_overrides(
    project: &Project,
    overrides: &[SchemaSourceOverride],
) -> Result<SchemaBuild, DiagnosticSet> {
    let schema_files = project.schema_files()?;
    let mut matched_overrides = vec![false; overrides.len()];
    let mut sources = BTreeMap::new();
    let mut paths = BTreeMap::new();
    let mut container = CftContainer::new();
    let mut diagnostics = Vec::new();

    for schema_file in schema_files {
        let source = if let Some((index, source_override)) = overrides
            .iter()
            .enumerate()
            .rev()
            .find(|(_, source_override)| {
                source_override
                    .requested_module
                    .as_deref()
                    .is_some_and(|module| module == schema_file.module_id)
                    || normalize_path(&schema_file.canonical_path)
                        == source_override.normalized_path
            }) {
            matched_overrides[index] = true;
            source_override.source.clone()
        } else {
            fs::read_to_string(&schema_file.path).map_err(|err| {
                diagnostics::file_error(
                    &schema_file.path,
                    "PROJECT-SCHEMA-READ",
                    "PROJECT",
                    format!("failed to read `{}`: {err}", schema_file.path.display()),
                )
            })?
        };
        sources.insert(schema_file.module_id.clone(), source.clone());
        paths.insert(
            schema_file.module_id.clone(),
            schema_file.canonical_path.display().to_string(),
        );
        if let Err(errors) = container.add_module(ModuleId::new(schema_file.module_id), source) {
            diagnostics.extend(errors.diagnostics);
        }
    }

    for (index, matched) in matched_overrides.into_iter().enumerate() {
        if !matched {
            let source_override = &overrides[index];
            let requested = source_override.requested_module.as_deref().map_or_else(
                || source_override.normalized_path.display().to_string(),
                str::to_string,
            );
            return Err(diagnostics::plain_error(
                "SCHEMA-STDIN-PATH",
                "SCHEMA",
                format!("`--stdin-path {requested}` is not part of the configured schema"),
            ));
        }
    }

    let compiled = if diagnostics.is_empty() {
        match container.compile() {
            Ok(()) => Some(container),
            Err(errors) => {
                diagnostics.extend(errors.diagnostics);
                None
            }
        }
    } else {
        None
    };

    Ok(SchemaBuild {
        container: compiled,
        diagnostics,
        sources,
        paths,
    })
}
