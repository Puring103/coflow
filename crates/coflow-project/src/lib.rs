use coflow_cft::{CftContainer, CftDiagnostic, CftLabel, ModuleId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Deserialize)]
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
pub struct SourceConfig {
    pub file: PathBuf,
    #[serde(default)]
    pub sheets: Vec<SheetConfig>,
}

#[derive(Debug, Deserialize)]
pub struct SheetConfig {
    pub sheet: String,
    #[serde(rename = "type")]
    pub type_name: Option<String>,
    #[serde(default)]
    pub columns: BTreeMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct OutputsConfig {
    pub data: Option<OutputConfig>,
    pub code: Option<OutputConfig>,
}

#[derive(Debug, Deserialize)]
pub struct OutputConfig {
    #[serde(rename = "type")]
    pub output_type: String,
    pub dir: PathBuf,
    pub namespace: Option<String>,
}

#[derive(Debug)]
pub struct Project {
    pub config_path: PathBuf,
    pub root_dir: PathBuf,
    pub config: ProjectConfig,
}

impl Project {
    pub fn open(config_or_dir: Option<&Path>) -> Result<Self, String> {
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

    pub fn resolve_path(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root_dir.join(path)
        }
    }

    pub fn schema_files(&self) -> Result<Vec<SchemaFile>, String> {
        let mut files = Vec::new();
        match &self.config.schema {
            SchemaConfig::One(path) => self.push_schema_path(path, &mut files)?,
            SchemaConfig::Many(paths) => {
                for path in paths {
                    self.push_schema_path(path, &mut files)?;
                }
            }
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
    let yaml = dir.join("coflow.yaml");
    let yml = dir.join("coflow.yml");
    match (yaml.exists(), yml.exists()) {
        (true, false) => Ok(yaml),
        (false, true) => Ok(yml),
        (true, true) => Err(format!(
            "both `{}` and `{}` exist; specify the config file explicitly",
            yaml.display(),
            yml.display()
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
    entries.sort_by_key(|entry| entry.path());
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
    pub fn from_cft(
        diagnostic: &CftDiagnostic,
        sources: &BTreeMap<String, String>,
        paths: &BTreeMap<String, String>,
    ) -> Self {
        let fallback = CftLabel {
            module: ModuleId::new(""),
            span: Default::default(),
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

#[derive(Debug, Serialize)]
pub struct RelatedJson {
    pub path: String,
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

pub fn path_to_slash(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().replace('\\', "/")),
            Component::Prefix(prefix) => Some(prefix.as_os_str().to_string_lossy().to_string()),
            Component::RootDir => None,
            Component::CurDir => None,
            Component::ParentDir => Some("..".to_string()),
        })
        .collect::<Vec<_>>()
        .join("/")
}

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
