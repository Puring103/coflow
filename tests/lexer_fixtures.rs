use std::fs;
use std::path::{Path, PathBuf};

use coflow::lexer::lex;

#[test]
fn valid_fixtures_have_no_lex_errors() {
    for path in fixture_files("tests/fixtures/coflow/valid") {
        let source = fs::read_to_string(&path).expect("fixture should be readable");
        let output = lex(&source);
        assert!(
            output.errors.is_empty(),
            "expected no lex errors in {}\nerrors: {:#?}",
            path.display(),
            output.errors
        );
    }
}

#[test]
fn invalid_lex_fixtures_report_errors() {
    for path in fixture_files("tests/fixtures/coflow/invalid/lex") {
        let source = fs::read_to_string(&path).expect("fixture should be readable");
        let output = lex(&source);
        assert!(
            !output.errors.is_empty(),
            "expected lex errors in {}",
            path.display()
        );
    }
}

fn fixture_files(root: impl AsRef<Path>) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_fixture_files(root.as_ref(), &mut files);
    files.sort();
    files
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
