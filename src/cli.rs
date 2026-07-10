use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "coflow")]
#[command(about = "Project-level tools for Coflow schemas and data.")]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Create a minimal Coflow project.
    Init(InitArgs),
    /// CFT schema tools.
    Cft(CftArgs),
    /// Start the Coflow language server (CFT + CFD).
    Lsp(LspArgs),
    /// Run the full project validation pipeline.
    Check(ProjectCheckArgs),
    /// Run validation, data export, and configured code generation.
    Build(BuildArgs),
    /// Export project data.
    Export(ExportArgs),
    /// Generate runtime code.
    Codegen(CodegenArgs),
    /// Schema inspection tools for automation and AI agents.
    Schema(SchemaArgs),
    /// Data inspection and patch tools for automation and AI agents.
    Data(DataArgs),
}

#[derive(Debug, Args)]
pub(crate) struct InitArgs {
    #[arg(value_name = "DIR")]
    pub(crate) dir: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub(crate) struct CftArgs {
    #[command(subcommand)]
    pub(crate) command: CftCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum CftCommand {
    /// Compile all CFT schema files from coflow.yaml.
    Check(CftCheckArgs),
}

#[derive(Debug, Args)]
pub(crate) struct CftCheckArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    pub(crate) config_or_dir: Option<PathBuf>,
    /// Emit machine-readable diagnostics JSON.
    #[arg(long)]
    pub(crate) json: bool,
    /// Treat stdin as this schema file's source.
    #[arg(long = "stdin-path", value_name = "PATH")]
    pub(crate) stdin_path: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub(crate) struct LspArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    pub(crate) config_or_dir: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub(crate) struct ProjectCheckArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    pub(crate) config_or_dir: Option<PathBuf>,
    /// Emit machine-readable diagnostics JSON.
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct BuildArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    pub(crate) config_or_dir: Option<PathBuf>,
    /// Override outputs.data.dir for this invocation.
    #[arg(long = "data-out", value_name = "DIR")]
    pub(crate) data_out_dir: Option<PathBuf>,
    /// Override outputs.code.dir for this invocation.
    #[arg(long = "code-out", value_name = "DIR")]
    pub(crate) code_out_dir: Option<PathBuf>,
    /// Override outputs.code.namespace for this invocation.
    #[arg(long, value_name = "NAME")]
    pub(crate) namespace: Option<String>,
}

#[derive(Debug, Args)]
pub(crate) struct ExportArgs {
    #[command(subcommand)]
    pub(crate) command: ExportCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum ExportCommand {
    /// Export data as JSON. The project config must declare outputs.data.type: json.
    Json(ExportJsonArgs),
    /// Export data as `MessagePack`. The project config must declare outputs.data.type: messagepack.
    Messagepack(ExportMessagePackArgs),
}

#[derive(Debug, Args)]
pub(crate) struct ExportJsonArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    pub(crate) config_or_dir: Option<PathBuf>,
    /// Override outputs.data.dir for this invocation.
    #[arg(long = "out", value_name = "DIR")]
    pub(crate) out_dir: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub(crate) struct ExportMessagePackArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    pub(crate) config_or_dir: Option<PathBuf>,
    /// Override outputs.data.dir for this invocation.
    #[arg(long = "out", value_name = "DIR")]
    pub(crate) out_dir: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub(crate) struct CodegenArgs {
    #[command(subcommand)]
    pub(crate) command: CodegenCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum CodegenCommand {
    /// Generate C# runtime code. The project config must declare outputs.code.type: csharp.
    Csharp(CodegenCsharpArgs),
}

#[derive(Debug, Args)]
pub(crate) struct CodegenCsharpArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    pub(crate) config_or_dir: Option<PathBuf>,
    /// Override outputs.code.dir for this invocation.
    #[arg(long = "out", value_name = "DIR")]
    pub(crate) out_dir: Option<PathBuf>,
    /// Override outputs.code.namespace for this invocation.
    #[arg(long, value_name = "NAME")]
    pub(crate) namespace: Option<String>,
}

#[derive(Debug, Args)]
pub(crate) struct SchemaArgs {
    #[command(subcommand)]
    pub(crate) command: SchemaCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum SchemaCommand {
    /// Inspect compiled schema types, enums, consts, and diagnostics.
    Inspect(SchemaInspectArgs),
    /// Print compiled schema file sources.
    Files(SchemaFilesArgs),
    /// Write a configured CFT schema file from stdin.
    WriteFile(SchemaWriteFileArgs),
}

#[derive(Debug, Args)]
pub(crate) struct SchemaInspectArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    pub(crate) config_or_dir: Option<PathBuf>,
    /// Restrict output to a schema type.
    #[arg(long = "type", value_name = "TYPE")]
    pub(crate) type_filter: Option<String>,
    /// Include derived types when --type is supplied.
    #[arg(long)]
    pub(crate) include_derived: bool,
    /// Emit human-readable text instead of JSON.
    #[arg(long)]
    pub(crate) human: bool,
}

#[derive(Debug, Args)]
pub(crate) struct SchemaFilesArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    pub(crate) config_or_dir: Option<PathBuf>,
    /// Emit human-readable text instead of JSON.
    #[arg(long)]
    pub(crate) human: bool,
}

#[derive(Debug, Args)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct SchemaWriteFileArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    pub(crate) config_or_dir: Option<PathBuf>,
    /// Project-relative configured .cft schema file to write.
    #[arg(long, value_name = "FILE")]
    pub(crate) file: String,
    /// Read the replacement CFT source from stdin.
    #[arg(long)]
    pub(crate) stdin: bool,
    /// Validate and report without writing the file.
    #[arg(long)]
    pub(crate) dry_run: bool,
    /// Compile the schema after writing, or against the in-memory source in --dry-run mode.
    #[arg(long)]
    pub(crate) check: bool,
    /// Emit human-readable text instead of JSON.
    #[arg(long)]
    pub(crate) human: bool,
}

