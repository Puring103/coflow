use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "coflow")]
#[command(about = "Project-level tools for Coflow schemas and data.")]
#[command(version)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::{Cli, Command, SchemaCommand};
    use clap::{error::ErrorKind, Parser};
    use std::path::PathBuf;

    #[test]
    #[allow(clippy::expect_used)]
    fn version_flags_report_package_version() {
        for flag in ["--version", "-V"] {
            let error = Cli::try_parse_from(["coflow", flag]).expect_err("version exits early");
            assert_eq!(error.kind(), ErrorKind::DisplayVersion);
            assert_eq!(
                error.to_string(),
                format!("coflow {}\n", env!("CARGO_PKG_VERSION"))
            );
        }
    }

    #[test]
    fn artifact_commands_only_accept_the_project_argument() {
        let cli =
            Cli::try_parse_from(["coflow", "export", "project"]).expect("parse export project");
        let Command::Export(args) = cli.command else {
            panic!("expected export command");
        };
        assert_eq!(args.config_or_dir, Some(PathBuf::from("project")));

        let cli =
            Cli::try_parse_from(["coflow", "codegen", "project"]).expect("parse codegen project");
        let Command::Codegen(args) = cli.command else {
            panic!("expected codegen command");
        };
        assert_eq!(args.config_or_dir, Some(PathBuf::from("project")));

        for args in [
            ["coflow", "build", "--namespace"],
            ["coflow", "export", "--out"],
            ["coflow", "codegen", "--out"],
        ] {
            let error = Cli::try_parse_from(args).expect_err("override option was removed");
            assert_eq!(error.kind(), ErrorKind::UnknownArgument);
        }
    }

    #[test]
    fn schema_and_data_commands_default_to_human_output() {
        let cli = Cli::try_parse_from(["coflow", "schema", "inspect"])
            .expect("parse default schema output");
        let Command::Schema(schema) = cli.command else {
            panic!("expected schema command");
        };
        let SchemaCommand::Inspect(args) = schema.command else {
            panic!("expected schema inspect command");
        };
        assert!(!args.json);

        let cli = Cli::try_parse_from(["coflow", "schema", "inspect", "--json"])
            .expect("parse JSON schema output");
        let Command::Schema(schema) = cli.command else {
            panic!("expected schema command");
        };
        let SchemaCommand::Inspect(args) = schema.command else {
            panic!("expected schema inspect command");
        };
        assert!(args.json);

        let error = Cli::try_parse_from(["coflow", "schema", "inspect", "--human"])
            .expect_err("--human was removed");
        assert_eq!(error.kind(), ErrorKind::UnknownArgument);
    }
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
    /// Remove historical artifact generations and abandoned temporary files.
    Clean(CleanArgs),
    /// Export project data.
    Export(ExportArgs),
    /// Generate runtime code.
    Codegen(CodegenArgs),
    /// Schema inspection tools for automation and AI agents.
    Schema(SchemaArgs),
    /// Data inspection and patch tools for automation and AI agents.
    Data(DataArgs),
    /// Install, remove, or inspect bundled Coflow agent skills.
    Skill(SkillArgs),
}

#[derive(Debug, Args)]
pub(crate) struct InitArgs {
    #[arg(value_name = "DIR")]
    pub(crate) dir: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub(crate) struct SkillArgs {
    #[command(subcommand)]
    pub(crate) command: SkillCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum SkillCommand {
    /// Install bundled skills into a project or the current user's agent directories.
    Install(SkillScopeArgs),
    /// Remove bundled skills from a project or the current user's agent directories.
    Uninstall(SkillScopeArgs),
    /// Show bundled skill installation status.
    Status(SkillScopeArgs),
}

#[derive(Debug, Args)]
pub(crate) struct SkillScopeArgs {
    #[arg(value_name = "CONFIG_OR_DIR", conflicts_with = "global")]
    pub(crate) config_or_dir: Option<PathBuf>,
    /// Use current-user global agent skill directories instead of a Coflow project.
    #[arg(short = 'g', long)]
    pub(crate) global: bool,
    /// Emit machine-readable JSON.
    #[arg(long)]
    pub(crate) json: bool,
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
}

#[derive(Debug, Args)]
pub(crate) struct CleanArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    pub(crate) config_or_dir: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub(crate) struct ExportArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    pub(crate) config_or_dir: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub(crate) struct CodegenArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    pub(crate) config_or_dir: Option<PathBuf>,
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
    /// Emit machine-readable JSON instead of human-readable text.
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct SchemaFilesArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    pub(crate) config_or_dir: Option<PathBuf>,
    /// Emit machine-readable JSON instead of human-readable text.
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct SchemaWriteFileArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    pub(crate) config_or_dir: Option<PathBuf>,
    /// Project-relative configured .cft schema file to write.
    #[arg(long, value_name = "FILE")]
    pub(crate) file: String,
    /// Validate and report without writing the file.
    #[arg(long)]
    pub(crate) dry_run: bool,
    /// Compile the schema after writing, or against the in-memory source in --dry-run mode.
    #[arg(long)]
    pub(crate) check: bool,
    /// Emit machine-readable JSON instead of human-readable text.
    #[arg(long)]
    pub(crate) json: bool,
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
    /// Emit machine-readable JSON instead of human-readable text.
    #[arg(long)]
    pub(crate) json: bool,
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
    /// Emit machine-readable JSON instead of human-readable text.
    #[arg(long)]
    pub(crate) json: bool,
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
    /// Emit machine-readable JSON instead of human-readable text.
    #[arg(long)]
    pub(crate) json: bool,
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
    /// Emit machine-readable JSON instead of human-readable text.
    #[arg(long)]
    pub(crate) json: bool,
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
    /// Emit machine-readable JSON instead of human-readable text.
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct DataCreateTableArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    pub(crate) config_or_dir: Option<PathBuf>,
    /// Project-relative table file.
    #[arg(long, value_name = "SOURCE")]
    pub(crate) source: String,
    /// Concrete record type for table headers.
    #[arg(long = "type", value_name = "TYPE")]
    pub(crate) actual_type: Option<String>,
    /// Table provider, inferred from the local file when omitted.
    #[arg(long, value_name = "PROVIDER")]
    pub(crate) provider: Option<String>,
    /// Sheet name to create.
    #[arg(long, value_name = "SHEET")]
    pub(crate) sheet: Option<String>,
    /// Emit machine-readable JSON instead of human-readable text.
    #[arg(long)]
    pub(crate) json: bool,
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
    /// Emit machine-readable JSON instead of human-readable text.
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct DataWriteFileArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    pub(crate) config_or_dir: Option<PathBuf>,
    /// Project-relative configured .cfd data file to write.
    #[arg(long, value_name = "FILE")]
    pub(crate) file: String,
    /// Validate and report without writing the file.
    #[arg(long)]
    pub(crate) dry_run: bool,
    /// Run full project validation after writing. In --dry-run mode this is skipped.
    #[arg(long)]
    pub(crate) check: bool,
    /// Emit machine-readable JSON instead of human-readable text.
    #[arg(long)]
    pub(crate) json: bool,
}
