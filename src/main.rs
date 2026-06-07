mod lsp;

use clap::{Args, Parser, Subcommand};
use coflow_cft::{CftContainer, CftDiagnostic, CftLabel, ModuleId};
use coflow_codegen_csharp::{generate_csharp_json, CsharpCodegenOptions};
use coflow_excel_loader::{
    load_excel, ExcelDiagnostic, ExcelDiagnostics, ExcelLoadError, ExcelLocation, ExcelSheet,
    ExcelSource,
};
use coflow_json_export::export_json_model;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(message) => {
            let _ = writeln!(io::stderr().lock(), "{message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<bool, String> {
    match Cli::parse().command {
        Command::Init(args) => init_project(args),
        Command::Cft(command) => match command.command {
            CftCommand::Check(args) => cft_check(args),
            CftCommand::Lsp(args) => cft_lsp(args),
        },
        Command::Check(args) => project_check(args),
        Command::Export(command) => match command.command {
            ExportCommand::Json(args) => export_json(args),
        },
        Command::Codegen(command) => match command.command {
            CodegenCommand::Csharp(args) => codegen_csharp(args),
        },
    }
}

#[derive(Debug, Parser)]
#[command(name = "coflow")]
#[command(about = "Project-level tools for Coflow schemas and data.")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create a minimal Coflow project.
    Init(InitArgs),
    /// CFT schema tools.
    Cft(CftArgs),
    /// Run the full project validation pipeline.
    Check(ProjectCheckArgs),
    /// Export project data.
    Export(ExportArgs),
    /// Generate runtime code.
    Codegen(CodegenArgs),
}

#[derive(Debug, Args)]
struct InitArgs {
    #[arg(value_name = "DIR")]
    dir: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct CftArgs {
    #[command(subcommand)]
    command: CftCommand,
}

#[derive(Debug, Subcommand)]
enum CftCommand {
    /// Compile all CFT schema files from coflow.yaml.
    Check(CftCheckArgs),
    /// Start the CFT language server.
    Lsp(CftLspArgs),
}

#[derive(Debug, Args)]
struct CftCheckArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    config_or_dir: Option<PathBuf>,
    /// Emit machine-readable diagnostics JSON.
    #[arg(long)]
    json: bool,
    /// Treat stdin as this schema file's source.
    #[arg(long = "stdin-path", value_name = "PATH")]
    stdin_path: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct CftLspArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    config_or_dir: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct ProjectCheckArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    config_or_dir: Option<PathBuf>,
    /// Emit machine-readable diagnostics JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ExportArgs {
    #[command(subcommand)]
    command: ExportCommand,
}

#[derive(Debug, Subcommand)]
enum ExportCommand {
    /// Export data as JSON. The project config must declare outputs.data.type: json.
    Json(ExportJsonArgs),
}

#[derive(Debug, Args)]
struct ExportJsonArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    config_or_dir: Option<PathBuf>,
    /// Override outputs.data.dir for this invocation.
    #[arg(long = "out", value_name = "DIR")]
    out_dir: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct CodegenArgs {
    #[command(subcommand)]
    command: CodegenCommand,
}

#[derive(Debug, Subcommand)]
enum CodegenCommand {
    /// Generate C# runtime code. The project config must declare outputs.code.type: csharp.
    Csharp(CodegenCsharpArgs),
}

