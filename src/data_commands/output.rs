use super::DataWriteFileReport;
use coflow_api::{DiagnosticSet, FlatDiagnostic};
use coflow_runtime::{
    DataFileReport, DataGetReport, DataListReport, DataPatchReport, DataSourcesReport,
};
use serde::Serialize;
use std::io::{self, Write};

pub(super) fn write_json(value: &impl Serialize) -> Result<(), String> {
    serde_json::to_writer(io::stdout().lock(), value)
        .map_err(|err| format!("failed to write JSON: {err}"))?;
    println!();
    Ok(())
}

pub(super) fn write_sources_human(report: &DataSourcesReport) -> Result<(), String> {
    let mut stdout = io::stdout().lock();
    for source in &report.sources {
        writeln!(
            stdout,
            "{}\t{}\t{}",
            source.file,
            source.provider,
            source.types.join(",")
        )
        .map_err(|err| format!("failed to write output: {err}"))?;
    }
    write_flat_diagnostics(&mut stdout, &report.diagnostics)
}

pub(super) fn write_list_human(report: &DataListReport) -> Result<(), String> {
    let mut stdout = io::stdout().lock();
    for record in &report.records {
        writeln!(
            stdout,
            "{}.{}\t{}\t{}",
            record.record.actual_type, record.record.key, record.file, record.provider
        )
        .map_err(|err| format!("failed to write output: {err}"))?;
    }
    write_flat_diagnostics(&mut stdout, &report.diagnostics)
}

pub(super) fn write_get_human(report: &DataGetReport) -> Result<(), String> {
    let mut stdout = io::stdout().lock();
    for record in &report.records {
        writeln!(
            stdout,
            "{}.{}\t{}\t{}",
            record.record.actual_type, record.record.key, record.file, record.provider
        )
        .map_err(|err| format!("failed to write output: {err}"))?;
        for (name, value) in &record.fields {
            writeln!(stdout, "  {name}\t{value:?}")
                .map_err(|err| format!("failed to write output: {err}"))?;
        }
    }
    write_flat_diagnostics(&mut stdout, &report.diagnostics)
}

pub(super) fn write_patch_human(report: &DataPatchReport) -> Result<(), String> {
    let mut stdout = io::stdout().lock();
    writeln!(
        stdout,
        "write_ok={}\tcheck_ok={}\tapplied={}\tfailed={}",
        report.write_ok,
        report.check_ok,
        report.applied.len(),
        report.failed.len()
    )
    .map_err(|err| format!("failed to write output: {err}"))?;
    for applied in &report.applied {
        let record = applied.record.as_ref().map_or_else(String::new, |record| {
            format!("{}.{}", record.actual_type, record.key)
        });
        writeln!(
            stdout,
            "applied\t{}\t{}\t{}\t{}",
            applied.index,
            applied.op,
            record,
            applied.file.as_deref().unwrap_or("")
        )
        .map_err(|err| format!("failed to write output: {err}"))?;
    }
    for failed in &report.failed {
        writeln!(stdout, "failed\t{}\t{}", failed.index, failed.op)
            .map_err(|err| format!("failed to write output: {err}"))?;
        write_flat_diagnostics(&mut stdout, &failed.diagnostics)?;
    }
    write_flat_diagnostics(&mut stdout, &report.diagnostics)
}

pub(super) fn write_file_report_human(report: &DataFileReport) -> Result<(), String> {
    let mut stdout = io::stdout().lock();
    writeln!(
        stdout,
        "{}\t{}\t{}",
        report.provider,
        report.file,
        report.headers.join(",")
    )
    .map_err(|err| format!("failed to write output: {err}"))?;
    if !report.added.is_empty() {
        writeln!(stdout, "added\t{}", report.added.join(","))
            .map_err(|err| format!("failed to write output: {err}"))?;
    }
    if !report.removed.is_empty() {
        writeln!(stdout, "removed\t{}", report.removed.join(","))
            .map_err(|err| format!("failed to write output: {err}"))?;
    }
    write_flat_diagnostics(&mut stdout, &report.diagnostics)
}

pub(super) fn write_data_write_file_human(report: &DataWriteFileReport) -> Result<(), String> {
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
    .map_err(|err| format!("failed to write output: {err}"))?;
    write_flat_diagnostics(&mut stdout, &report.diagnostics)
}

fn write_flat_diagnostics(
    stdout: &mut impl Write,
    diagnostics: &[FlatDiagnostic],
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

pub(super) fn flat_diagnostics(diagnostics: &DiagnosticSet) -> Vec<FlatDiagnostic> {
    diagnostics
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.flat_view(None, None, None))
        .collect()
}

pub(super) fn file_error_report(diagnostics: &DiagnosticSet) -> DataFileReport {
    DataFileReport {
        file: String::new(),
        provider: String::new(),
        sheet: None,
        actual_type: None,
        headers: Vec::new(),
        added: Vec::new(),
        removed: Vec::new(),
        diagnostics: flat_diagnostics(diagnostics),
    }
}
