use crate::diagnostics::{cli_error, cli_file_error};
use crate::write_file::write_report_human;
use coflow_api::{DiagnosticSet, FlatDiagnostic, ProviderRegistry};
use coflow_project::Project;
use coflow_runtime::{
    data_get, data_list, data_sources, DataGetQuery, DataGetReport, DataListQuery,
    DataPatchRequest, ProjectSchemaSession, ReadOnlyProjectSession, RecordCoordinate, Runtime,
    WriteProjectSession,
};
use output::{
    file_error_report, write_file_report_human, write_get_human, write_json, write_list_human,
    write_patch_human, write_sources_human,
};
use std::io::Read;
use std::path::{Path, PathBuf};

mod files;
mod output;
mod write_file;

#[derive(Debug)]
pub struct DataGetOptions {
    pub config_or_dir: Option<PathBuf>,
    pub selector: Option<RecordCoordinate>,
    pub actual_type: Option<String>,
    pub file: Option<String>,
    pub keys: Vec<String>,
    pub limit: Option<usize>,
    pub offset: usize,
    pub all: bool,
    pub human: bool,
}

pub(crate) use crate::write_file::{
    WriteCheck as DataWriteCheck, WriteFileOptions as DataWriteFileOptions,
    WriteFileReport as DataWriteFileReport, WriteMode as DataWriteMode,
    WriteOutput as DataWriteOutput,
};

#[derive(Debug)]
pub struct DataPatchInput {
    pub json: Option<String>,
    pub file: Option<PathBuf>,
    pub stdin: bool,
}

