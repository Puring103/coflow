use coflow_api::{DiagnosticSet, FlatDiagnostic, ProviderRegistry};
use coflow_engine::{
    build_project_schema_session, build_project_session, create_data_file, data_get, data_list,
    data_sources, sync_data_header, DataCreateFileOptions, DataFileReport, DataGetQuery,
    DataGetReport, DataListQuery, DataPatchReport, DataPatchRequest, DataSourcesReport,
    DataSyncHeaderOptions, ProjectSession, RecordCoordinate,
};
use coflow_project::Project;
use serde::Serialize;
use std::io::{self, Write};
use std::path::Path;

#[derive(Debug)]
pub struct DataGetOptions {
    pub config_or_dir: Option<std::path::PathBuf>,
    pub selector: Option<RecordCoordinate>,
    pub actual_type: Option<String>,
    pub file: Option<String>,
    pub keys: Vec<String>,
    pub limit: Option<usize>,
    pub offset: usize,
    pub all: bool,
    pub human: bool,
}

/// Lists resolved data sources and provider writer capabilities.
///
/// # Errors
///
/// Returns an error when the project cannot be opened, the default provider
/// registry cannot be built, the project session cannot be built, or output
/// cannot be written.
pub fn sources(config_or_dir: Option<&Path>, human: bool) -> Result<bool, String> {
    let (session, registry) = open_session(config_or_dir)?;
    let report = data_sources(&session, &registry);
    if human {
        write_sources_human(&report)?;
    } else {
        write_json(&report)?;
    }
    Ok(report.diagnostics.is_empty())
}

/// Lists records known to the project session.
///
/// # Errors
///
/// Returns an error when the project cannot be opened, the default provider
/// registry cannot be built, the project session cannot be built, or output
/// cannot be written.
pub fn list(
    config_or_dir: Option<&Path>,
    actual_type: Option<String>,
    file: Option<String>,
    limit: Option<usize>,
    offset: usize,
    human: bool,
) -> Result<bool, String> {
    let (session, _registry) = open_session(config_or_dir)?;
    let report = data_list(
        &session,
        &DataListQuery {
            actual_type,
            file,
            limit,
            offset,
        },
    );
    if human {
        write_list_human(&report)?;
    } else {
        write_json(&report)?;
    }
    Ok(report.diagnostics.is_empty())
}

/// Fetches complete records from the project data model.
///
/// # Errors
///
/// Returns an error when the project cannot be opened, the default provider
/// registry cannot be built, the project session cannot be built, or output
/// cannot be written. User-fixable lookup diagnostics are written as command
/// output and return `Ok(false)`.
pub fn get(options: DataGetOptions) -> Result<bool, String> {
    let (session, _registry) = open_session(options.config_or_dir.as_deref())?;
    let query = DataGetQuery {
        selector: options.selector,
        actual_type: options.actual_type,
        file: options.file,
        keys: options.keys,
        limit: options.limit,
        offset: options.offset,
        all: options.all,
    };
    match data_get(&session, &query) {
        Ok(report) => {
            let ok = report.diagnostics.is_empty();
            if options.human {
                write_get_human(&report)?;
            } else {
                write_json(&report)?;
            }
            Ok(ok)
        }
        Err(diagnostics) => {
            let report = DataGetReport {
                records: Vec::new(),
                diagnostics: flat_diagnostics(&diagnostics),
            };
            if options.human {
                write_get_human(&report)?;
            } else {
                write_json(&report)?;
            }
            Ok(false)
        }
    }
}

/// Applies a JSON patch request through provider writers.
///
/// # Errors
///
/// Returns an error when the patch file cannot be read or parsed, the project
/// cannot be opened, the default provider registry cannot be built, the
/// project session cannot be built, or output cannot be written. Engine patch
/// diagnostics are written as command output and return `Ok(false)`.
pub fn patch(config_or_dir: Option<&Path>, patch_path: &Path, human: bool) -> Result<bool, String> {
    let patch_text = std::fs::read_to_string(patch_path)
        .map_err(|err| format!("failed to read `{}`: {err}", patch_path.display()))?;
    let request: DataPatchRequest = serde_json::from_str(&patch_text)
        .map_err(|err| format!("failed to parse `{}`: {err}", patch_path.display()))?;
    let (mut session, registry) = open_session(config_or_dir)?;
    let report = match session.apply_data_patch(&registry, request) {
        Ok(report) => report,
        Err(diagnostics) => DataPatchReport {
            write_ok: false,
            check_ok: false,
            applied: Vec::new(),
            failed: Vec::new(),
            diagnostics: flat_diagnostics(&diagnostics),
        },
    };
    let ok = report.write_ok && !has_error_diagnostics(&report.diagnostics);
    if human {
        write_patch_human(&report)?;
    } else {
        write_json(&report)?;
    }
    Ok(ok)
}

