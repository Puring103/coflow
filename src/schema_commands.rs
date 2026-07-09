use crate::diagnostics::{cli_error, cli_file_error};
use coflow_api::{DiagnosticSet, FlatDiagnostic};
use coflow_project::{
    compile_schema_project_with_overrides, dedupe_cft_diagnostics, diagnostic_set_from_cft,
    path_to_slash, Project, SchemaSourceOverride,
};
use coflow_runtime::{
    build_project_schema_session, inspect_schema, schema_files, SchemaFilesReport,
    SchemaInspectReport,
};
use serde::Serialize;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct SchemaWriteFileOptions {
    pub file: String,
    pub input: SchemaWriteInput,
    pub mode: SchemaWriteMode,
    pub check: SchemaWriteCheck,
    pub output: SchemaWriteOutput,
}

#[derive(Debug, Serialize)]
pub struct SchemaWriteFileReport {
    pub file: String,
    pub written: bool,
    pub dry_run: bool,
    pub changed: bool,
    pub check_ok: Option<bool>,
    pub diagnostics: Vec<FlatDiagnostic>,
}

#[derive(Debug, Clone, Copy)]
pub enum SchemaWriteInput {
    Stdin,
    Missing,
}

#[derive(Debug, Clone, Copy)]
pub enum SchemaWriteMode {
    Write,
    DryRun,
}

#[derive(Debug, Clone, Copy)]
pub enum SchemaWriteCheck {
    Run,
    Skip,
}

#[derive(Debug, Clone, Copy)]
pub enum SchemaWriteOutput {
    Json,
    Human,
}

/// Inspects the compiled project schema.
///
/// # Errors
///
/// Returns an error when the project cannot be opened, the schema session
/// cannot be built, or output cannot be written.
pub fn inspect(
    config_or_dir: Option<&Path>,
    type_filter: Option<&str>,
    include_derived: bool,
    human: bool,
) -> Result<bool, DiagnosticSet> {
    let project = Project::open_schema_only(config_or_dir)?;
    let session = build_project_schema_session(project)?;
    let report = inspect_schema(&session, type_filter, include_derived);
    if human {
        write_schema_inspect_human(&report)?;
    } else {
        write_json(&report)?;
    }
    Ok(report.diagnostics.is_empty())
}

/// Prints compiled schema file sources.
///
/// # Errors
///
/// Returns an error when the project cannot be opened, the schema session
/// cannot be built, or output cannot be written.
pub fn files(config_or_dir: Option<&Path>, human: bool) -> Result<bool, DiagnosticSet> {
    let project = Project::open_schema_only(config_or_dir)?;
    let session = build_project_schema_session(project)?;
    let report = schema_files(&session);
    if human {
        write_schema_files_human(&report)?;
    } else {
        write_json(&report)?;
    }
    Ok(report.diagnostics.is_empty())
}

/// Writes a configured CFT schema file from stdin.
///
/// # Errors
///
/// Returns an error when the project cannot be opened, the target is not a
/// configured `.cft` schema file, stdin cannot be read, the file cannot be
/// written, or output cannot be written. User-fixable CFT diagnostics from
/// `--check` are written as command output and return `Ok(false)`.
pub fn write_file(
    config_or_dir: Option<&Path>,
    options: &SchemaWriteFileOptions,
) -> Result<bool, DiagnosticSet> {
    let project = Project::open_schema_only(config_or_dir)?;
    let target = resolve_schema_write_target(&project, &options.file)?;
    let current = std::fs::read_to_string(&target.absolute_path).map_err(|err| {
        cli_file_error(
            &target.absolute_path,
            "CLI-FILE-READ",
            format!("failed to read `{}`: {err}", target.absolute_path.display()),
        )
    })?;
    let source = match options.input {
        SchemaWriteInput::Stdin => read_stdin_source()?,
        SchemaWriteInput::Missing => {
            return Err(cli_error("CLI-ARG", "schema write-file requires --stdin"));
        }
    };
    let changed = current != source;

    let diagnostics = if matches!(options.check, SchemaWriteCheck::Run) {
        check_schema_source(&project, &target, &source)?
    } else {
        Vec::new()
    };
    let dry_run = matches!(options.mode, SchemaWriteMode::DryRun);
    if !dry_run {
        std::fs::write(&target.absolute_path, &source).map_err(|err| {
            cli_file_error(
                &target.absolute_path,
                "CLI-FILE-WRITE",
                format!(
                    "failed to write `{}`: {err}",
                    target.absolute_path.display()
                ),
            )
        })?;
    }
    let check_ok = matches!(options.check, SchemaWriteCheck::Run).then_some(diagnostics.is_empty());
    let report = SchemaWriteFileReport {
        file: target.project_path,
        written: !dry_run,
        dry_run,
        changed,
        check_ok,
        diagnostics,
    };
    let ok = report.check_ok.unwrap_or(true);
    match options.output {
        SchemaWriteOutput::Json => write_json(&report)?,
        SchemaWriteOutput::Human => write_schema_write_file_human(&report)?,
    }
    Ok(ok)
}

fn write_json(value: &impl Serialize) -> Result<(), DiagnosticSet> {
    serde_json::to_writer(io::stdout().lock(), value)
        .map_err(|err| cli_error("CLI-OUTPUT", format!("failed to write JSON: {err}")))?;
    println!();
    Ok(())
}

