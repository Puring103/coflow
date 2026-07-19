use crate::diagnostics::{cli_error, cli_file_error};
use coflow_api::{DiagnosticSet, FlatDiagnostic};
use serde::Serialize;
use std::io::{self, Read, Write};
use std::path::Path;

#[derive(Debug)]
pub(crate) struct WriteFileOptions {
    pub file: String,
    pub input: WriteInput,
    pub mode: WriteMode,
    pub check: WriteCheck,
    pub output: WriteOutput,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum WriteInput {
    Stdin,
    Missing,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum WriteMode {
    Write,
    DryRun,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum WriteCheck {
    Run,
    Skip,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum WriteOutput {
    Json,
    Human,
}

#[derive(Debug, Serialize)]
pub(crate) struct WriteFileReport {
    pub file: String,
    pub written: bool,
    pub dry_run: bool,
    pub changed: bool,
    pub check_ok: Option<bool>,
    pub diagnostics: Vec<FlatDiagnostic>,
}

pub(crate) fn read_source(path: &Path) -> Result<String, DiagnosticSet> {
    std::fs::read_to_string(path).map_err(|err| {
        cli_file_error(
            path,
            "CLI-FILE-READ",
            format!("failed to read `{}`: {err}", path.display()),
        )
    })
}

pub(crate) fn read_stdin_source() -> Result<String, DiagnosticSet> {
    let mut source = String::new();
    io::stdin()
        .read_to_string(&mut source)
        .map_err(|err| cli_error("CLI-STDIN", format!("failed to read stdin: {err}")))?;
    Ok(source)
}

pub(crate) fn write_source(path: &Path, source: &str) -> Result<(), DiagnosticSet> {
    std::fs::write(path, source).map_err(|err| {
        cli_file_error(
            path,
            "CLI-FILE-WRITE",
            format!("failed to write `{}`: {err}", path.display()),
        )
    })
}

pub(crate) fn write_json(value: &impl Serialize) -> Result<(), DiagnosticSet> {
    serde_json::to_writer(io::stdout().lock(), value)
        .map_err(|err| cli_error("CLI-OUTPUT", format!("failed to write JSON: {err}")))?;
    println!();
    Ok(())
}

pub(crate) fn write_report_human(report: &WriteFileReport) -> Result<(), DiagnosticSet> {
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
    for diagnostic in &report.diagnostics {
        writeln!(
            stdout,
            "[{}] [{}] {}",
            diagnostic.code, diagnostic.stage, diagnostic.message
        )
        .map_err(output_error)?;
    }
    Ok(())
}

fn output_error(error: io::Error) -> DiagnosticSet {
    cli_error("CLI-OUTPUT", format!("failed to write output: {error}"))
}