#[derive(Debug, Args)]
struct CodegenCsharpArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    config_or_dir: Option<PathBuf>,
    /// Override outputs.code.dir for this invocation.
    #[arg(long = "out", value_name = "DIR")]
    out_dir: Option<PathBuf>,
    /// Override outputs.code.namespace for this invocation.
    #[arg(long, value_name = "NAME")]
    namespace: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProjectConfig {
    schema: SchemaConfig,
    #[serde(default)]
    sources: Vec<SourceConfig>,
    #[serde(default)]
    outputs: OutputsConfig,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum SchemaConfig {
    One(PathBuf),
    Many(Vec<PathBuf>),
}

#[derive(Debug, Deserialize)]
struct SourceConfig {
    file: PathBuf,
    #[serde(default)]
    sheets: Vec<SheetConfig>,
}

#[derive(Debug, Deserialize)]
struct SheetConfig {
    sheet: String,
    #[serde(rename = "type")]
    type_name: Option<String>,
    #[serde(default)]
    columns: BTreeMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
struct OutputsConfig {
    data: Option<OutputConfig>,
    code: Option<OutputConfig>,
}

#[derive(Debug, Deserialize)]
struct OutputConfig {
    #[serde(rename = "type")]
    output_type: String,
    dir: PathBuf,
    namespace: Option<String>,
}

#[derive(Debug)]
struct Project {
    config_path: PathBuf,
    root_dir: PathBuf,
    config: ProjectConfig,
}

impl Project {
    fn open(config_or_dir: Option<&Path>) -> Result<Self, String> {
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

    fn resolve_path(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root_dir.join(path)
        }
    }

    fn schema_files(&self) -> Result<Vec<SchemaFile>, String> {
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

    fn excel_sources(&self) -> Vec<ExcelSource> {
        self.config
            .sources
            .iter()
            .map(|source| {
                let sheets = source
                    .sheets
                    .iter()
                    .map(|sheet| {
                        let mut out = ExcelSheet::new(sheet.sheet.clone());
                        if let Some(type_name) = &sheet.type_name {
                            out = out.with_type(type_name.clone());
                        }
                        if !sheet.columns.is_empty() {
                            out = out.with_columns(sheet.columns.clone());
                        }
                        out
                    })
                    .collect();
                ExcelSource::new(self.resolve_path(&source.file), sheets)
            })
            .collect()
    }
}

#[derive(Debug)]
struct SchemaFile {
    path: PathBuf,
    canonical_path: PathBuf,
    module_id: String,
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

struct SchemaBuild {
    container: Option<CftContainer>,
    diagnostics: Vec<CftDiagnostic>,
    sources: BTreeMap<String, String>,
    paths: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
struct SchemaSourceOverride {
    requested_module: Option<String>,
    normalized_path: PathBuf,
    source: String,
}

fn init_project(args: InitArgs) -> Result<bool, String> {
    let dir = args.dir.unwrap_or_else(|| PathBuf::from("."));
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
    let config_path = dir.join("coflow.yaml");
    if config_path.exists() {
        return Err(format!("`{}` already exists", config_path.display()));
    }
    let config = r#"schema: schema/

sources: []

outputs:
  data:
    type: json
    dir: generated/data
  code:
    type: csharp
    dir: generated/csharp
    namespace: Game.Config
"#;
    fs::write(&config_path, config)
        .map_err(|err| format!("failed to write `{}`: {err}", config_path.display()))?;
    println!("created {}", config_path.display());
    Ok(true)
}

fn cft_check(args: CftCheckArgs) -> Result<bool, String> {
    let project = Project::open(args.config_or_dir.as_deref())?;
    let build = compile_schema_project(&project, args.stdin_path.as_deref())?;
    let diagnostics = dedupe_cft_diagnostics(build.diagnostics);
    if args.json {
        write_json_diagnostics(
            diagnostics
                .iter()
                .map(|diagnostic| {
                    DiagnosticJson::from_cft(diagnostic, &build.sources, &build.paths)
                })
                .collect(),
        )?;
    } else if diagnostics.is_empty() {
        println!(
            "CFT check passed: {}",
            project
                .config_path
                .strip_prefix(&project.root_dir)
                .map_or_else(
                    |_| project.config_path.display().to_string(),
                    |path| path.display().to_string()
                )
        );
    } else {
        write_human_cft_diagnostics(&diagnostics, &build.sources, &build.paths)?;
    }
    Ok(diagnostics.is_empty())
}

fn cft_lsp(args: CftLspArgs) -> Result<bool, String> {
    let project = Project::open(args.config_or_dir.as_deref())?;
    lsp::run(project)
}

fn project_check(args: ProjectCheckArgs) -> Result<bool, String> {
    let project = Project::open(args.config_or_dir.as_deref())?;
    let build = compile_schema_project(&project, None)?;
    let cft_diagnostics = dedupe_cft_diagnostics(build.diagnostics);
    if !cft_diagnostics.is_empty() {
        if args.json {
            write_json_diagnostics(
                cft_diagnostics
                    .iter()
                    .map(|diagnostic| {
                        DiagnosticJson::from_cft(diagnostic, &build.sources, &build.paths)
                    })
                    .collect(),
            )?;
        } else {
            write_human_cft_diagnostics(&cft_diagnostics, &build.sources, &build.paths)?;
        }
        return Ok(false);
    }

    let Some(schema) = build.container else {
        return Err("schema compilation did not produce a container".to_string());
    };
    let sources = project.excel_sources();
    match load_excel(&schema, &sources) {
        Ok(output) => {
            if let Some(checks) = output.check_diagnostics {
                if args.json {
                    write_json_diagnostics(diagnostics_from_excel_checks(&checks))?;
                } else {
                    write_human_excel_diagnostics(&checks)?;
                }
                Ok(false)
            } else {
                if args.json {
                    write_json_diagnostics(Vec::new())?;
                } else {
                    println!("Project check passed: {}", project.config_path.display());
                }
                Ok(true)
            }
        }
        Err(err) => {
            if args.json {
                write_json_diagnostics(diagnostics_from_excel_error(&err))?;
            } else {
                write_human_excel_error(&err)?;
            }
            Ok(false)
        }
    }
}

fn export_json(args: ExportJsonArgs) -> Result<bool, String> {
    let project = Project::open(args.config_or_dir.as_deref())?;
    let output = project.config.outputs.data.as_ref().ok_or_else(|| {
        "coflow.yaml missing outputs.data; required `type: json` and `dir` for `coflow export json`"
            .to_string()
    })?;
    if output.output_type != "json" {
        return Err(format!(
            "coflow.yaml outputs.data.type is `{}`; required `json` for `coflow export json`",
            output.output_type
        ));
    }
    let dir = args.out_dir.as_deref().map_or_else(
        || project.resolve_path(&output.dir),
        |path| project.resolve_path(path),
    );
    let build = compile_schema_project(&project, None)?;
    let cft_diagnostics = dedupe_cft_diagnostics(build.diagnostics);
    if !cft_diagnostics.is_empty() {
        write_human_cft_diagnostics(&cft_diagnostics, &build.sources, &build.paths)?;
        return Ok(false);
    }
    let Some(schema) = build.container else {
        return Err("schema compilation did not produce a container".to_string());
    };
    let sources = project.excel_sources();
    let load_output = match load_excel(&schema, &sources) {
        Ok(output) => output,
        Err(err) => {
            write_human_excel_error(&err)?;
            return Ok(false);
        }
    };
    if let Some(checks) = load_output.check_diagnostics {
        write_human_excel_diagnostics(&checks)?;
        return Ok(false);
    }

    let tables = export_json_model(&schema, &load_output.model)
        .map_err(|err| format!("failed to export JSON model: {err}"))?;
    fs::create_dir_all(&dir)
        .map_err(|err| format!("failed to create output dir `{}`: {err}", dir.display()))?;
    for (table, value) in tables {
        let path = dir.join(format!("{table}.json"));
        let file = fs::File::create(&path)
            .map_err(|err| format!("failed to create `{}`: {err}", path.display()))?;
        serde_json::to_writer_pretty(file, &value)
            .map_err(|err| format!("failed to write `{}`: {err}", path.display()))?;
    }
    println!("JSON data exported to {}", dir.display());
    Ok(true)
}

fn codegen_csharp(args: CodegenCsharpArgs) -> Result<bool, String> {
    let project = Project::open(args.config_or_dir.as_deref())?;
    let output = project
        .config
        .outputs
        .code
        .as_ref()
        .ok_or_else(|| {
            "coflow.yaml missing outputs.code; required `type: csharp` and `dir` for `coflow codegen csharp`"
                .to_string()
        })?;
    if output.output_type != "csharp" {
        return Err(format!(
            "coflow.yaml outputs.code.type is `{}`; required `csharp` for `coflow codegen csharp`",
            output.output_type
        ));
    }
    let dir = args.out_dir.as_deref().map_or_else(
        || project.resolve_path(&output.dir),
        |path| project.resolve_path(path),
    );
    let namespace = args
        .namespace
        .as_deref()
        .or(output.namespace.as_deref())
        .unwrap_or("Game.Config");
    let build = compile_schema_project(&project, None)?;
    let cft_diagnostics = dedupe_cft_diagnostics(build.diagnostics);
    if !cft_diagnostics.is_empty() {
        write_human_cft_diagnostics(&cft_diagnostics, &build.sources, &build.paths)?;
        return Ok(false);
    }
    let Some(schema) = build.container else {
        return Err("schema compilation did not produce a container".to_string());
    };

    let options = CsharpCodegenOptions::new(namespace);
    let files = generate_csharp_json(&schema, &options)
        .map_err(|err| format!("failed to generate C# code: {err}"))?;
    fs::create_dir_all(&dir)
        .map_err(|err| format!("failed to create output dir `{}`: {err}", dir.display()))?;
    for file in files {
        let path = dir.join(&file.relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("failed to create `{}`: {err}", parent.display()))?;
        }
        fs::write(&path, file.contents)
            .map_err(|err| format!("failed to write `{}`: {err}", path.display()))?;
    }
    println!("C# code generated to {}", dir.display());
    Ok(true)
}

fn compile_schema_project(
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

fn compile_schema_project_with_overrides(
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

fn resolve_config_path(config_or_dir: Option<&Path>) -> Result<PathBuf, String> {
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

fn dedupe_cft_diagnostics(diagnostics: Vec<CftDiagnostic>) -> Vec<CftDiagnostic> {
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

#[derive(Debug, Default, Serialize)]
struct DiagnosticsOutput {
    diagnostics: Vec<DiagnosticJson>,
}

#[derive(Debug, Serialize)]
struct DiagnosticJson {
    code: String,
    stage: String,
    severity: String,
    message: String,
    path: String,
    #[serde(rename = "startLine")]
    start_line: usize,
    #[serde(rename = "startCharacter")]
    start_character: usize,
    #[serde(rename = "endLine")]
    end_line: usize,
    #[serde(rename = "endCharacter")]
    end_character: usize,
    related: Vec<RelatedJson>,
}

impl DiagnosticJson {
    fn from_cft(
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

    fn from_excel_check(diagnostic: &ExcelDiagnostic) -> Self {
        let fallback = ExcelLocation::new("");
        let location = diagnostic
            .primary
            .as_ref()
            .map(|label| &label.location)
            .unwrap_or(&fallback);
        let (line, character) = excel_position(location);
        Self {
            code: diagnostic.source.code.as_str().to_string(),
            stage: diagnostic.source.stage.to_string(),
            severity: "error".to_string(),
            message: diagnostic.source.message.clone(),
            path: location.file.display().to_string(),
            start_line: line,
            start_character: character,
            end_line: line,
            end_character: character.saturating_add(1),
            related: diagnostic
                .related
                .iter()
                .map(|label| RelatedJson::from_excel(&label.location, label.message.clone()))
                .collect(),
        }
    }

    fn from_excel_error(
        code: impl Into<String>,
        stage: impl Into<String>,
        message: String,
    ) -> Self {
        Self {
            code: code.into(),
            stage: stage.into(),
            severity: "error".to_string(),
            message,
            path: String::new(),
            start_line: 0,
            start_character: 0,
            end_line: 0,
            end_character: 1,
            related: Vec::new(),
        }
    }

    fn from_excel_location(
        code: impl Into<String>,
        stage: impl Into<String>,
        message: String,
        location: &ExcelLocation,
    ) -> Self {
        let (line, character) = excel_position(location);
        Self {
            code: code.into(),
            stage: stage.into(),
            severity: "error".to_string(),
            message,
            path: location.file.display().to_string(),
            start_line: line,
            start_character: character,
            end_line: line,
            end_character: character.saturating_add(1),
            related: Vec::new(),
        }
    }
}

#[derive(Debug, Serialize)]
struct RelatedJson {
    path: String,
    #[serde(rename = "startLine")]
    start_line: usize,
    #[serde(rename = "startCharacter")]
    start_character: usize,
    #[serde(rename = "endLine")]
    end_line: usize,
    #[serde(rename = "endCharacter")]
    end_character: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<String>,
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

    fn from_excel(location: &ExcelLocation, label: Option<String>) -> Self {
        let (line, character) = excel_position(location);
        Self {
            path: location.file.display().to_string(),
            start_line: line,
            start_character: character,
            end_line: line,
            end_character: character.saturating_add(1),
            label,
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

fn excel_position(location: &ExcelLocation) -> (usize, usize) {
    (
        location.row.unwrap_or(1).saturating_sub(1),
        location.column.unwrap_or(1).saturating_sub(1),
    )
}

fn diagnostics_from_excel_checks(checks: &ExcelDiagnostics) -> Vec<DiagnosticJson> {
    checks
        .diagnostics
        .iter()
        .map(DiagnosticJson::from_excel_check)
        .collect()
}

fn diagnostics_from_excel_error(err: &ExcelLoadError) -> Vec<DiagnosticJson> {
    match err {
        ExcelLoadError::OpenWorkbook { file, message } => vec![DiagnosticJson::from_excel_error(
            "EXCEL-OPEN",
            "EXCEL",
            format!("failed to open workbook `{}`: {message}", file.display()),
        )],
        ExcelLoadError::ReadSheet { location, message } => {
            vec![DiagnosticJson::from_excel_location(
                "EXCEL-SHEET",
                "EXCEL",
                message.clone(),
                location,
            )]
        }
        ExcelLoadError::MissingSheet { file, sheet } => vec![DiagnosticJson::from_excel_error(
            "EXCEL-SHEET",
            "EXCEL",
            format!("workbook `{}` is missing sheet `{sheet}`", file.display()),
        )],
        ExcelLoadError::EmptySheet { location } => vec![DiagnosticJson::from_excel_location(
            "EXCEL-SHEET",
            "EXCEL",
            "sheet is empty".to_string(),
            location,
        )],
        ExcelLoadError::UnknownType {
            location,
            type_name,
        } => vec![DiagnosticJson::from_excel_location(
            "EXCEL-TYPE",
            "EXCEL",
            format!("unknown CFT type `{type_name}`"),
            location,
        )],
        ExcelLoadError::UnknownColumn {
            location,
            type_name,
            column,
            field,
        } => vec![DiagnosticJson::from_excel_location(
            "EXCEL-COLUMN",
            "EXCEL",
            format!("column `{column}` maps to unknown field `{field}` on type `{type_name}`"),
            location,
        )],
        ExcelLoadError::DuplicateFieldColumn {
            location,
            field,
            first_column,
            duplicate_column,
        } => vec![DiagnosticJson::from_excel_location(
            "EXCEL-COLUMN",
            "EXCEL",
            format!("field `{field}` is mapped by both `{first_column}` and `{duplicate_column}`"),
            location,
        )],
        ExcelLoadError::CellParse {
            location,
            type_name,
            field,
            diagnostics,
        } => diagnostics
            .diagnostics
            .iter()
            .map(|diag| {
                DiagnosticJson::from_excel_location(
                    format!("CELL-{:?}", diag.code),
                    "CELL",
                    format!(
                        "failed to parse `{type_name}.{field}` cell: {}",
                        diag.message
                    ),
                    location,
                )
            })
            .collect(),
        ExcelLoadError::DataModel(diagnostics) => diagnostics_from_excel_checks(diagnostics),
    }
}

fn write_json_diagnostics(diagnostics: Vec<DiagnosticJson>) -> Result<(), String> {
    serde_json::to_writer(io::stdout().lock(), &DiagnosticsOutput { diagnostics })
        .map_err(|err| format!("failed to write diagnostics JSON: {err}"))?;
    println!();
    Ok(())
}

fn write_human_cft_diagnostics(
    diagnostics: &[CftDiagnostic],
    sources: &BTreeMap<String, String>,
    paths: &BTreeMap<String, String>,
) -> Result<(), String> {
    let mut stderr = io::stderr().lock();
    for diagnostic in diagnostics {
        let json = DiagnosticJson::from_cft(diagnostic, sources, paths);
        writeln!(
            stderr,
            "{} [{}] {}:{}:{} {}",
            diagnostic.code.as_str(),
            diagnostic.stage,
            json.path,
            json.start_line + 1,
            json.start_character + 1,
            diagnostic.message
        )
        .map_err(|err| format!("failed to write diagnostics: {err}"))?;
    }
    Ok(())
}

fn write_human_excel_diagnostics(diagnostics: &ExcelDiagnostics) -> Result<(), String> {
    let mut stderr = io::stderr().lock();
    for diagnostic in &diagnostics.diagnostics {
        let location = diagnostic.primary.as_ref().map(|label| &label.location);
        writeln!(
            stderr,
            "{} [{}] {} {}",
            diagnostic.source.code.as_str(),
            diagnostic.source.stage,
            location.map_or_else(String::new, format_excel_location),
            diagnostic.source.message
        )
        .map_err(|err| format!("failed to write diagnostics: {err}"))?;
    }
    Ok(())
}

fn write_human_excel_error(err: &ExcelLoadError) -> Result<(), String> {
    let mut stderr = io::stderr().lock();
    for diagnostic in diagnostics_from_excel_error(err) {
        writeln!(
            stderr,
            "{} [{}] {}:{}:{} {}",
            diagnostic.code,
            diagnostic.stage,
            diagnostic.path,
            diagnostic.start_line + 1,
            diagnostic.start_character + 1,
            diagnostic.message
        )
        .map_err(|err| format!("failed to write diagnostics: {err}"))?;
    }
    Ok(())
}

fn format_excel_location(location: &ExcelLocation) -> String {
    let mut out = location.file.display().to_string();
    if let Some(sheet) = &location.sheet {
        out.push('#');
        out.push_str(sheet);
    }
    if let Some(row) = location.row {
        out.push(':');
        out.push_str(&row.to_string());
        if let Some(column) = location.column {
            out.push(':');
            out.push_str(&column.to_string());
        }
    }
    out
}

fn path_to_slash(path: &Path) -> String {
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

fn normalize_path(path: &Path) -> PathBuf {
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
