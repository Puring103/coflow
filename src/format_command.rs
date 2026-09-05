use crate::diagnostics::cli_file_error;
use atomicwrites::{AllowOverwrite, AtomicFile};
use coflow_format::{format_cfd, format_cft};
use coflow_runtime::{
    discover_directory_files, path_is_same_or_descendant, path_to_slash, DiagnosticSet, Project,
};
use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Language {
    Cft,
    Cfd,
}

#[derive(Debug)]
struct FormatTarget {
    path: PathBuf,
    language: Language,
}

/// Formats every configured CFT and CFD source in a project.
///
/// # Errors
///
/// Returns diagnostics when the project cannot be opened, configured sources
/// cannot be discovered or read, or a formatted source cannot be written.
pub(crate) fn run(config_or_dir: Option<&Path>, check: bool) -> Result<bool, DiagnosticSet> {
    let project = Project::open_schema_only(config_or_dir)?;
    let schema_diagnostics = project.schema_diagnostic_set();
    if !schema_diagnostics.is_empty() {
        return Err(schema_diagnostics);
    }
    let data_diagnostics = project.data_diagnostic_set();
    if !data_diagnostics.is_empty() {
        return Err(data_diagnostics);
    }

    let targets = discover_targets(&project)?;
    let mut changed = Vec::new();
    for target in &targets {
        let source = std::fs::read_to_string(&target.path).map_err(|error| {
            cli_file_error(
                &target.path,
                "FORMAT-READ",
                format!("failed to read `{}`: {error}", target.path.display()),
            )
        })?;
        let formatted = match target.language {
            Language::Cft => format_cft(&source),
            Language::Cfd => format_cfd(&source),
        };
        if source == formatted {
            continue;
        }
        changed.push(display_path(&project, &target.path));
        if !check {
            write_atomic(&target.path, &formatted)?;
        }
    }

    write_report(check, targets.len(), &changed)?;
    Ok(changed.is_empty() || !check)
}

fn discover_targets(project: &Project) -> Result<Vec<FormatTarget>, DiagnosticSet> {
    let mut targets = BTreeMap::<PathBuf, FormatTarget>::new();
    for file in project.schema_files()? {
        targets.insert(
            file.canonical_path,
            FormatTarget {
                path: file.path,
                language: Language::Cft,
            },
        );
    }

    let managed_dimension_dirs = project
        .config()
        .dimensions
        .values()
        .filter_map(|dimension| dimension.out_dir.as_ref())
        .map(|path| project.resolve_path(path))
        .collect::<Vec<_>>();
    for source in project.data_paths() {
        let path = project.resolve_path(source.path());
        let files = if path.is_dir() {
            discover_directory_files(&path).map_err(|error| {
                cli_file_error(
                    error.path(),
                    "FORMAT-DISCOVERY",
                    error.to_string(),
                )
            })?
        } else {
            vec![path]
        };
        for path in files {
            if managed_dimension_dirs
                .iter()
                .any(|directory| path_is_same_or_descendant(&path, directory))
            {
                continue;
            }
            if language_for_path(&path) != Some(Language::Cfd) {
                continue;
            }
            let canonical_path = std::fs::canonicalize(&path).map_err(|error| {
                cli_file_error(
                    &path,
                    "FORMAT-PATH",
                    format!("failed to resolve `{}`: {error}", path.display()),
                )
            })?;
            targets.entry(canonical_path).or_insert(FormatTarget {
                path,
                language: Language::Cfd,
            });
        }
    }

    let mut targets = targets.into_values().collect::<Vec<_>>();
    targets.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(targets)
}

fn language_for_path(path: &Path) -> Option<Language> {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("cft") => Some(Language::Cft),
        Some("cfd") => Some(Language::Cfd),
        _ => None,
    }
}

fn write_atomic(path: &Path, source: &str) -> Result<(), DiagnosticSet> {
    AtomicFile::new(path, AllowOverwrite)
        .write(|file| file.write_all(source.as_bytes()))
        .map_err(|error| {
            cli_file_error(
                path,
                "FORMAT-WRITE",
                format!("failed to write `{}`: {error}", path.display()),
            )
        })
}

fn display_path(project: &Project, path: &Path) -> String {
    path.strip_prefix(project.root_dir())
        .map_or_else(|_| path_to_slash(path), path_to_slash)
}

fn write_report(check: bool, total: usize, changed: &[String]) -> Result<(), DiagnosticSet> {
    let mut stdout = io::stdout().lock();
    if changed.is_empty() {
        let action = if check { "Checked" } else { "Formatted" };
        writeln!(stdout, "{action} {total} file(s); no changes needed.")
            .map_err(output_error)?;
        return Ok(());
    }
    let action = if check { "Would reformat" } else { "Formatted" };
    for path in changed {
        writeln!(stdout, "{action} {path}").map_err(output_error)?;
    }
    writeln!(
        stdout,
        "{} {} of {total} file(s).",
        if check { "Found" } else { "Updated" },
        changed.len()
    )
    .map_err(output_error)
}

fn output_error(error: io::Error) -> DiagnosticSet {
    crate::diagnostics::cli_error("CLI-OUTPUT", format!("failed to write output: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn format_project_updates_only_configured_sources() {
        let dir = tempdir().expect("temp dir");
        fs::create_dir_all(dir.path().join("schema")).expect("schema dir");
        fs::create_dir_all(dir.path().join("data")).expect("data dir");
        fs::create_dir_all(dir.path().join("data/dimensions/language"))
            .expect("dimension dir");
        fs::write(
            dir.path().join("coflow.yaml"),
            "schema: schema/\ndata: data/\ndimensions:\n  language:\n    variants: [en]\n    out_dir: data/dimensions/language\ncodegen:\n  - language: csharp\n    dir: generated/\n",
        )
        .expect("config");
        fs::write(dir.path().join("schema/main.cft"), "type Item{name:string;}")
            .expect("schema");
        fs::write(dir.path().join("data/items.cfd"), "sword:Item{name:\"Sword\",}")
            .expect("data");
        fs::write(dir.path().join("ignored.cfd"), "ignored:Item{name:\"Ignored\",}")
            .expect("ignored data");
        let dimension = dir.path().join("data/dimensions/language/Item_name.cfd");
        fs::write(&dimension, "ignored:Item{name:\"Generated\",}").expect("dimension data");

        assert!(run(Some(dir.path()), false).expect("format project"));
        assert_eq!(
            fs::read_to_string(dir.path().join("schema/main.cft")).expect("read schema"),
            "type Item {\n  name: string;\n}\n"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("data/items.cfd")).expect("read data"),
            "sword: Item {\n  name: \"Sword\",\n}\n"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("ignored.cfd")).expect("read ignored"),
            "ignored:Item{name:\"Ignored\",}"
        );
        assert_eq!(
            fs::read_to_string(dimension).expect("read dimension"),
            "ignored:Item{name:\"Generated\",}"
        );
    }

    #[test]
    fn check_reports_changes_without_writing() {
        let dir = tempdir().expect("temp dir");
        fs::write(
            dir.path().join("coflow.yaml"),
            "schema: schema.cft\ndata: []\ncodegen:\n  - language: csharp\n    dir: generated/\n",
        )
        .expect("config");
        let schema = dir.path().join("schema.cft");
        fs::write(&schema, "type Item{name:string;}").expect("schema");

        assert!(!run(Some(dir.path()), true).expect("check format"));
        assert_eq!(
            fs::read_to_string(schema).expect("read schema"),
            "type Item{name:string;}"
        );
    }
}
