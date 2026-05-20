use std::fs;
use std::path::{Path, PathBuf};

use coflow::lexer::{LexError, Token};

pub fn fixture_files(root: impl AsRef<Path>) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_fixture_files(root.as_ref(), &mut files);
    files.sort();
    files
}

pub fn render_tokens(source: &str, tokens: &[Token]) -> String {
    tokens
        .iter()
        .map(|token| {
            format!(
                "{:?} {:?}",
                token.kind,
                &source[token.span.start..token.span.end]
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn render_errors(source: &str, errors: &[LexError]) -> String {
    errors
        .iter()
        .map(|error| {
            format!(
                "{:?} [{}..{}] {:?}",
                error.kind,
                error.span.start,
                error.span.end,
                &source[error.span.start..error.span.end]
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn collect_fixture_files(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("fixture directory should exist") {
        let entry = entry.expect("fixture directory entry should be readable");
        let path = entry.path();
        if path.is_dir() {
            collect_fixture_files(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "cf") {
            files.push(path);
        }
    }
}