/// Creates a local data file for a project.
///
/// # Errors
///
/// Returns an error when the project cannot be opened or output cannot be
/// written. User-fixable create diagnostics are written as command output and
/// return `Ok(false)`.
pub fn create_file(
    config_or_dir: Option<&Path>,
    file: String,
    actual_type: Option<String>,
    provider: Option<String>,
    sheet: Option<String>,
    human: bool,
) -> Result<bool, String> {
    let session = open_schema_session(config_or_dir)?;
    match create_data_file(
        &session,
        DataCreateFileOptions {
            file,
            actual_type,
            provider,
            sheet,
        },
    ) {
        Ok(report) => {
            if human {
                write_file_report_human(&report)?;
            } else {
                write_json(&report)?;
            }
            Ok(report.diagnostics.is_empty())
        }
        Err(diagnostics) => {
            let report = file_error_report(&diagnostics);
            if human {
                write_file_report_human(&report)?;
            } else {
                write_json(&report)?;
            }
            Ok(false)
        }
    }
}

/// Synchronizes a local data file's schema-controlled columns.
///
/// # Errors
///
/// Returns an error when the project cannot be opened or output cannot be
/// written. User-fixable sync diagnostics are written as command output and
/// return `Ok(false)`.
pub fn sync_header(
    config_or_dir: Option<&Path>,
    file: String,
    actual_type: String,
    provider: Option<String>,
    sheet: Option<String>,
    human: bool,
) -> Result<bool, String> {
    let session = open_schema_session(config_or_dir)?;
    match sync_data_header(
        &session,
        DataSyncHeaderOptions {
            file,
            actual_type,
            provider,
            sheet,
        },
    ) {
        Ok(report) => {
            if human {
                write_file_report_human(&report)?;
            } else {
                write_json(&report)?;
            }
            Ok(report.diagnostics.is_empty())
        }
        Err(diagnostics) => {
            let report = file_error_report(&diagnostics);
            if human {
                write_file_report_human(&report)?;
            } else {
                write_json(&report)?;
            }
            Ok(false)
        }
    }
}

fn has_error_diagnostics(diagnostics: &[FlatDiagnostic]) -> bool {
    diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == "error")
}

fn open_session(
    config_or_dir: Option<&Path>,
) -> Result<(ProjectSession, ProviderRegistry), String> {
    let project = Project::open_schema_only(config_or_dir)?;
    let registry = coflow_builtins::default_provider_registry().map_err(|err| err.to_string())?;
    let session = build_project_session(project, &registry)?;
    Ok((session, registry))
}

fn open_schema_session(
    config_or_dir: Option<&Path>,
) -> Result<coflow_engine::ProjectSchemaSession, String> {
    let project = Project::open_schema_only(config_or_dir)?;
    build_project_schema_session(project)
}

fn write_json(value: &impl Serialize) -> Result<(), String> {
    serde_json::to_writer(io::stdout().lock(), value)
        .map_err(|err| format!("failed to write JSON: {err}"))?;
    println!();
    Ok(())
}

fn write_sources_human(report: &DataSourcesReport) -> Result<(), String> {
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

fn write_list_human(report: &coflow_engine::DataListReport) -> Result<(), String> {
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

fn write_get_human(report: &DataGetReport) -> Result<(), String> {
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

fn write_patch_human(report: &DataPatchReport) -> Result<(), String> {
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

fn write_file_report_human(report: &DataFileReport) -> Result<(), String> {
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

fn flat_diagnostics(diagnostics: &DiagnosticSet) -> Vec<FlatDiagnostic> {
    diagnostics
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.flat_view(None, None, None))
        .collect()
}

fn file_error_report(diagnostics: &DiagnosticSet) -> DataFileReport {
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
