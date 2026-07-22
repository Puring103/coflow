use crate::diagnostics::{cli_error, cli_file_error};
use crate::write_file::{
    read_source, read_stdin_source, write_json, write_report_human, write_source,
};
use coflow_api::{DiagnosticSet, FlatDiagnostic};
use coflow_project::{path_to_slash, Project};
use coflow_runtime::{
    inspect_schema, schema_files, ProjectRuntime, Runtime, SchemaFilesReport, SchemaInspectReport,
    SchemaTextOverride, SchemaTypeRefInfo,
};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::write_file::WriteFileReport as SchemaWriteFileReport;
pub(crate) use crate::write_file::{
    WriteCheck as SchemaWriteCheck, WriteFileOptions as SchemaWriteFileOptions,
    WriteMode as SchemaWriteMode, WriteOutput as SchemaWriteOutput,
};

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
    let session = Runtime::open_schema_session(project)?;
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
    let session = Runtime::open_schema_session(project)?;
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
    let current = read_source(&target.absolute_path)?;
    let source = read_stdin_source()?;
    let changed = current != source;

    let diagnostics = if matches!(options.check, SchemaWriteCheck::Run) {
        check_schema_source(&project, &target, &source)?
    } else {
        Vec::new()
    };
    let dry_run = matches!(options.mode, SchemaWriteMode::DryRun);
    if !dry_run {
        write_source(&target.absolute_path, &source)?;
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
        SchemaWriteOutput::Human => write_report_human(&report)?,
    }
    Ok(ok)
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
    let mut runtime = ProjectRuntime::new(project.clone());
    let refresh = runtime.refresh_with_overrides(&[SchemaTextOverride {
        requested_module: Some(target.module_id.clone()),
        normalized_path: target.canonical_path.clone(),
        source: source.to_string(),
    }]);
    if let Some(attempt) = runtime.latest_attempt() {
        diagnostics.extend(attempt.diagnostics().clone().into_set());
    } else {
        refresh?;
    }
    Ok(diagnostics.flat_diagnostics())
}

fn write_schema_inspect_human(report: &SchemaInspectReport) -> Result<(), DiagnosticSet> {
    let mut stdout = io::stdout().lock();
    for ty in &report.types {
        writeln!(stdout, "type {}", ty.name).map_err(|err| output_error(&err))?;
        for field in &ty.fields {
            writeln!(
                stdout,
                "  {}: {}",
                field.name,
                display_value_type(&field.ty)
            )
            .map_err(|err| output_error(&err))?;
        }
    }
    for schema_enum in &report.enums {
        writeln!(stdout, "enum {}", schema_enum.name).map_err(|err| output_error(&err))?;
        for variant in &schema_enum.variants {
            writeln!(stdout, "  {} = {}", variant.name, variant.value)
                .map_err(|err| output_error(&err))?;
        }
    }
    for schema_const in &report.consts {
        writeln!(stdout, "const {}", schema_const.name).map_err(|err| output_error(&err))?;
    }
    write_flat_diagnostics(&mut stdout, &report.diagnostics)
}

fn display_value_type(ty: &SchemaTypeRefInfo) -> String {
    match ty {
        SchemaTypeRefInfo::Int => "int".to_string(),
        SchemaTypeRefInfo::Float => "float".to_string(),
        SchemaTypeRefInfo::Bool => "bool".to_string(),
        SchemaTypeRefInfo::String => "string".to_string(),
        SchemaTypeRefInfo::Named { name, .. } => name.clone(),
        SchemaTypeRefInfo::Ref { target } => format!("&{target}"),
        SchemaTypeRefInfo::Array { item } => format!("{}[]", display_value_type(item)),
        SchemaTypeRefInfo::Dict { key, value } => {
            format!(
                "dict<{}, {}>",
                display_value_type(key),
                display_value_type(value)
            )
        }
        SchemaTypeRefInfo::Nullable { inner } => format!("{}?", display_value_type(inner)),
    }
}

fn write_schema_files_human(report: &SchemaFilesReport) -> Result<(), DiagnosticSet> {
    let mut stdout = io::stdout().lock();
    for file in &report.files {
        writeln!(stdout, "{}", file.module).map_err(|err| output_error(&err))?;
        writeln!(stdout, "{}", file.source).map_err(|err| output_error(&err))?;
    }
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
        .map_err(|err| output_error(&err))?;
    }
    Ok(())
}

fn output_error(err: &io::Error) -> DiagnosticSet {
    cli_error("CLI-OUTPUT", format!("failed to write output: {err}"))
}
