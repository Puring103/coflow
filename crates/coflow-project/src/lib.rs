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

pub use coflow_api::SourceLocationSpec;
use coflow_api::{Diagnostic, DiagnosticSet, Label, Severity, SourceLocation};
use coflow_cft::{CftContainer, CftDiagnostic, CftLabel, ModuleId, Span};
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

const PROJECT_DIAGNOSTIC_STAGE: &str = "PROJECT";

#[derive(Debug)]
struct NoDuplicateValue(Value);

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

#[derive(Debug)]
pub struct SourceConfig {
    pub source_type: Option<String>,
    pub location: SourceLocationSpec,
    pub options: Value,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputsConfig {
    pub data: Option<OutputConfig>,
    pub code: Option<OutputConfig>,
}

#[derive(Debug)]
pub struct OutputConfig {
    pub output_type: String,
    pub dir: PathBuf,
    pub options: Value,
}

impl SourceConfig {
    #[must_use]
    pub const fn location(&self) -> &SourceLocationSpec {
        &self.location
    }

    #[must_use]
    pub const fn options(&self) -> &Value {
        &self.options
    }
}

impl OutputConfig {
    #[must_use]
    pub const fn options(&self) -> &Value {
        &self.options
    }
}

impl<'de> Deserialize<'de> for SourceConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut fields = no_duplicate_object(deserializer)?;
        reject_removed_source_fields(&fields).map_err(de::Error::custom)?;
        let source_type = fields
            .remove("type")
            .map(string_field("source `type`"))
            .transpose()
            .map_err(de::Error::custom)?;
        let path = fields.remove("path");
        let url = fields.remove("url");
        let location = match (path, url) {
            (Some(path), None) => {
                SourceLocationSpec::Path(path_value(path).map_err(de::Error::custom)?)
            }
            (None, Some(url)) => {
                SourceLocationSpec::Uri(url_value(url).map_err(de::Error::custom)?)
            }
            (Some(_), Some(_)) | (None, None) => {
                return Err(de::Error::custom(
                    "source must set exactly one of `path` or `url`",
                ))
            }
        };
        expand_env_in_object(&mut fields).map_err(de::Error::custom)?;
        Ok(Self {
            source_type,
            location,
            options: Value::Object(fields),
        })
    }
}

impl<'de> Deserialize<'de> for OutputConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut fields = no_duplicate_object(deserializer)?;
        let output_type = fields
            .remove("type")
            .map(string_field("output `type`"))
            .transpose()
            .map_err(de::Error::custom)?
            .ok_or_else(|| de::Error::custom("output must set `type`"))?;
        let dir = fields
            .remove("dir")
            .map(path_value)
            .transpose()
            .map_err(de::Error::custom)?
            .ok_or_else(|| de::Error::custom("output must set `dir`"))?;
        expand_env_in_object(&mut fields).map_err(de::Error::custom)?;
        Ok(Self {
            output_type,
            dir,
            options: Value::Object(fields),
        })
    }
}

fn no_duplicate_object<'de, D>(deserializer: D) -> Result<Map<String, Value>, D::Error>
where
    D: Deserializer<'de>,
{
    let NoDuplicateValue(Value::Object(fields)) = NoDuplicateValue::deserialize(deserializer)?
    else {
        return Err(de::Error::custom("expected an object"));
    };
    Ok(fields)
}

impl<'de> Deserialize<'de> for NoDuplicateValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(NoDuplicateValueVisitor)
    }
}

struct NoDuplicateValueVisitor;

impl<'de> Visitor<'de> for NoDuplicateValueVisitor {
    type Value = NoDuplicateValue;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a YAML value without duplicate mapping keys")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(NoDuplicateValue(Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(NoDuplicateValue(Value::Number(value.into())))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(NoDuplicateValue(Value::Number(value.into())))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        let number = serde_json::Number::from_f64(value)
            .ok_or_else(|| E::custom("non-finite numbers are not supported"))?;
        Ok(NoDuplicateValue(Value::Number(number)))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(NoDuplicateValue(Value::String(value.to_string())))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(NoDuplicateValue(Value::String(value)))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(NoDuplicateValue(Value::Null))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(NoDuplicateValue(Value::Null))
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        NoDuplicateValue::deserialize(deserializer)
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(NoDuplicateValue(value)) = seq.next_element()? {
            values.push(value);
        }
        Ok(NoDuplicateValue(Value::Array(values)))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut object = Map::new();
        while let Some(key) = map.next_key::<String>()? {
            if object.contains_key(&key) {
                return Err(de::Error::custom(format!("duplicate key `{key}`")));
            }
            let NoDuplicateValue(value) = map.next_value()?;
            object.insert(key, value);
        }
        Ok(NoDuplicateValue(Value::Object(object)))
    }
}

