use std::fs;

mod common;
use common::fixture_files;

#[test]
fn valid_fixtures_parse_successfully() {
    for path in fixture_files("tests/fixtures/coflow/valid") {
        let source = fs::read_to_string(&path).expect("fixture should be readable");
        let output = coflow::parser::parse_module(&source);
        assert!(
            output.errors.is_empty(),
            "expected no parse errors in {}\nerrors: {:#?}",
            path.display(),
            output.errors
        );
        assert!(
            output.module.is_some(),
            "expected parser module for {}",
            path.display()
        );
    }
}

#[test]
fn invalid_parse_fixtures_report_errors() {
    for path in fixture_files("tests/fixtures/coflow/invalid/parse") {
        let source = fs::read_to_string(&path).expect("fixture should be readable");
        let output = coflow::parser::parse_module(&source);
        assert!(
            !output.errors.is_empty(),
            "expected parse errors in {}",
            path.display()
        );
    }
}
