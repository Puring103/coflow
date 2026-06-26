use coflow_engine::{
    build_project_schema_session, inspect_schema, schema_files, SchemaFilesReport,
    SchemaInspectReport,
};
use coflow_project::Project;
use serde::Serialize;
use std::io::{self, Write};
use std::path::Path;

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
) -> Result<bool, String> {
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
pub fn files(config_or_dir: Option<&Path>, human: bool) -> Result<bool, String> {
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

fn write_json(value: &impl Serialize) -> Result<(), String> {
    serde_json::to_writer(io::stdout().lock(), value)
        .map_err(|err| format!("failed to write JSON: {err}"))?;
    println!();
    Ok(())
}

fn write_schema_inspect_human(report: &SchemaInspectReport) -> Result<(), String> {
    let mut stdout = io::stdout().lock();
    for ty in &report.types {
        writeln!(stdout, "type {}", ty.name)
            .map_err(|err| format!("failed to write output: {err}"))?;
        for annotation in &ty.annotations {
            writeln!(stdout, "  @{}", annotation.name)
                .map_err(|err| format!("failed to write output: {err}"))?;
        }
        for field in &ty.fields {
            writeln!(stdout, "  {}: {}", field.name, field.raw_type)
                .map_err(|err| format!("failed to write output: {err}"))?;
        }
    }
    for schema_enum in &report.enums {
        writeln!(stdout, "enum {}", schema_enum.name)
            .map_err(|err| format!("failed to write output: {err}"))?;
        for variant in &schema_enum.variants {
            writeln!(stdout, "  {} = {}", variant.name, variant.value)
                .map_err(|err| format!("failed to write output: {err}"))?;
        }
    }
    for schema_const in &report.consts {
        writeln!(stdout, "const {}", schema_const.name)
            .map_err(|err| format!("failed to write output: {err}"))?;
    }
    write_flat_diagnostics(&mut stdout, &report.diagnostics)
}

fn write_schema_files_human(report: &SchemaFilesReport) -> Result<(), String> {
    let mut stdout = io::stdout().lock();
    for file in &report.files {
        writeln!(stdout, "{}", file.module)
            .map_err(|err| format!("failed to write output: {err}"))?;
        writeln!(stdout, "{}", file.source)
            .map_err(|err| format!("failed to write output: {err}"))?;
    }
    write_flat_diagnostics(&mut stdout, &report.diagnostics)
}

fn write_flat_diagnostics(
    stdout: &mut impl Write,
    diagnostics: &[coflow_api::FlatDiagnostic],
) -> Result<(), String> {
    for diagnostic in diagnostics {
        writeln!(
            stdout,
            "[{}] [{}] {}",
            diagnostic.code, diagnostic.stage, diagnostic.message
        )
        .map_err(|err| format!("failed to write output: {err}"))?;
    }
    Ok(())
}
