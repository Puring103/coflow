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
mod validation;

pub use config::{
    DimensionConfig, OutputConfig, OutputsConfig, ProjectConfig, SchemaConfig, SourceConfig,
};

use validation::{
    validate_for_codegen_collecting, validate_project_config_schema_only_collecting,
    validate_sources_collecting, ProjectDiagnostic,
};

use coflow_api::{Diagnostic, DiagnosticSet, Label, Severity, SourceLocation};
use coflow_cft::{CftContainer, CftDiagnostic, CftLabel, ModuleId};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

const PROJECT_DIAGNOSTIC_STAGE: &str = "PROJECT";

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
    pub fn open(config_or_dir: Option<&Path>) -> Result<Self, String> {
        let project = Self::open_schema_only(config_or_dir)?;
        let schema_diagnostics = project.schema_diagnostic_set();
        if !schema_diagnostics.is_empty() {
            return Err(schema_diagnostics
                .diagnostics
                .into_iter()
                .map(|diagnostic| diagnostic.message)
                .collect::<Vec<_>>()
                .join("\n"));
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
    pub fn open_schema_only(config_or_dir: Option<&Path>) -> Result<Self, String> {
        let config_path = resolve_config_path(config_or_dir)?;
        let config_path = fs::canonicalize(&config_path).map_err(|err| {
            format!(
                "failed to resolve config `{}`: {err}",
                config_path.display()
            )
        })?;
        let root_dir = config_path
            .parent()
            .ok_or_else(|| format!("config `{}` has no parent directory", config_path.display()))?
            .to_path_buf();
        let source = fs::read_to_string(&config_path)
            .map_err(|err| format!("failed to read `{}`: {err}", config_path.display()))?;
        let config = serde_yaml::from_str(&source)
            .map_err(|err| format!("failed to parse `{}`: {err}", config_path.display()))?;
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
    pub fn validate_for_data(&self) -> Result<(), String> {
        let diagnostics = self.data_diagnostic_set();
        if diagnostics.is_empty() {
            Ok(())
        } else {
            Err(join_diagnostic_messages(diagnostics))
        }
    }

    /// Validates output settings required by C# code generation.
    ///
    /// # Errors
    ///
    /// Returns an error when code or data output settings are missing or have
    /// invalid shape.
    pub fn validate_for_codegen(&self) -> Result<(), String> {
        let diagnostics = self.codegen_diagnostic_set();
        if diagnostics.is_empty() {
            Ok(())
        } else {
            Err(join_diagnostic_messages(diagnostics))
        }
    }

    #[must_use]
    pub fn schema_diagnostic_set(&self) -> DiagnosticSet {
        project_diagnostics_to_set(
            &self.config_path,
            validate_project_config_schema_only_collecting(&self.root_dir, &self.config),
        )
    }

    #[must_use]
    pub fn data_diagnostic_set(&self) -> DiagnosticSet {
        project_diagnostics_to_set(
            &self.config_path,
            validate_sources_collecting(&self.root_dir, &self.config.sources),
        )
    }

    #[must_use]
    pub fn codegen_diagnostic_set(&self) -> DiagnosticSet {
        project_diagnostics_to_set(
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
    pub fn schema_files(&self) -> Result<Vec<SchemaFile>, String> {
        let mut files = Vec::new();
        let mut errors = Vec::new();
        match &self.config.schema {
            SchemaConfig::One(path) => {
                if let Err(err) = self.push_schema_path(path, &mut files) {
                    errors.push(err);
                }
            }
            SchemaConfig::Many(paths) => {
                for path in paths {
                    if let Err(err) = self.push_schema_path(path, &mut files) {
                        errors.push(err);
                    }
                }
            }
        }
        if !errors.is_empty() {
            return Err(errors.join("\n"));
        }
        files.sort_by(|left, right| left.module_id.cmp(&right.module_id));
        Ok(files)
    }

    fn push_schema_path(&self, path: &Path, files: &mut Vec<SchemaFile>) -> Result<(), String> {
        let path = self.resolve_path(path);
        if path.is_dir() {
            collect_cft_files(&path, files, &self.root_dir)
        } else if path.is_file() {
            if !is_cft_path(&path) {
                return Err(format!(
                    "schema file `{}` has unsupported extension",
                    path_to_slash(path.strip_prefix(&self.root_dir).unwrap_or(&path))
                ));
            }
            files.push(SchemaFile::new(path, &self.root_dir)?);
            Ok(())
        } else {
            Err(format!("schema path `{}` does not exist", path.display()))
        }
    }
}

fn resolve_project_relative(root_dir: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root_dir.join(path)
    }
}

#[derive(Debug)]
pub struct SchemaFile {
    pub path: PathBuf,
    pub canonical_path: PathBuf,
    pub module_id: String,
}

impl SchemaFile {
    fn new(path: PathBuf, root_dir: &Path) -> Result<Self, String> {
        let canonical_path = fs::canonicalize(&path)
            .map_err(|err| format!("failed to resolve schema `{}`: {err}", path.display()))?;
        let module_path = canonical_path
            .strip_prefix(root_dir)
            .unwrap_or(canonical_path.as_path());
        let module_id = path_to_slash(module_path);
        Ok(Self {
            path,
            canonical_path,
            module_id,
        })
    }
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
pub fn init_project(dir: impl AsRef<Path>) -> Result<InitOutcome, String> {
    let dir = dir.as_ref();
    let config_path = dir.join("coflow.yaml");
    if config_path.exists() {
        return Err(format!("`{}` already exists", config_path.display()));
    }
    fs::create_dir_all(dir.join("schema"))
        .map_err(|err| format!("failed to create `{}`: {err}", dir.join("schema").display()))?;
    fs::create_dir_all(dir.join("data"))
        .map_err(|err| format!("failed to create `{}`: {err}", dir.join("data").display()))?;
    fs::create_dir_all(dir.join("generated").join("data")).map_err(|err| {
        format!(
            "failed to create `{}`: {err}",
            dir.join("generated").join("data").display()
        )
    })?;
    fs::create_dir_all(dir.join("generated").join("csharp")).map_err(|err| {
        format!(
            "failed to create `{}`: {err}",
            dir.join("generated").join("csharp").display()
        )
    })?;
    fs::write(&config_path, DEFAULT_PROJECT_YAML)
        .map_err(|err| format!("failed to write `{}`: {err}", config_path.display()))?;
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
) -> Result<SchemaBuild, String> {
    let overrides = if let Some(path) = stdin_path {
        let mut source = String::new();
        io::stdin()
            .read_to_string(&mut source)
            .map_err(|err| format!("failed to read stdin: {err}"))?;
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
) -> Result<SchemaBuild, String> {
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
            fs::read_to_string(&schema_file.path)
                .map_err(|err| format!("failed to read `{}`: {err}", schema_file.path.display()))?
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
            return Err(format!(
                "`--stdin-path {requested}` is not part of the configured schema"
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

/// Resolves a config file path from an explicit path, directory, or current directory.
///
/// # Errors
///
/// Returns an error when the requested config file/directory cannot be resolved
/// to `coflow.yaml` or `coflow.yml`.
pub fn resolve_config_path(config_or_dir: Option<&Path>) -> Result<PathBuf, String> {
    let candidate = config_or_dir.unwrap_or_else(|| Path::new("."));
    if config_or_dir.is_some() && candidate.is_file() {
        return Ok(candidate.to_path_buf());
    }
    if config_or_dir.is_some() && !candidate.exists() {
        if is_yaml_path(candidate) {
            return Ok(candidate.to_path_buf());
        }
        return Err(format!(
            "config or directory `{}` does not exist",
            candidate.display()
        ));
    }
    let dir = if candidate.is_dir() {
        candidate
    } else if is_yaml_path(candidate) {
        return Ok(candidate.to_path_buf());
    } else {
        return Err(format!(
            "`{}` is neither a config file nor a directory",
            candidate.display()
        ));
    };
    find_default_config(dir)
}

fn find_default_config(dir: &Path) -> Result<PathBuf, String> {
    let yaml_path = dir.join("coflow.yaml");
    let yml_path = dir.join("coflow.yml");
    match (yaml_path.exists(), yml_path.exists()) {
        (true, false) => Ok(yaml_path),
        (false, true) => Ok(yml_path),
        (true, true) => Err(format!(
            "both `{}` and `{}` exist; specify the config file explicitly",
            yaml_path.display(),
            yml_path.display()
        )),
        (false, false) => Err(format!(
            "no coflow.yaml or coflow.yml found in `{}`",
            dir.display()
        )),
    }
}

fn is_yaml_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| matches!(ext, "yaml" | "yml"))
}

fn is_cft_path(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("cft")
}

fn collect_cft_files(
    dir: &Path,
    files: &mut Vec<SchemaFile>,
    root_dir: &Path,
) -> Result<(), String> {
    let mut entries = fs::read_dir(dir)
        .map_err(|err| format!("failed to read schema directory `{}`: {err}", dir.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| format!("failed to read schema directory `{}`: {err}", dir.display()))?;
    entries.sort_by_key(fs::DirEntry::path);
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_cft_files(&path, files, root_dir)?;
        } else if is_cft_path(&path) {
            files.push(SchemaFile::new(path, root_dir)?);
        }
    }
    Ok(())
}

#[must_use]
pub fn dedupe_cft_diagnostics(diagnostics: Vec<CftDiagnostic>) -> Vec<CftDiagnostic> {
    let mut keys = BTreeSet::new();
    let mut out = Vec::new();
    for diagnostic in diagnostics {
        if keys.insert(cft_diagnostic_key(&diagnostic)) {
            out.push(diagnostic);
        }
    }
    out
}

fn cft_diagnostic_key(diagnostic: &CftDiagnostic) -> String {
    let mut key = format!(
        "{}\n{}\n{}\n",
        diagnostic.code.as_str(),
        diagnostic.stage,
        diagnostic.message
    );
    if let Some(primary) = &diagnostic.primary {
        push_cft_label_key(&mut key, primary);
    }
    for related in &diagnostic.related {
        push_cft_label_key(&mut key, related);
    }
    key
}

fn push_cft_label_key(key: &mut String, label: &CftLabel) {
    key.push_str(label.module.as_str());
    key.push(':');
    key.push_str(&label.span.start.to_string());
    key.push(':');
    key.push_str(&label.span.end.to_string());
    key.push(':');
    if let Some(message) = &label.message {
        key.push_str(message);
    }
    key.push('\n');
}

#[must_use]
pub fn diagnostic_set_from_cft(
    diagnostics: Vec<CftDiagnostic>,
    sources: &BTreeMap<String, String>,
    paths: &BTreeMap<String, String>,
) -> DiagnosticSet {
    DiagnosticSet {
        diagnostics: diagnostics
            .into_iter()
            .map(|diagnostic| diagnostic_from_cft(diagnostic, sources, paths))
            .collect(),
    }
}

fn diagnostic_from_cft(
    diagnostic: CftDiagnostic,
    sources: &BTreeMap<String, String>,
    paths: &BTreeMap<String, String>,
) -> Diagnostic {
    Diagnostic {
        code: diagnostic.code.as_str().to_string(),
        stage: diagnostic.stage.to_string(),
        severity: Severity::Error,
        message: diagnostic.message,
        primary: diagnostic
            .primary
            .as_ref()
            .map(|label| label_from_cft(label, sources, paths)),
        related: diagnostic
            .related
            .iter()
            .map(|label| label_from_cft(label, sources, paths))
            .collect(),
    }
}

fn label_from_cft(
    label: &CftLabel,
    sources: &BTreeMap<String, String>,
    paths: &BTreeMap<String, String>,
) -> Label {
    let range = cft_label_range(label, sources);
    let path = paths
        .get(label.module.as_str())
        .map_or_else(|| PathBuf::from(label.module.as_str()), PathBuf::from);
    Label {
        location: SourceLocation::FileSpan {
            path,
            start_line: range.start.line,
            start_character: range.start.character,
            end_line: range.end.line,
            end_character: range.end.character,
        },
        message: label.message.clone(),
    }
}

#[derive(Debug, Clone, Copy)]
struct Range {
    start: Position,
    end: Position,
}

#[derive(Debug, Clone, Copy)]
struct Position {
    line: usize,
    character: usize,
}

fn cft_label_range(label: &CftLabel, sources: &BTreeMap<String, String>) -> Range {
    let source = sources
        .get(label.module.as_str())
        .map_or("", String::as_str);
    Range {
        start: byte_position(source, label.span.start),
        end: byte_position(source, label.span.end.max(label.span.start + 1)),
    }
}

fn byte_position(source: &str, byte_offset: usize) -> Position {
    let target = byte_offset.min(source.len());
    let mut line = 0;
    let mut character = 0;
    for (byte_index, ch) in source.char_indices() {
        if byte_index >= target {
            break;
        }
        if ch == '\n' {
            line += 1;
            character = 0;
        } else {
            character += ch.len_utf16();
        }
    }
    Position { line, character }
}

fn project_diagnostics_to_set(
    config_path: &Path,
    diagnostics: Vec<ProjectDiagnostic>,
) -> DiagnosticSet {
    DiagnosticSet {
        diagnostics: diagnostics
            .into_iter()
            .map(|diagnostic| project_diagnostic(config_path, diagnostic))
            .collect(),
    }
}

fn project_diagnostic(config_path: &Path, diagnostic: ProjectDiagnostic) -> Diagnostic {
    Diagnostic {
        code: diagnostic.code.unwrap_or_else(|| "PROJECT-001".to_string()),
        stage: PROJECT_DIAGNOSTIC_STAGE.to_string(),
        severity: Severity::Error,
        message: diagnostic.message,
        primary: Some(Label {
            location: SourceLocation::ProjectConfig {
                path: config_path.to_path_buf(),
                key_path: diagnostic.key_path,
            },
            message: None,
        }),
        related: Vec::new(),
    }
}

fn join_diagnostic_messages(diagnostics: DiagnosticSet) -> String {
    diagnostics
        .diagnostics
        .into_iter()
        .map(|diagnostic| diagnostic.message)
        .collect::<Vec<_>>()
        .join("\n")
}

#[must_use]
pub fn path_to_slash(path: &Path) -> String {
    let raw = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().replace('\\', "/")),
            Component::Prefix(prefix) => Some(prefix.as_os_str().to_string_lossy().to_string()),
            Component::RootDir | Component::CurDir => None,
            Component::ParentDir => Some("..".to_string()),
        })
        .collect::<Vec<_>>()
        .join("/");
    // Strip the Windows verbatim-path prefix (\\?\  or //?/) so the result
    // is portable and can be round-tripped through YAML or the LSP protocol.
    raw.strip_prefix(r"\\?\")
        .or_else(|| raw.strip_prefix("//?/"))
        .map_or_else(|| raw.clone(), str::to_owned)
}

#[must_use]
pub fn normalize_path(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| {
        let mut out = PathBuf::new();
        for component in path.components() {
            match component {
                Component::CurDir => {}
                Component::ParentDir => {
                    out.pop();
                }
                other => out.push(other.as_os_str()),
            }
        }
        out
    })
}
