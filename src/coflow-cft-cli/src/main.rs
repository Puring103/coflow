use clap::{Parser, Subcommand};
use coflow_cft::{CftContainer, CftDiagnostic, CftLabel, ModuleId};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Read, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            let _ = writeln!(io::stderr().lock(), "{message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    match Cli::parse().command {
        Command::Diagnostics(args) => write_diagnostics(args),
    }
}

#[derive(Debug, Parser)]
#[command(name = "coflow-cft")]
#[command(about = "Command-line tools for Coflow CFT schemas.")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Emit compiler diagnostics as JSON.
    Diagnostics(DiagnosticsArgs),
}

#[derive(Debug, Parser)]
struct DiagnosticsArgs {
    /// Treat stdin as the source for this path. The path must also appear in paths.
    #[arg(long = "stdin-path")]
    stdin_path: Option<String>,

    /// CFT module paths to check.
    #[arg(value_name = "PATH")]
    paths: Vec<String>,
}

fn write_diagnostics(args: DiagnosticsArgs) -> Result<(), String> {
    let output = collect_diagnostics(args)?;
    serde_json::to_writer(io::stdout().lock(), &output)
        .map_err(|err| format!("failed to write diagnostics JSON: {err}"))?;
    println!();
    Ok(())
}

fn collect_diagnostics(args: DiagnosticsArgs) -> Result<DiagnosticsOutput, String> {
    if args.paths.is_empty() {
        return Ok(DiagnosticsOutput::default());
    }

    let mut stdin_source = String::new();
    if args.stdin_path.is_some() {
        io::stdin()
            .read_to_string(&mut stdin_source)
            .map_err(|err| format!("failed to read stdin: {err}"))?;
    }

    let mut sources = BTreeMap::new();
    let mut container = CftContainer::new();
    let mut diagnostics = Vec::new();

    for path in args.paths {
        let source = if args.stdin_path.as_deref() == Some(path.as_str()) {
            stdin_source.clone()
        } else {
            fs::read_to_string(&path).map_err(|err| format!("failed to read `{path}`: {err}"))?
        };
        sources.insert(path.clone(), source.clone());
        if let Err(errors) = container.add_module(ModuleId::new(path), source) {
            diagnostics.extend(errors.diagnostics);
        }
    }

    if diagnostics.is_empty() {
        if let Err(errors) = container.compile() {
            diagnostics.extend(errors.diagnostics);
        }
    }

    Ok(DiagnosticsOutput {
        diagnostics: dedupe_diagnostics(diagnostics)
            .iter()
            .map(|diagnostic| DiagnosticJson::from_diagnostic(diagnostic, &sources))
            .collect(),
    })
}

fn dedupe_diagnostics(diagnostics: Vec<CftDiagnostic>) -> Vec<CftDiagnostic> {
    let mut keys = BTreeSet::new();
    let mut out = Vec::new();
    for diagnostic in diagnostics {
        if keys.insert(diagnostic_key(&diagnostic)) {
            out.push(diagnostic);
        }
    }
    out
}

fn diagnostic_key(diagnostic: &CftDiagnostic) -> String {
    let mut key = format!(
        "{}\n{}\n{}\n",
        diagnostic.code.as_str(),
        diagnostic.stage,
        diagnostic.message
    );
    if let Some(primary) = &diagnostic.primary {
        push_label_key(&mut key, primary);
    }
    for related in &diagnostic.related {
        push_label_key(&mut key, related);
    }
    key
}

fn push_label_key(key: &mut String, label: &CftLabel) {
    key.push_str(label.module.as_str());
    key.push(':');
    key.push_str(&label.span.start.to_string());
    key.push(':');
    key.push_str(&label.span.end.to_string());
    key.push(':');
    if let Some(message) = &label.message {
        key.push_str(message);
    }
    key.push('\n');
}

#[derive(Debug, Default, Serialize)]
struct DiagnosticsOutput {
    diagnostics: Vec<DiagnosticJson>,
}

#[derive(Debug, Serialize)]
struct DiagnosticJson {
    code: String,
    stage: String,
    severity: String,
    message: String,
    path: String,
    #[serde(rename = "startLine")]
    start_line: usize,
    #[serde(rename = "startCharacter")]
    start_character: usize,
    #[serde(rename = "endLine")]
    end_line: usize,
    #[serde(rename = "endCharacter")]
    end_character: usize,
    related: Vec<RelatedJson>,
}

impl DiagnosticJson {
    fn from_diagnostic(diagnostic: &CftDiagnostic, sources: &BTreeMap<String, String>) -> Self {
        let fallback = CftLabel {
            module: ModuleId::new(""),
            span: Default::default(),
            message: None,
        };
        let primary = diagnostic.primary.as_ref().unwrap_or(&fallback);
        let range = label_range(primary, sources);
        Self {
            code: diagnostic.code.as_str().to_string(),
            stage: diagnostic.stage.to_string(),
            severity: "error".to_string(),
            message: diagnostic.message.clone(),
            path: primary.module.as_str().to_string(),
            start_line: range.start.line,
            start_character: range.start.character,
            end_line: range.end.line,
            end_character: range.end.character,
            related: diagnostic
                .related
                .iter()
                .map(|label| RelatedJson::from_label(label, sources))
                .collect(),
        }
    }
}

#[derive(Debug, Serialize)]
struct RelatedJson {
    path: String,
    #[serde(rename = "startLine")]
    start_line: usize,
    #[serde(rename = "startCharacter")]
    start_character: usize,
    #[serde(rename = "endLine")]
    end_line: usize,
    #[serde(rename = "endCharacter")]
    end_character: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<String>,
}

impl RelatedJson {
    fn from_label(label: &CftLabel, sources: &BTreeMap<String, String>) -> Self {
        let range = label_range(label, sources);
        Self {
            path: label.module.as_str().to_string(),
            start_line: range.start.line,
            start_character: range.start.character,
            end_line: range.end.line,
            end_character: range.end.character,
            label: label.message.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Range {
    start: Position,
    end: Position,
}

fn label_range(label: &CftLabel, sources: &BTreeMap<String, String>) -> Range {
    let source = sources
        .get(label.module.as_str())
        .map_or("", String::as_str);
    Range {
        start: byte_position(source, label.span.start),
        end: byte_position(source, label.span.end.max(label.span.start + 1)),
    }
}

#[derive(Debug, Clone, Copy)]
struct Position {
    line: usize,
    character: usize,
}

fn byte_position(source: &str, byte_offset: usize) -> Position {
    let target = byte_offset.min(source.len());
    let mut line = 0;
    let mut character = 0;

    for (byte_index, ch) in source.char_indices() {
        if byte_index >= target {
            break;
        }
        if ch == '\n' {
            line += 1;
            character = 0;
        } else {
            character += ch.len_utf16();
        }
    }

    Position { line, character }
}
