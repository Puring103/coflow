use coflow::diagnostics::{diagnostic_json_from_set, DiagnosticJson};
use coflow_project::Project;
use serde::Serialize;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const DIAGNOSTIC_SEPARATOR: &str = "----------------------------------------";

#[derive(Debug, Default, Serialize)]
struct DiagnosticsOutput {
    diagnostics: Vec<DiagnosticJson>,
}

pub(crate) fn write_json_diagnostics(diagnostics: Vec<DiagnosticJson>) -> Result<(), String> {
    serde_json::to_writer(io::stdout().lock(), &DiagnosticsOutput { diagnostics })
        .map_err(|err| format!("failed to write diagnostics JSON: {err}"))?;
    println!();
    Ok(())
}

pub(crate) fn write_project_diagnostics(
    diagnostics: coflow_api::DiagnosticSet,
    json: bool,
    root_dir: &Path,
) -> Result<(), String> {
    let diagnostics = diagnostic_json_from_set(diagnostics);
    if json {
        write_json_diagnostics(diagnostics)
    } else {
        write_human_diagnostics(&diagnostics, Some(root_dir))
    }
}

fn write_human_diagnostics(
    diagnostics: &[DiagnosticJson],
    root_dir: Option<&Path>,
) -> Result<(), String> {
    let mut stderr = io::stderr().lock();
    for diagnostic in diagnostics {
        write_diagnostic_block(&mut stderr, diagnostic, root_dir)
            .map_err(|err| format!("failed to write diagnostics: {err}"))?;
    }
    Ok(())
}

fn write_diagnostic_block(
    stderr: &mut impl Write,
    diagnostic: &DiagnosticJson,
    root_dir: Option<&Path>,
) -> io::Result<()> {
    writeln!(stderr, "{DIAGNOSTIC_SEPARATOR}")?;
    writeln!(stderr, "[{}] [{}]", diagnostic.code, diagnostic.stage)?;
    if !diagnostic.path.is_empty() {
        writeln!(
            stderr,
            "{:<8}{}",
            "file",
            display_path(&diagnostic.path, root_dir)
        )?;
    }
    if let Some(sheet) = &diagnostic.sheet {
        writeln!(stderr, "{:<8}{sheet}", "sheet")?;
    }
    if let Some(cell) = &diagnostic.cell {
        writeln!(stderr, "{:<8}{cell}", "cell")?;
    } else {
        writeln!(stderr, "{:<8}{}", "line", diagnostic.start_line + 1)?;
        writeln!(stderr, "{:<8}{}", "column", diagnostic.start_character + 1)?;
    }
    let message = root_dir.map_or_else(
        || diagnostic.message.clone(),
        |root_dir| relativize_message_paths(&diagnostic.message, root_dir),
    );
    write_message_field(stderr, &message)
}

fn write_message_field(stderr: &mut impl Write, message: &str) -> io::Result<()> {
    writeln!(stderr, "message")?;
    for line in message.lines() {
        writeln!(stderr, "  {line}")?;
    }
    if message.is_empty() {
        writeln!(stderr, "  ")?;
    }
    Ok(())
}

pub(crate) fn display_path(path: &str, root_dir: Option<&Path>) -> String {
    let cleaned_path = strip_windows_extended_prefix(path);
    let path = PathBuf::from(&cleaned_path);
    if let Some(root_dir) = root_dir {
        let cleaned_root = strip_windows_extended_prefix(&root_dir.display().to_string());
        let root = PathBuf::from(cleaned_root);
        if let Ok(relative) = path.strip_prefix(&root) {
            let value = slash_path(relative);
            return if value.is_empty() {
                ".".to_string()
            } else {
                value
            };
        }
    }
    slash_path(&path)
}

pub(crate) fn project_path(project: &Project, path: &Path) -> String {
    display_path(&path.display().to_string(), Some(&project.root_dir))
}

pub(crate) fn relativize_message_paths(message: &str, root_dir: &Path) -> String {
    let mut out = String::with_capacity(message.len());
    let mut rest = message;
    while let Some(start) = rest.find('`') {
        out.push_str(&rest[..=start]);
        let after_start = &rest[start + 1..];
        let Some(end) = after_start.find('`') else {
            out.push_str(after_start);
            return out;
        };
        out.push_str(&display_path(&after_start[..end], Some(root_dir)));
        out.push('`');
        rest = &after_start[end + 1..];
    }
    out.push_str(rest);
    out
}

fn strip_windows_extended_prefix(path: &str) -> String {
    path.strip_prefix(r"\\?\").unwrap_or(path).to_string()
}

fn slash_path(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}