#[derive(Debug, Args)]
pub(crate) struct DataArgs {
    #[command(subcommand)]
    pub(crate) command: DataCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum DataCommand {
    /// List configured and resolved data sources.
    Sources(DataSourcesArgs),
    /// List record coordinates.
    List(DataListArgs),
    /// Fetch complete records.
    Get(DataGetArgs),
    /// Apply a JSON data patch through provider writers.
    Patch(DataPatchArgs),
    /// Create a local data file, including table headers when applicable.
    CreateFile(DataCreateFileArgs),
    /// Create a sheet/table in a table source and write its header.
    CreateTable(DataCreateTableArgs),
    /// Synchronize local data file columns with the latest schema.
    SyncHeader(DataSyncHeaderArgs),
    /// Write a configured local CFD data file from stdin.
    WriteFile(DataWriteFileArgs),
}

#[derive(Debug, Args)]
pub(crate) struct DataSourcesArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    pub(crate) config_or_dir: Option<PathBuf>,
    /// Emit human-readable text instead of JSON.
    #[arg(long)]
    pub(crate) human: bool,
}

#[derive(Debug, Args)]
pub(crate) struct DataListArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    pub(crate) config_or_dir: Option<PathBuf>,
    /// Restrict output to a concrete record type.
    #[arg(long = "type", value_name = "TYPE")]
    pub(crate) actual_type: Option<String>,
    /// Restrict output to a project-relative source file.
    #[arg(long, value_name = "FILE")]
    pub(crate) file: Option<String>,
    /// Maximum number of records to return.
    #[arg(long)]
    pub(crate) limit: Option<usize>,
    /// Number of matching records to skip.
    #[arg(long, default_value_t = 0)]
    pub(crate) offset: usize,
    /// Emit human-readable text instead of JSON.
    #[arg(long)]
    pub(crate) human: bool,
}

#[derive(Debug, Args)]
pub(crate) struct DataGetArgs {
    // Clap cannot disambiguate two optional positionals reliably. Parse this
    // tail manually so `coflow data get <project> Item.sword` stays supported.
    /// Optional `CONFIG_OR_DIR` and `TYPE.KEY` tail.
    #[arg(value_name = "CONFIG_OR_DIR_OR_TYPE.KEY", num_args = 0..=2)]
    pub(crate) target: Vec<String>,
    /// Restrict output to a concrete record type.
    #[arg(long = "type", value_name = "TYPE")]
    pub(crate) actual_type: Option<String>,
    /// Restrict output to a project-relative source file.
    #[arg(long, value_name = "FILE")]
    pub(crate) file: Option<String>,
    /// Restrict output to comma-separated keys.
    #[arg(long, value_delimiter = ',')]
    pub(crate) keys: Vec<String>,
    /// Maximum number of records to return.
    #[arg(long)]
    pub(crate) limit: Option<usize>,
    /// Number of matching records to skip.
    #[arg(long, default_value_t = 0)]
    pub(crate) offset: usize,
    /// Fetch all matching records without the default safety limit.
    #[arg(long)]
    pub(crate) all: bool,
    /// Emit human-readable text instead of JSON.
    #[arg(long)]
    pub(crate) human: bool,
}