fn string_field(label: &'static str) -> impl FnOnce(Value) -> Result<String, String> {
    move |value| {
        let Value::String(value) = value else {
            return Err(format!("{label} must be a string"));
        };
        Ok(value)
    }
}

fn reject_removed_source_fields(fields: &Map<String, Value>) -> Result<(), String> {
    for key in ["file", "dir", "lark_sheet"] {
        if fields.contains_key(key) {
            return Err(format!("unknown field `{key}`"));
        }
    }
    Ok(())
}

fn path_value(value: Value) -> Result<PathBuf, String> {
    let Value::String(value) = value else {
        return Err("source `path` must be a string".to_string());
    };
    Ok(PathBuf::from(value))
}

fn url_value(value: Value) -> Result<String, String> {
    let Value::String(value) = value else {
        return Err("source `url` must be a string".to_string());
    };
    Ok(value)
}

fn expand_env_in_object(fields: &mut Map<String, Value>) -> Result<(), String> {
    for value in fields.values_mut() {
        expand_env_in_value(value)?;
    }
    Ok(())
}

fn expand_env_in_value(value: &mut Value) -> Result<(), String> {
    match value {
        Value::String(text) => {
            if let Some(env_name) = env_reference_name(text) {
                *text = std::env::var(env_name)
                    .map_err(|_| format!("environment variable `{env_name}` is not set"))?;
            }
        }
        Value::Array(items) => {
            for item in items {
                expand_env_in_value(item)?;
            }
        }
        Value::Object(object) => expand_env_in_object(object)?,
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
    Ok(())
}

fn env_reference_name(value: &str) -> Option<&str> {
    value.strip_prefix("${")?.strip_suffix('}')
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
    pub fn schema_diagnostics(&self) -> Vec<DiagnosticJson> {
        diagnostic_json_from_set(self.schema_diagnostic_set())
    }

    #[must_use]
    pub fn data_diagnostics(&self) -> Vec<DiagnosticJson> {
        diagnostic_json_from_set(self.data_diagnostic_set())
    }

    #[must_use]
    pub fn codegen_diagnostics(&self) -> Vec<DiagnosticJson> {
        diagnostic_json_from_set(self.codegen_diagnostic_set())
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProjectDiagnostic {
    message: String,
    key_path: Vec<String>,
}

impl ProjectDiagnostic {
    fn new(
        message: impl Into<String>,
        key_path: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            message: message.into(),
            key_path: key_path.into_iter().map(Into::into).collect(),
        }
    }
}

fn validate_project_config_schema_only_collecting(
    root_dir: &Path,
    config: &ProjectConfig,
) -> Vec<ProjectDiagnostic> {
    let mut diagnostics = Vec::new();
    diagnostics.extend(validate_schema_config_collecting(root_dir, &config.schema));
    diagnostics.extend(validate_outputs_collecting(&config.outputs));
    diagnostics.extend(validate_source_shapes_collecting(&config.sources));
    diagnostics
}

fn validate_schema_config_collecting(
    root_dir: &Path,
    schema: &SchemaConfig,
) -> Vec<ProjectDiagnostic> {
    let mut diagnostics = Vec::new();
    match schema {
        SchemaConfig::One(path) => {
            if let Err(err) = validate_schema_path(root_dir, path, "schema") {
                diagnostics.push(ProjectDiagnostic::new(err, ["schema"]));
            }
        }
        SchemaConfig::Many(paths) => {
            if paths.is_empty() {
                diagnostics.push(ProjectDiagnostic::new("schema list is empty", ["schema"]));
            }
            for (index, path) in paths.iter().enumerate() {
                if let Err(err) = validate_schema_path(root_dir, path, &format!("schema[{index}]"))
                {
                    diagnostics.push(ProjectDiagnostic::new(
                        err,
                        ["schema".to_string(), index.to_string()],
                    ));
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
    if resolved.is_file() && !is_cft_path(&resolved) {
        return Err(format!(
            "schema file `{}` has unsupported extension",
            path_to_slash(path)
        ));
    }
    Ok(())
}

fn validate_sources_collecting(
    root_dir: &Path,
    sources: &[SourceConfig],
) -> Vec<ProjectDiagnostic> {
    let mut diagnostics = validate_source_shapes_collecting(sources);
    for (source_index, source) in sources.iter().enumerate() {
        let source_label = format!("sources[{source_index}]");
        let source_index_key = source_index.to_string();
        match &source.location {
            SourceLocationSpec::Path(path) => {
                let resolved = resolve_project_relative(root_dir, path);
                if !resolved.is_file() && !resolved.is_dir() {
                    diagnostics.push(ProjectDiagnostic::new(
                        format!("{source_label}.path `{}` does not exist", path.display()),
                        [
                            "sources".to_string(),
                            source_index_key.clone(),
                            "path".to_string(),
                        ],
                    ));
                }
            }
            SourceLocationSpec::Uri(_) => {}
        }
    }
    diagnostics
}

fn validate_source_shapes_collecting(sources: &[SourceConfig]) -> Vec<ProjectDiagnostic> {
    let mut diagnostics = Vec::new();
    for (source_index, source) in sources.iter().enumerate() {
        let source_label = format!("sources[{source_index}]");
        let source_index_key = source_index.to_string();
        if source
            .source_type
            .as_ref()
            .is_some_and(|source_type| source_type.trim().is_empty())
        {
            diagnostics.push(ProjectDiagnostic::new(
                format!("{source_label}.type is empty"),
                [
                    "sources".to_string(),
                    source_index_key.clone(),
                    "type".to_string(),
                ],
            ));
        }
        match &source.location {
            SourceLocationSpec::Path(path) if path.as_os_str().is_empty() => {
                diagnostics.push(ProjectDiagnostic::new(
                    format!("{source_label}.path is empty"),
                    [
                        "sources".to_string(),
                        source_index_key.clone(),
                        "path".to_string(),
                    ],
                ));
            }
            SourceLocationSpec::Uri(uri) if uri.trim().is_empty() => {
                diagnostics.push(ProjectDiagnostic::new(
                    format!("{source_label}.url is empty"),
                    [
                        "sources".to_string(),
                        source_index_key.clone(),
                        "url".to_string(),
                    ],
                ));
            }
            SourceLocationSpec::Path(_) | SourceLocationSpec::Uri(_) => {}
        }
    }
    diagnostics
}

fn validate_outputs_collecting(outputs: &OutputsConfig) -> Vec<ProjectDiagnostic> {
    let mut diagnostics = Vec::new();
    if let Some(data) = &outputs.data {
        if data.output_type.trim().is_empty() {
            diagnostics.push(ProjectDiagnostic::new(
                "outputs.data.type is empty",
                ["outputs", "data", "type"],
            ));
        }
        if let Err(err) = validate_output_dir("outputs.data.dir", &data.dir) {
            diagnostics.push(ProjectDiagnostic::new(err, ["outputs", "data", "dir"]));
        }
    }
    if let Some(code) = &outputs.code {
        if code.output_type.trim().is_empty() {
            diagnostics.push(ProjectDiagnostic::new(
                "outputs.code.type is empty",
                ["outputs", "code", "type"],
            ));
        }
        if let Err(err) = validate_output_dir("outputs.code.dir", &code.dir) {
            diagnostics.push(ProjectDiagnostic::new(err, ["outputs", "code", "dir"]));
        }
    }
    diagnostics
}

fn validate_for_codegen_collecting(outputs: &OutputsConfig) -> Vec<ProjectDiagnostic> {
    let mut diagnostics = Vec::new();
    match outputs.code.as_ref() {
        Some(code) => {
            if code.output_type.trim().is_empty() {
                diagnostics.push(ProjectDiagnostic::new(
                    "coflow.yaml outputs.code.type is empty",
                    ["outputs", "code", "type"],
                ));
            }
            if let Err(err) = validate_output_dir("outputs.code.dir", &code.dir) {
                diagnostics.push(ProjectDiagnostic::new(err, ["outputs", "code", "dir"]));
            }
        }
        None => diagnostics.push(ProjectDiagnostic::new(
            "coflow.yaml missing outputs.code",
            ["outputs", "code"],
        )),
    }
    match outputs.data.as_ref() {
        Some(data) => {
            if data.output_type.trim().is_empty() {
                diagnostics.push(ProjectDiagnostic::new(
                    "coflow.yaml outputs.data.type is empty",
                    ["outputs", "data", "type"],
                ));
            }
            if let Err(err) = validate_output_dir("outputs.data.dir", &data.dir) {
                diagnostics.push(ProjectDiagnostic::new(err, ["outputs", "data", "dir"]));
            }
        }
        None => diagnostics.push(ProjectDiagnostic::new(
            "coflow.yaml missing outputs.data",
            ["outputs", "data"],
        )),
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

#[must_use]
pub fn diagnostic_json_from_set(diagnostics: DiagnosticSet) -> Vec<DiagnosticJson> {
    diagnostics
        .diagnostics
        .into_iter()
        .map(diagnostic_json_from_diagnostic)
        .collect()
}

fn diagnostic_json_from_diagnostic(diagnostic: Diagnostic) -> DiagnosticJson {
    let primary = diagnostic.primary.as_ref().map(label_location);
    DiagnosticJson {
        code: diagnostic.code,
        stage: diagnostic.stage,
        severity: severity_name(diagnostic.severity).to_string(),
        message: diagnostic.message,
        path: primary
            .as_ref()
            .map_or_else(String::new, |location| location.path.clone()),
        sheet: primary.as_ref().and_then(|location| location.sheet.clone()),
        cell: primary.as_ref().and_then(|location| location.cell.clone()),
        start_line: primary.as_ref().map_or(0, |location| location.start_line),
        start_character: primary
            .as_ref()
            .map_or(0, |location| location.start_character),
        end_line: primary.as_ref().map_or(0, |location| location.end_line),
        end_character: primary
            .as_ref()
            .map_or(1, |location| location.end_character),
        related: diagnostic
            .related
            .iter()
            .map(related_json_from_label)
            .collect(),
    }
}

fn related_json_from_label(label: &Label) -> RelatedJson {
    let location = label_location(label);
    RelatedJson {
        path: location.path,
        sheet: location.sheet,
        cell: location.cell,
        start_line: location.start_line,
        start_character: location.start_character,
        end_line: location.end_line,
        end_character: location.end_character,
        label: label.message.clone(),
    }
}

#[derive(Debug)]
struct JsonLocation {
    path: String,
    sheet: Option<String>,
    cell: Option<String>,
    start_line: usize,
    start_character: usize,
    end_line: usize,
    end_character: usize,
}

fn label_location(label: &Label) -> JsonLocation {
    match &label.location {
        SourceLocation::FileSpan {
            path,
            start_line,
            start_character,
            end_line,
            end_character,
        } => JsonLocation {
            path: path.display().to_string(),
            sheet: None,
            cell: None,
            start_line: *start_line,
            start_character: *start_character,
            end_line: *end_line,
            end_character: *end_character,
        },
        SourceLocation::TableCell {
            path,
            sheet,
            row,
            column,
        } => JsonLocation {
            path: path.display().to_string(),
            sheet: sheet.clone(),
            cell: Some(excel_cell(*row, *column)),
            start_line: row.saturating_sub(1),
            start_character: column.saturating_sub(1),
            end_line: row.saturating_sub(1),
            end_character: *column,
        },
        SourceLocation::RemoteCell {
            document,
            sheet,
            row,
            column,
        } => JsonLocation {
            path: document.clone(),
            sheet: sheet.clone(),
            cell: (*row > 0 && *column > 0).then(|| excel_cell(*row, *column)),
            start_line: row.saturating_sub(1),
            start_character: column.saturating_sub(1),
            end_line: row.saturating_sub(1),
            end_character: (*column).max(1),
        },
        SourceLocation::ProjectConfig { path, .. } | SourceLocation::Artifact { path } => {
            JsonLocation {
                path: path.display().to_string(),
                sheet: None,
                cell: None,
                start_line: 0,
                start_character: 0,
                end_line: 0,
                end_character: 1,
            }
        }
    }
}

const fn severity_name(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "info",
    }
}

fn excel_cell(row: usize, column: usize) -> String {
    format!("{}{}", excel_column_name(column), row)
}

fn excel_column_name(column: usize) -> String {
    let mut value = column;
    let mut name = Vec::new();
    while value > 0 {
        value -= 1;
        #[allow(clippy::cast_possible_truncation)]
        let offset = (value % 26) as u8;
        name.push((b'A' + offset) as char);
        value /= 26;
    }
    name.iter().rev().collect()
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
        code: "PROJECT-001".to_string(),
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
