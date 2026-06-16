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

use coflow_cft::{CftContainer, CftDiagnostic, CftLabel, ModuleId, Span};
use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

const PROJECT_DIAGNOSTIC_STAGE: &str = "PROJECT";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {
    pub schema: SchemaConfig,
    #[serde(default)]
    pub sources: Vec<SourceConfig>,
    #[serde(default)]
    pub outputs: OutputsConfig,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum SchemaConfig {
    One(PathBuf),
    Many(Vec<PathBuf>),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceConfig {
    pub file: Option<PathBuf>,
    pub dir: Option<PathBuf>,
    #[serde(default)]
    pub sheets: Vec<SheetConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SheetConfig {
    pub sheet: String,
    #[serde(rename = "type")]
    pub type_name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_columns")]
    pub columns: BTreeMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputsConfig {
    pub data: Option<OutputConfig>,
    pub code: Option<OutputConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputConfig {
    #[serde(rename = "type")]
    pub output_type: String,
    pub dir: PathBuf,
    pub namespace: Option<String>,
}

fn deserialize_columns<'de, D>(deserializer: D) -> Result<BTreeMap<String, String>, D::Error>
where
    D: Deserializer<'de>,
{
    struct ColumnsVisitor;

    impl<'de> Visitor<'de> for ColumnsVisitor {
        type Value = BTreeMap<String, String>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a mapping of Excel column names to CFT field names")
        }

        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: MapAccess<'de>,
        {
            let mut columns = BTreeMap::new();
            while let Some((key, value)) = map.next_entry::<String, String>()? {
                if columns.insert(key.clone(), value).is_some() {
                    return Err(de::Error::custom(format!("duplicate columns key `{key}`")));
                }
            }
            Ok(columns)
        }
    }

    deserializer.deserialize_map(ColumnsVisitor)
}

#[derive(Debug)]
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
        let schema_diagnostics = project.schema_diagnostics();
        if !schema_diagnostics.is_empty() {
            return Err(schema_diagnostics
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
        validate_sources(&self.root_dir, &self.config.sources)
    }

    /// Validates output settings required by C# code generation.
    ///
    /// # Errors
    ///
    /// Returns an error when code or data output settings are missing or use
    /// unsupported output types.
    pub fn validate_for_codegen(&self) -> Result<(), String> {
        let code = self.config.outputs.code.as_ref().ok_or_else(|| {
            "coflow.yaml missing outputs.code; required `type: csharp` for `coflow codegen csharp`"
                .to_string()
        })?;
        if code.output_type != "csharp" {
            return Err(format!(
                "coflow.yaml outputs.code.type is `{}`; expected `csharp`",
                code.output_type
            ));
        }
        let data = self.config.outputs.data.as_ref().ok_or_else(|| {
            "coflow.yaml missing outputs.data; required `type: json` or `type: messagepack` for `coflow codegen csharp`"
                .to_string()
        })?;
        if !matches!(data.output_type.as_str(), "json" | "messagepack") {
            return Err(format!(
                "coflow.yaml outputs.data.type is `{}`; expected `json` or `messagepack`",
                data.output_type
            ));
        }
        Ok(())
    }

    #[must_use]
    pub fn schema_diagnostics(&self) -> Vec<DiagnosticJson> {
        diagnostics_from_messages(validate_project_config_schema_only_collecting(
            &self.root_dir,
            &self.config,
        ))
    }

    #[must_use]
    pub fn data_diagnostics(&self) -> Vec<DiagnosticJson> {
        diagnostics_from_messages(validate_sources_collecting(
            &self.root_dir,
            &self.config.sources,
        ))
    }

    #[must_use]
    pub fn codegen_diagnostics(&self) -> Vec<DiagnosticJson> {
        diagnostics_from_messages(validate_for_codegen_collecting(&self.config.outputs))
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
            files.push(SchemaFile::new(path, &self.root_dir)?);
            Ok(())
        } else {
            Err(format!("schema path `{}` does not exist", path.display()))
        }
    }
}

fn validate_project_config_schema_only_collecting(
    root_dir: &Path,
    config: &ProjectConfig,
) -> Vec<String> {
    let mut diagnostics = Vec::new();
    diagnostics.extend(validate_schema_config_collecting(root_dir, &config.schema));
    diagnostics.extend(validate_outputs_collecting(&config.outputs));
    diagnostics.extend(validate_source_shapes_collecting(&config.sources));
    diagnostics
}

fn validate_schema_config_collecting(root_dir: &Path, schema: &SchemaConfig) -> Vec<String> {
    let mut diagnostics = Vec::new();
    match schema {
        SchemaConfig::One(path) => {
            if let Err(err) = validate_schema_path(root_dir, path, "schema") {
                diagnostics.push(err);
            }
        }
        SchemaConfig::Many(paths) => {
            if paths.is_empty() {
                diagnostics.push("schema list is empty".to_string());
            }
            for (index, path) in paths.iter().enumerate() {
                if let Err(err) = validate_schema_path(root_dir, path, &format!("schema[{index}]"))
                {
                    diagnostics.push(err);
                }
            }
        }
    }
    diagnostics
}

fn validate_schema_path(root_dir: &Path, path: &Path, label: &str) -> Result<(), String> {
    if path.as_os_str().is_empty() {
        return Err(format!("{label} path is empty"));
    }
    let resolved = resolve_project_relative(root_dir, path);
    if !resolved.exists() {
        return Err(format!("{label} path `{}` does not exist", path.display()));
    }
    Ok(())
}

fn validate_sources(root_dir: &Path, sources: &[SourceConfig]) -> Result<(), String> {
    let diagnostics = validate_sources_collecting(root_dir, sources);
    if !diagnostics.is_empty() {
        return Err(diagnostics.join("\n"));
    }
    Ok(())
}

fn validate_sources_collecting(root_dir: &Path, sources: &[SourceConfig]) -> Vec<String> {
    let mut diagnostics = validate_source_shapes_collecting(sources);
    for (source_index, source) in sources.iter().enumerate() {
        let source_label = format!("sources[{source_index}]");
        match (&source.file, &source.dir) {
            (Some(file), None) => {
                let resolved = resolve_project_relative(root_dir, file);
                if !resolved.is_file() && !resolved.is_dir() {
                    diagnostics.push(format!(
                        "{source_label}.file `{}` does not exist",
                        file.display()
                    ));
                }
            }
            (None, Some(dir)) => {
                let resolved = resolve_project_relative(root_dir, dir);
                if !resolved.is_dir() {
                    diagnostics.push(format!(
                        "{source_label}.dir `{}` does not exist or is not a directory",
                        dir.display()
                    ));
                }
            }
            (Some(_), Some(_)) | (None, None) => {}
        }
    }
    diagnostics
}

fn validate_source_shapes_collecting(sources: &[SourceConfig]) -> Vec<String> {
    let mut diagnostics = Vec::new();
    for (source_index, source) in sources.iter().enumerate() {
        let source_label = format!("sources[{source_index}]");
        if source.file.is_some() == source.dir.is_some() {
            diagnostics.push(format!(
                "{source_label} must set exactly one of `file` or `dir`"
            ));
        }
        if source
            .file
            .as_ref()
            .is_some_and(|file| file.as_os_str().is_empty())
        {
            diagnostics.push(format!("{source_label}.file is empty"));
        }
        if source
            .dir
            .as_ref()
            .is_some_and(|dir| dir.as_os_str().is_empty())
        {
            diagnostics.push(format!("{source_label}.dir is empty"));
        }
        for (sheet_index, sheet) in source.sheets.iter().enumerate() {
            let sheet_label = format!("{source_label}.sheets[{sheet_index}]");
            if sheet.sheet.trim().is_empty() {
                diagnostics.push(format!("{sheet_label}.sheet is empty"));
            }
            if let Some(type_name) = &sheet.type_name {
                if type_name.trim().is_empty() {
                    diagnostics.push(format!("{sheet_label}.type is empty"));
                }
            }
        }
    }
    diagnostics
}

fn validate_outputs_collecting(outputs: &OutputsConfig) -> Vec<String> {
    let mut diagnostics = Vec::new();
    if let Some(data) = &outputs.data {
        if !matches!(data.output_type.as_str(), "json" | "messagepack") {
            diagnostics.push(format!(
                "outputs.data.type is `{}`; expected `json` or `messagepack`",
                data.output_type
            ));
        }
        if let Err(err) = validate_output_dir("outputs.data.dir", &data.dir) {
            diagnostics.push(err);
        }
        if data.namespace.is_some() {
            diagnostics.push("outputs.data.namespace is only valid for code outputs".to_string());
        }
    }
    if let Some(code) = &outputs.code {
        if code.output_type != "csharp" {
            diagnostics.push(format!(
                "outputs.code.type is `{}`; expected `csharp`",
                code.output_type
            ));
        }
        if let Err(err) = validate_output_dir("outputs.code.dir", &code.dir) {
            diagnostics.push(err);
        }
        if let Some(namespace) = &code.namespace {
            if namespace.trim().is_empty() {
                diagnostics.push("outputs.code.namespace is empty".to_string());
            }
        }
    }
    diagnostics
}

fn validate_for_codegen_collecting(outputs: &OutputsConfig) -> Vec<String> {
    let mut diagnostics = Vec::new();
    match outputs.code.as_ref() {
        Some(code) => {
            if code.output_type != "csharp" {
                diagnostics.push(format!(
                    "coflow.yaml outputs.code.type is `{}`; expected `csharp`",
                    code.output_type
                ));
            }
            if let Err(err) = validate_output_dir("outputs.code.dir", &code.dir) {
                diagnostics.push(err);
            }
            if let Some(namespace) = &code.namespace {
                if namespace.trim().is_empty() {
                    diagnostics.push("outputs.code.namespace is empty".to_string());
                }
            }
        }
        None => diagnostics.push(
            "coflow.yaml missing outputs.code; required `type: csharp` for `coflow codegen csharp`"
                .to_string(),
        ),
    }
    match outputs.data.as_ref() {
        Some(data) => {
            if !matches!(data.output_type.as_str(), "json" | "messagepack") {
                diagnostics.push(format!(
                    "coflow.yaml outputs.data.type is `{}`; expected `json` or `messagepack`",
                    data.output_type
                ));
            }
            if let Err(err) = validate_output_dir("outputs.data.dir", &data.dir) {
                diagnostics.push(err);
            }
        }
        None => diagnostics.push(
            "coflow.yaml missing outputs.data; required `type: json` or `type: messagepack` for `coflow codegen csharp`"
                .to_string(),
        ),
    }
    diagnostics
}

fn validate_output_dir(label: &str, path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty() {
        Err(format!("{label} is empty"))
    } else {
        Ok(())
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
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("cft") {
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

#[derive(Debug, Serialize)]
pub struct DiagnosticJson {
    pub code: String,
    pub stage: String,
    pub severity: String,
    pub message: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sheet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell: Option<String>,
    #[serde(rename = "startLine")]
    pub start_line: usize,
    #[serde(rename = "startCharacter")]
    pub start_character: usize,
    #[serde(rename = "endLine")]
    pub end_line: usize,
    #[serde(rename = "endCharacter")]
    pub end_character: usize,
    pub related: Vec<RelatedJson>,
}

impl DiagnosticJson {
    #[must_use]
    pub fn project(message: impl Into<String>) -> Self {
        Self::plain("PROJECT-001", PROJECT_DIAGNOSTIC_STAGE, message)
    }

    #[must_use]
    pub fn artifact(message: impl Into<String>) -> Self {
        Self::plain("ARTIFACT-001", "ARTIFACT", message)
    }

    #[must_use]
    pub fn codegen(
        code: impl Into<String>,
        stage: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::plain(code, stage, message)
    }

    fn plain(
        code: impl Into<String>,
        stage: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            stage: stage.into(),
            severity: "error".to_string(),
            message: message.into(),
            path: String::new(),
            sheet: None,
            cell: None,
            start_line: 0,
            start_character: 0,
            end_line: 0,
            end_character: 1,
            related: Vec::new(),
        }
    }

    pub fn from_cft(
        diagnostic: &CftDiagnostic,
        sources: &BTreeMap<String, String>,
        paths: &BTreeMap<String, String>,
    ) -> Self {
        let fallback = CftLabel {
            module: ModuleId::new(""),
            span: Span::default(),
            message: None,
        };
        let primary = diagnostic.primary.as_ref().unwrap_or(&fallback);
        let range = cft_label_range(primary, sources);
        let path = paths
            .get(primary.module.as_str())
            .map_or_else(|| primary.module.as_str().to_string(), Clone::clone);
        Self {
            code: diagnostic.code.as_str().to_string(),
            stage: diagnostic.stage.to_string(),
            severity: "error".to_string(),
            message: diagnostic.message.clone(),
            path,
            sheet: None,
            cell: None,
            start_line: range.start.line,
            start_character: range.start.character,
            end_line: range.end.line,
            end_character: range.end.character,
            related: diagnostic
                .related
                .iter()
                .map(|label| RelatedJson::from_cft(label, sources, paths))
                .collect(),
        }
    }
}

fn diagnostics_from_messages(messages: Vec<String>) -> Vec<DiagnosticJson> {
    messages.into_iter().map(DiagnosticJson::project).collect()
}

#[derive(Debug, Serialize)]
pub struct RelatedJson {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sheet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell: Option<String>,
    #[serde(rename = "startLine")]
    pub start_line: usize,
    #[serde(rename = "startCharacter")]
    pub start_character: usize,
    #[serde(rename = "endLine")]
    pub end_line: usize,
    #[serde(rename = "endCharacter")]
    pub end_character: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl RelatedJson {
    fn from_cft(
        label: &CftLabel,
        sources: &BTreeMap<String, String>,
        paths: &BTreeMap<String, String>,
    ) -> Self {
        let range = cft_label_range(label, sources);
        let path = paths
            .get(label.module.as_str())
            .map_or_else(|| label.module.as_str().to_string(), Clone::clone);
        Self {
            path,
            sheet: None,
            cell: None,
            start_line: range.start.line,
            start_character: range.start.character,
            end_line: range.end.line,
            end_character: range.end.character,
            label: label.message.clone(),
        }
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

#[must_use]
pub fn path_to_slash(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().replace('\\', "/")),
            Component::Prefix(prefix) => Some(prefix.as_os_str().to_string_lossy().to_string()),
            Component::RootDir | Component::CurDir => None,
            Component::ParentDir => Some("..".to_string()),
        })
        .collect::<Vec<_>>()
        .join("/")
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
