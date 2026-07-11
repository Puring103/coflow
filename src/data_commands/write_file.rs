use super::{
    open_session, DataWriteCheck, DataWriteFileOptions, DataWriteFileReport, DataWriteInput,
    DataWriteMode,
};
use crate::diagnostics::{cli_error, cli_file_error};
use coflow_api::{DiagnosticSet, FlatDiagnostic, SourceLocationSpec};
use coflow_project::Project;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

pub(super) fn run_write_file(
    config_or_dir: Option<&Path>,
    options: &DataWriteFileOptions,
) -> Result<DataWriteFileReport, DiagnosticSet> {
    let project = Project::open_schema_only(config_or_dir)?;
    let target = resolve_data_write_target(&project, &options.file)?;
    let current = std::fs::read_to_string(&target.absolute_path).map_err(|err| {
        cli_file_error(
            &target.absolute_path,
            "CLI-FILE-READ",
            format!("failed to read `{}`: {err}", target.absolute_path.display()),
        )
    })?;
    let source = match options.input {
        DataWriteInput::Stdin => read_stdin_source()?,
        DataWriteInput::Missing => {
            return Err(cli_error("CLI-ARG", "data write-file requires --stdin"));
        }
    };
    let changed = current != source;
    let dry_run = matches!(options.mode, DataWriteMode::DryRun);
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

    let should_check = matches!(options.check, DataWriteCheck::Run) && !dry_run;
    let diagnostics = if should_check {
        check_project_after_data_write(config_or_dir)?
    } else {
        Vec::new()
    };
    let check_ok = if should_check {
        Some(diagnostics.is_empty())
    } else {
        None
    };
    Ok(DataWriteFileReport {
        file: target.project_path,
        written: !dry_run,
        dry_run,
        changed,
        check_ok,
        diagnostics,
    })
}

#[derive(Debug)]
struct DataWriteTarget {
    absolute_path: PathBuf,
    project_path: String,
}

fn resolve_data_write_target(
    project: &Project,
    file: &str,
) -> Result<DataWriteTarget, DiagnosticSet> {
    let requested_path = Path::new(file);
    if requested_path
        .extension()
        .and_then(|extension| extension.to_str())
        != Some("cfd")
    {
        return Err(cli_error(
            "DATA-WRITE-TARGET",
            format!("`--file {file}` must name a configured .cfd data file"),
        ));
    }
    let absolute_path = project.resolve_path(requested_path);
    let canonical_path = std::fs::canonicalize(&absolute_path).map_err(|err| {
        cli_file_error(
            &absolute_path,
            "DATA-WRITE-TARGET",
            format!(
                "failed to resolve data file `{}`: {err}",
                absolute_path.display()
            ),
        )
    })?;
    if !is_within_configured_local_data_source(project, &canonical_path) {
        return Err(cli_error(
            "DATA-WRITE-TARGET",
            format!("`--file {file}` is not covered by a configured local CFD data source"),
        ));
    }
    let project_path = canonical_path.strip_prefix(&project.root_dir).map_or_else(
        |_| coflow_project::path_to_slash(&canonical_path),
        coflow_project::path_to_slash,
    );
    Ok(DataWriteTarget {
        absolute_path,
        project_path,
    })
}

fn is_within_configured_local_data_source(project: &Project, canonical_path: &Path) -> bool {
    project.config.sources.iter().any(|source| {
        if source
            .source_type
            .as_deref()
            .is_some_and(|source_type| source_type != "cfd")
        {
            return false;
        }
        let SourceLocationSpec::Path(path) = source.location() else {
            return false;
        };
        let source_path = project.resolve_path(path);
        let Ok(source_canonical) = std::fs::canonicalize(source_path) else {
            return false;
        };
        if source_canonical.is_file() {
            canonical_path == source_canonical
        } else {
            canonical_path.starts_with(source_canonical)
        }
    })
}

fn read_stdin_source() -> Result<String, DiagnosticSet> {
    let mut source = String::new();
    io::stdin()
        .read_to_string(&mut source)
        .map_err(|err| cli_error("CLI-STDIN", format!("failed to read stdin: {err}")))?;
    Ok(source)
}

fn check_project_after_data_write(
    config_or_dir: Option<&Path>,
) -> Result<Vec<FlatDiagnostic>, DiagnosticSet> {
    let (session, _registry) = open_session(config_or_dir)?;
    Ok(session.queries().diagnostics().flat_diagnostics())
}
