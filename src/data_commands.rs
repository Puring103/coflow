use coflow_api::{FlatDiagnostic, ProviderRegistry};
use coflow_project::Project;
use coflow_runtime::{
    build_project_schema_session, build_project_session, data_get, data_list, data_sources,
    DataGetQuery, DataGetReport, DataListQuery, DataPatchReport, DataPatchRequest, ProjectSession,
    RecordCoordinate,
};
use output::{
    flat_diagnostics, write_data_write_file_human, write_file_report_human, write_get_human,
    write_json, write_list_human, write_patch_human, write_sources_human,
};
use serde::Serialize;
use std::path::{Path, PathBuf};

mod files;
mod lark;
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

#[derive(Debug)]
pub struct DataWriteFileOptions {
    pub file: String,
    pub input: DataWriteInput,
    pub mode: DataWriteMode,
    pub check: DataWriteCheck,
    pub output: DataWriteOutput,
}

#[derive(Debug, Clone, Copy)]
pub enum DataWriteInput {
    Stdin,
    Missing,
}

#[derive(Debug, Clone, Copy)]
pub enum DataWriteMode {
    Write,
    DryRun,
}

#[derive(Debug, Clone, Copy)]
pub enum DataWriteCheck {
    Run,
    Skip,
}

#[derive(Debug, Clone, Copy)]
pub enum DataWriteOutput {
    Json,
    Human,
}

#[derive(Debug, Serialize)]
pub struct DataWriteFileReport {
    pub file: String,
    pub written: bool,
    pub dry_run: bool,
    pub changed: bool,
    pub check_ok: Option<bool>,
    pub diagnostics: Vec<FlatDiagnostic>,
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
            remaining_ops: Vec::new(),
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
    let registry = coflow_builtins::default_provider_registry().map_err(|err| err.to_string())?;
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
) -> Result<bool, String> {
    let session = open_schema_session(config_or_dir)?;
    let registry = coflow_builtins::default_provider_registry().map_err(|err| err.to_string())?;
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
) -> Result<bool, String> {
    let session = open_schema_session(config_or_dir)?;
    let registry = coflow_builtins::default_provider_registry().map_err(|err| err.to_string())?;
    let report = files::sync_header_report(&session, &registry, file, actual_type, provider, sheet);
    let ok = report.diagnostics.is_empty();
    if human {
        write_file_report_human(&report)?;
    } else {
        write_json(&report)?;
    }
    Ok(ok)
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
) -> Result<bool, String> {
    let report = write_file::run_write_file(config_or_dir, options)?;
    let ok = report.check_ok.unwrap_or(true);
    match options.output {
        DataWriteOutput::Json => write_json(&report)?,
        DataWriteOutput::Human => write_data_write_file_human(&report)?,
    }
    Ok(ok)
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
) -> Result<coflow_runtime::ProjectSchemaSession, String> {
    let project = Project::open_schema_only(config_or_dir)?;
    build_project_schema_session(project)
}
