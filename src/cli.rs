use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "coflow", about = "CFD schema validation and multi-language code generation.", version)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    Init(InitArgs),
    Cft(CftArgs),
    Lsp(LspArgs),
    Check(ProjectCheckArgs),
    Build(BuildArgs),
    Clean(CleanArgs),
    Codegen(CodegenArgs),
    Schema(SchemaArgs),
    Skill(SkillArgs),
    SelfUpdate(SelfUpdateArgs),
}

#[derive(Debug, Args)]
pub(crate) struct SelfUpdateArgs {
    #[arg(long)]
    pub(crate) check: bool,
    #[arg(long, short = 'y')]
    pub(crate) yes: bool,
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
    Install(SkillScopeArgs),
    Uninstall(SkillScopeArgs),
    Status(SkillScopeArgs),
}

#[derive(Debug, Args)]
pub(crate) struct SkillScopeArgs {
    #[arg(value_name = "CONFIG_OR_DIR", conflicts_with = "global")]
    pub(crate) config_or_dir: Option<PathBuf>,
    #[arg(short = 'g', long)]
    pub(crate) global: bool,
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
    Check(CftCheckArgs),
}

#[derive(Debug, Args)]
pub(crate) struct CftCheckArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    pub(crate) config_or_dir: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
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
    Inspect(SchemaInspectArgs),
    Files(SchemaFilesArgs),
    WriteFile(SchemaWriteFileArgs),
}

#[derive(Debug, Args)]
pub(crate) struct SchemaInspectArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    pub(crate) config_or_dir: Option<PathBuf>,
    #[arg(long = "type", value_name = "TYPE")]
    pub(crate) type_filter: Option<String>,
    #[arg(long)]
    pub(crate) include_derived: bool,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct SchemaFilesArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    pub(crate) config_or_dir: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct SchemaWriteFileArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    pub(crate) config_or_dir: Option<PathBuf>,
    #[arg(long, value_name = "FILE")]
    pub(crate) file: String,
    #[arg(long)]
    pub(crate) dry_run: bool,
    #[arg(long)]
    pub(crate) check: bool,
    #[arg(long)]
    pub(crate) json: bool,
}
