#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use coflow::lexer::{LexError, Token};
use coflow::parser::{parse_module, ParseError, ParseErrorKind};

use coflow::ast::Module;

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

pub fn parse_ok(source: &str) -> Module {
    let output = parse_module(source);
    assert_eq!(output.errors, [], "source should parse cleanly:\n{source}");
    output.module.expect("parser should return a module")
}

pub fn parse_error_kinds(source: &str) -> Vec<ParseErrorKind> {
    parse_module(source)
        .errors
        .into_iter()
        .map(|error| error.kind)
        .collect()
}

pub fn parse_errors(source: &str) -> Vec<ParseError> {
    parse_module(source).errors
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
