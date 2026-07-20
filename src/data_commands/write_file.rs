use super::{
    open_read_session, DataWriteCheck, DataWriteFileOptions, DataWriteFileReport, DataWriteInput,
    DataWriteMode,
};
use crate::diagnostics::{cli_error, cli_file_error};
use crate::write_file::{read_source, read_stdin_source, write_source};
use coflow_api::{DiagnosticSet, FlatDiagnostic};
use coflow_project::Project;
use std::path::{Path, PathBuf};

pub(super) fn run_write_file(
    config_or_dir: Option<&Path>,
    options: &DataWriteFileOptions,
) -> Result<DataWriteFileReport, DiagnosticSet> {
    let project = Project::open_schema_only(config_or_dir)?;
    let target = resolve_data_write_target(&project, &options.file)?;
    let current = read_source(&target.absolute_path)?;
    let source = match options.input {
        DataWriteInput::Stdin => read_stdin_source()?,
        DataWriteInput::Missing => {
            return Err(cli_error("CLI-ARG", "data write-file requires --stdin"));
        }
    };
    let changed = current != source;
    let dry_run = matches!(options.mode, DataWriteMode::DryRun);
    if !dry_run {
        write_source(&target.absolute_path, &source)?;
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
        let path = (source.location()).path();
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

fn check_project_after_data_write(
    config_or_dir: Option<&Path>,
) -> Result<Vec<FlatDiagnostic>, DiagnosticSet> {
    let (session, _registry) = open_read_session(config_or_dir)?;
    Ok(session.queries().diagnostics().flat_diagnostics())
}