#[derive(Debug, Args)]
pub(crate) struct DataPatchArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    pub(crate) config_or_dir: Option<PathBuf>,
    /// JSON patch request string.
    #[arg(long, value_name = "JSON", conflicts_with_all = ["patch_file", "stdin"])]
    pub(crate) patch: Option<String>,
    /// JSON patch request file.
    #[arg(long = "patch-file", value_name = "PATCH_FILE", conflicts_with_all = ["patch", "stdin"])]
    pub(crate) patch_file: Option<PathBuf>,
    /// Read the JSON patch request from stdin.
    #[arg(long, conflicts_with_all = ["patch", "patch_file"])]
    pub(crate) stdin: bool,
    /// Emit human-readable text instead of JSON.
    #[arg(long)]
    pub(crate) human: bool,
}

#[derive(Debug, Args)]
pub(crate) struct DataCreateFileArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    pub(crate) config_or_dir: Option<PathBuf>,
    /// Project-relative file path to create.
    #[arg(long, value_name = "FILE")]
    pub(crate) file: String,
    /// Concrete record type for table headers.
    #[arg(long = "type", value_name = "TYPE")]
    pub(crate) actual_type: Option<String>,
    /// Data provider: cfd, csv, or excel. Inferred from extension when omitted.
    #[arg(long, value_name = "PROVIDER")]
    pub(crate) provider: Option<String>,
    /// Sheet name for Excel/table sources.
    #[arg(long, value_name = "SHEET")]
    pub(crate) sheet: Option<String>,
    /// Emit human-readable text instead of JSON.
    #[arg(long)]
    pub(crate) human: bool,
}

#[derive(Debug, Args)]
pub(crate) struct DataCreateTableArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    pub(crate) config_or_dir: Option<PathBuf>,
    /// Project-relative Excel file or configured remote source URI.
    #[arg(long, value_name = "SOURCE")]
    pub(crate) source: String,
    /// Concrete record type for table headers.
    #[arg(long = "type", value_name = "TYPE")]
    pub(crate) actual_type: Option<String>,
    /// Table provider: excel or lark-sheet.
    #[arg(long, value_name = "PROVIDER")]
    pub(crate) provider: Option<String>,
    /// Sheet name to create.
    #[arg(long, value_name = "SHEET")]
    pub(crate) sheet: Option<String>,
    /// Emit human-readable text instead of JSON.
    #[arg(long)]
    pub(crate) human: bool,
}

#[derive(Debug, Args)]
pub(crate) struct DataSyncHeaderArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    pub(crate) config_or_dir: Option<PathBuf>,
    /// Project-relative file path to update.
    #[arg(long, value_name = "FILE")]
    pub(crate) file: String,
    /// Concrete record type whose fields define the target columns.
    #[arg(long = "type", value_name = "TYPE")]
    pub(crate) actual_type: String,
    /// Data provider: cfd, csv, or excel. Inferred from extension when omitted.
    #[arg(long, value_name = "PROVIDER")]
    pub(crate) provider: Option<String>,
    /// Sheet name for Excel/table sources.
    #[arg(long, value_name = "SHEET")]
    pub(crate) sheet: Option<String>,
    /// Emit human-readable text instead of JSON.
    #[arg(long)]
    pub(crate) human: bool,
}

#[derive(Debug, Args)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct DataWriteFileArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    pub(crate) config_or_dir: Option<PathBuf>,
    /// Project-relative configured .cfd data file to write.
    #[arg(long, value_name = "FILE")]
    pub(crate) file: String,
    /// Read the replacement CFD source from stdin.
    #[arg(long)]
    pub(crate) stdin: bool,
    /// Validate and report without writing the file.
    #[arg(long)]
    pub(crate) dry_run: bool,
    /// Run full project validation after writing. In --dry-run mode this is skipped.
    #[arg(long)]
    pub(crate) check: bool,
    /// Emit human-readable text instead of JSON.
    #[arg(long)]
    pub(crate) human: bool,
}