fn read_stdin_source() -> Result<String, DiagnosticSet> {
    let mut source = String::new();
    io::stdin()
        .read_to_string(&mut source)
        .map_err(|err| cli_error("CLI-STDIN", format!("failed to read stdin: {err}")))?;
    Ok(source)
}

#[derive(Debug)]
struct SchemaWriteTarget {
    absolute_path: PathBuf,
    canonical_path: PathBuf,
    project_path: String,
    module_id: String,
}

fn resolve_schema_write_target(
    project: &Project,
    file: &str,
) -> Result<SchemaWriteTarget, DiagnosticSet> {
    let requested_path = Path::new(file);
    if requested_path
        .extension()
        .and_then(|extension| extension.to_str())
        != Some("cft")
    {
        return Err(cli_error(
            "SCHEMA-WRITE-TARGET",
            format!("`--file {file}` must name a configured .cft schema file"),
        ));
    }
    let requested_absolute = project.resolve_path(requested_path);
    let requested_canonical = std::fs::canonicalize(&requested_absolute).map_err(|err| {
        cli_file_error(
            &requested_absolute,
            "SCHEMA-WRITE-TARGET",
            format!(
                "failed to resolve schema file `{}`: {err}",
                requested_absolute.display()
            ),
        )
    })?;
    let schema_files = project.schema_files()?;
    let Some(schema_file) = schema_files
        .into_iter()
        .find(|schema_file| schema_file.canonical_path == requested_canonical)
    else {
        return Err(cli_error(
            "SCHEMA-WRITE-TARGET",
            format!("`--file {file}` is not part of the configured schema"),
        ));
    };
    let project_path = schema_file
        .canonical_path
        .strip_prefix(&project.root_dir)
        .map_or_else(
            |_| path_to_slash(&schema_file.canonical_path),
            path_to_slash,
        );
    Ok(SchemaWriteTarget {
        absolute_path: schema_file.path,
        canonical_path: schema_file.canonical_path,
        project_path,
        module_id: schema_file.module_id,
    })
}

fn check_schema_source(
    project: &Project,
    target: &SchemaWriteTarget,
    source: &str,
) -> Result<Vec<FlatDiagnostic>, DiagnosticSet> {
    let mut diagnostics = project.schema_diagnostic_set();
    let build = compile_schema_project_with_overrides(
        project,
        &[SchemaSourceOverride {
            requested_module: Some(target.module_id.clone()),
            normalized_path: target.canonical_path.clone(),
            source: source.to_string(),
        }],
    )?;
    diagnostics.extend(diagnostic_set_from_cft(
        dedupe_cft_diagnostics(build.diagnostics),
        &build.sources,
        &build.paths,
    ));
    Ok(diagnostics
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.flat_view(None, None, None))
        .collect())
}

fn write_schema_inspect_human(report: &SchemaInspectReport) -> Result<(), DiagnosticSet> {
    let mut stdout = io::stdout().lock();
    for ty in &report.types {
        writeln!(stdout, "type {}", ty.name)
            .map_err(output_error)?;
        for annotation in &ty.annotations {
            writeln!(stdout, "  @{}", annotation.name)
                .map_err(output_error)?;
        }
        for field in &ty.fields {
            writeln!(stdout, "  {}: {}", field.name, field.raw_type)
                .map_err(output_error)?;
        }
    }
    for schema_enum in &report.enums {
        writeln!(stdout, "enum {}", schema_enum.name)
            .map_err(output_error)?;
        for variant in &schema_enum.variants {
            writeln!(stdout, "  {} = {}", variant.name, variant.value)
                .map_err(output_error)?;
        }
    }
    for schema_const in &report.consts {
        writeln!(stdout, "const {}", schema_const.name)
            .map_err(output_error)?;
    }
    write_flat_diagnostics(&mut stdout, &report.diagnostics)
}

fn write_schema_files_human(report: &SchemaFilesReport) -> Result<(), DiagnosticSet> {
    let mut stdout = io::stdout().lock();
    for file in &report.files {
        writeln!(stdout, "{}", file.module)
            .map_err(output_error)?;
        writeln!(stdout, "{}", file.source)
            .map_err(output_error)?;
    }
    write_flat_diagnostics(&mut stdout, &report.diagnostics)
}

fn write_schema_write_file_human(report: &SchemaWriteFileReport) -> Result<(), DiagnosticSet> {
    let mut stdout = io::stdout().lock();
    writeln!(
        stdout,
        "{}\twritten={}\tdry_run={}\tchanged={}\tcheck_ok={}",
        report.file,
        report.written,
        report.dry_run,
        report.changed,
        report
            .check_ok
            .map_or_else(|| "skipped".to_string(), |ok| ok.to_string())
    )
    .map_err(output_error)?;
    write_flat_diagnostics(&mut stdout, &report.diagnostics)
}

fn write_flat_diagnostics(
    stdout: &mut impl Write,
    diagnostics: &[FlatDiagnostic],
) -> Result<(), DiagnosticSet> {
    for diagnostic in diagnostics {
        writeln!(
            stdout,
            "[{}] [{}] {}",
            diagnostic.code, diagnostic.stage, diagnostic.message
        )
        .map_err(output_error)?;
    }
    Ok(())
}

fn output_error(err: io::Error) -> DiagnosticSet {
    cli_error("CLI-OUTPUT", format!("failed to write output: {err}"))
}
