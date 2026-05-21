use std::fs;

use crate::common::{fixture_files, render_ast, render_parse_errors};

#[test]
fn valid_fixtures_parse_successfully() {
    for path in fixture_files("tests/fixtures/coflow/parser/valid") {
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
    for path in fixture_files("tests/fixtures/coflow/parser/invalid") {
        let source = fs::read_to_string(&path).expect("fixture should be readable");
        let output = coflow::parser::parse_module(&source);
        assert!(
            !output.errors.is_empty(),
            "expected parse errors in {}",
            path.display()
        );
    }
}

#[test]
fn ast_expectation_fixtures_match() {
    let mut checked = 0;
    for path in fixture_files("tests/fixtures/coflow/parser/expect") {
        let expect_path = path.with_extension("ast.expect");
        if !expect_path.exists() {
            continue;
        }

        let source = fs::read_to_string(&path).expect("fixture should be readable");
        let output = coflow::parser::parse_module(&source);
        assert_eq!(
            output.errors,
            [],
            "fixture should parse cleanly: {}",
            path.display()
        );
        let module = output.module.expect("parser should return a module");

        let actual = render_ast(&module);
        let expected =
            fs::read_to_string(&expect_path).expect("ast expectation should be readable");
        assert_eq!(
            expected.replace("\r\n", "\n").trim_end(),
            actual.trim_end(),
            "ast expectation mismatch for {}",
            path.display()
        );
        checked += 1;
    }

    assert!(checked > 0, "expected at least one ast expectation fixture");
}

#[test]
fn parse_error_expectation_fixtures_match() {
    let mut checked = 0;
    for path in fixture_files("tests/fixtures/coflow/parser/expect") {
        let expect_path = path.with_extension("parse-errors.expect");
        if !expect_path.exists() {
            continue;
        }

        let source = fs::read_to_string(&path).expect("fixture should be readable");
        let output = coflow::parser::parse_module(&source);
        let actual = render_parse_errors(&source, &output.errors);
        let expected =
            fs::read_to_string(&expect_path).expect("parse error expectation should be readable");
        assert_eq!(
            expected.replace("\r\n", "\n").trim_end(),
            actual.trim_end(),
            "parse error expectation mismatch for {}",
            path.display()
        );
        checked += 1;
    }

    assert!(
        checked > 0,
        "expected at least one parse error expectation fixture"
    );
}