/// Lists resolved data sources and provider writer capabilities.
///
/// # Errors
///
/// Returns an error when the project cannot be opened, the default provider
/// registry cannot be built, the project session cannot be built, or output
/// cannot be written.
pub fn sources(config_or_dir: Option<&Path>, human: bool) -> Result<bool, DiagnosticSet> {
    let (session, registry) = open_read_session(config_or_dir)?;
    let report = data_sources(session.queries(), &registry);
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
) -> Result<bool, DiagnosticSet> {
    let (session, _registry) = open_read_session(config_or_dir)?;
    let report = data_list(
        session.queries(),
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
pub fn get(options: DataGetOptions) -> Result<bool, DiagnosticSet> {
    let (session, _registry) = open_read_session(options.config_or_dir.as_deref())?;
    let query = DataGetQuery {
        selector: options.selector,
        actual_type: options.actual_type,
        file: options.file,
        keys: options.keys,
        limit: options.limit,
        offset: options.offset,
        all: options.all,
    };
    match data_get(session.queries(), &query) {
        Ok(report) => {
            let ok = !report.records.is_empty() || report.diagnostics.is_empty();
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
                diagnostics: diagnostics.flat_diagnostics(),
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
pub fn patch(
    config_or_dir: Option<&Path>,
    input: DataPatchInput,
    human: bool,
) -> Result<bool, DiagnosticSet> {
    let (patch_text, source_label) = match (input.json, input.file, input.stdin) {
        (Some(json), None, false) => (json, "--patch".to_string()),
        (None, Some(path), false) => {
            let text = std::fs::read_to_string(&path).map_err(|err| {
                cli_file_error(
                    &path,
                    "CLI-FILE-READ",
                    format!("failed to read `{}`: {err}", path.display()),
                )
            })?;
            (text, path.display().to_string())
        }
        (None, None, true) => (read_patch_stdin()?, "--stdin".to_string()),
        (None, None, false) => {
            return Err(DiagnosticSet::one(coflow_api::Diagnostic::error(
                "CLI-PATCH-INPUT",
                "CLI",
                "data patch requires --patch JSON, --patch-file PATCH_FILE, or --stdin",
            )));
        }
        _ => {
            return Err(DiagnosticSet::one(coflow_api::Diagnostic::error(
                "CLI-PATCH-INPUT",
                "CLI",
                "use only one of --patch, --patch-file, or --stdin",
            )));
        }
    };
    let request: DataPatchRequest = serde_json::from_str(&patch_text).map_err(|err| {
        DiagnosticSet::one(coflow_api::Diagnostic::error(
            "CLI-PATCH-PARSE",
            "CLI",
            format!("failed to parse patch request from {source_label}: {err}"),
        ))
    })?;
    let mut session = open_write_session(config_or_dir)?;
    let report = session.apply_data_patch(request);
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
) -> Result<bool, DiagnosticSet> {
    let (session, registry) = open_schema_session(config_or_dir)?;
    let report = files::create_file_report(&session, &registry, file, actual_type, provider, sheet);
    let ok = report.diagnostics.is_empty();
    if human {
        write_file_report_human(&report)?;
    } else {
        write_json(&report)?;
    }
    Ok(ok)
}

/// Creates a sheet/table in an existing table source.
///
/// # Errors
///
/// Returns an error when the project cannot be opened or output cannot be
/// written. User-fixable create diagnostics are written as command output and
/// return `Ok(false)`.
pub fn create_table(
    config_or_dir: Option<&Path>,
    source: String,
    actual_type: Option<String>,
    provider: Option<&str>,
    sheet: Option<String>,
    human: bool,
) -> Result<bool, DiagnosticSet> {
    let (session, registry) = open_schema_session(config_or_dir)?;
    let report =
        files::create_table_report(&session, &registry, source, actual_type, provider, sheet);
    let ok = report.diagnostics.is_empty();
    if human {
        write_file_report_human(&report)?;
    } else {
        write_json(&report)?;
    }
    Ok(ok)
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
) -> Result<bool, DiagnosticSet> {
    let project = Project::open_schema_only(config_or_dir)?;
    let registry = default_provider_registry()?;
    let runtime = Runtime::new(registry.clone());
    let session = runtime.open_read_only_session(project)?;
    let duplicate_header_diagnostics =
        duplicate_table_column_diagnostics(session.queries().diagnostics().as_set());
    if !duplicate_header_diagnostics.is_empty() {
        let report = file_error_report(&duplicate_header_diagnostics);
        if human {
            write_file_report_human(&report)?;
        } else {
            write_json(&report)?;
        }
        return Ok(false);
    }
    let session = session.into_schema_session();
    let report = files::sync_header_report(&session, &registry, file, actual_type, provider, sheet);
    let ok = report.diagnostics.is_empty();
    if human {
        write_file_report_human(&report)?;
    } else {
        write_json(&report)?;
    }
    Ok(ok)
}

fn read_patch_stdin() -> Result<String, DiagnosticSet> {
    let mut source = String::new();
    std::io::stdin()
        .read_to_string(&mut source)
        .map_err(|err| cli_error("CLI-STDIN", format!("failed to read stdin: {err}")))?;
    Ok(source)
}

fn duplicate_table_column_diagnostics(diagnostics: &DiagnosticSet) -> DiagnosticSet {
    DiagnosticSet {
        diagnostics: diagnostics
            .diagnostics
            .iter()
            .filter(|diagnostic| is_duplicate_table_column_code(&diagnostic.code))
            .cloned()
            .collect(),
    }
}

fn is_duplicate_table_column_code(code: &str) -> bool {
    matches!(
        code,
        "TABLE-COLUMN-DUPLICATE-FIELD"
            | "TABLE-COLUMN-DUPLICATE-HEADER"
            | "TABLE-COLUMN-DUPLICATE-KEY"
    )
}

/// Writes a configured local CFD data file from stdin.
///
/// # Errors
///
/// Returns an error when the project cannot be opened, the target is not a
/// configured local `.cfd` data file, stdin cannot be read, the file cannot be
/// written, full project validation cannot run, or output cannot be written.
pub fn write_file(
    config_or_dir: Option<&Path>,
    options: &DataWriteFileOptions,
) -> Result<bool, DiagnosticSet> {
    let report = write_file::run_write_file(config_or_dir, options)?;
    let ok = report.check_ok.unwrap_or(true);
    match options.output {
        DataWriteOutput::Json => write_json(&report)?,
        DataWriteOutput::Human => write_report_human(&report)?,
    }
    Ok(ok)
}

fn has_error_diagnostics(diagnostics: &[FlatDiagnostic]) -> bool {
    diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == "error")
}

fn open_read_session(
    config_or_dir: Option<&Path>,
) -> Result<(ReadOnlyProjectSession, ProviderRegistry), DiagnosticSet> {
    let project = Project::open_schema_only(config_or_dir)?;
    let registry = default_provider_registry()?;
    let runtime = Runtime::new(registry.clone());
    let session = runtime.open_read_only_session(project)?;
    Ok((session, registry))
}

fn open_write_session(config_or_dir: Option<&Path>) -> Result<WriteProjectSession, DiagnosticSet> {
    let project = Project::open_schema_only(config_or_dir)?;
    let registry = default_provider_registry()?;
    Runtime::new(registry).open_write_session(project)
}

fn open_schema_session(
    config_or_dir: Option<&Path>,
) -> Result<(ProjectSchemaSession, ProviderRegistry), DiagnosticSet> {
    let project = Project::open_schema_only(config_or_dir)?;
    let registry = default_provider_registry()?;
    let session = Runtime::open_schema_session(project)?;
    Ok((session, registry))
}

fn default_provider_registry() -> Result<ProviderRegistry, DiagnosticSet> {
    coflow_builtins::default_provider_registry()
        .map_err(|err| cli_error("PROVIDER-REGISTRY", err.to_string()))
}
