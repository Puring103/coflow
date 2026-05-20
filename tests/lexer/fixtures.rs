use std::fs;

use coflow::lexer::{lex, TokenKind};

use crate::common::{fixture_files, render_lex_errors, render_tokens};

#[test]
fn valid_fixtures_have_no_lex_errors() {
    for path in fixture_files("tests/fixtures/coflow/lexer/valid") {
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
    for path in fixture_files("tests/fixtures/coflow/lexer/invalid") {
        let source = fs::read_to_string(&path).expect("fixture should be readable");
        let output = lex(&source);
        assert!(
            !output.errors.is_empty(),
            "expected lex errors in {}",
            path.display()
        );
    }
}

#[test]
fn token_expectation_fixtures_match() {
    let mut checked = 0;
    for path in fixture_files("tests/fixtures/coflow/lexer/expect") {
        let expect_path = path.with_extension("tokens.expect");
        if !expect_path.exists() {
            continue;
        }

        let source = fs::read_to_string(&path).expect("fixture should be readable");
        let output = lex(&source);
        assert_eq!(
            output.errors,
            [],
            "fixture should lex cleanly: {}",
            path.display()
        );

        let actual = render_tokens(&source, &output.tokens);
        let expected =
            fs::read_to_string(&expect_path).expect("token expectation should be readable");
        assert_eq!(
            expected.trim_end(),
            actual.trim_end(),
            "token expectation mismatch for {}",
            path.display()
        );
        checked += 1;
    }

    assert!(
        checked > 0,
        "expected at least one token expectation fixture"
    );
}

#[test]
fn lex_error_expectation_fixtures_match() {
    let mut checked = 0;
    for path in fixture_files("tests/fixtures/coflow/lexer/expect") {
        let expect_path = path.with_extension("lex-errors.expect");
        if !expect_path.exists() {
            continue;
        }

        let source = fs::read_to_string(&path).expect("fixture should be readable");
        let output = lex(&source);
        let actual = render_lex_errors(&source, &output.errors);
        let expected =
            fs::read_to_string(&expect_path).expect("lex error expectation should be readable");
        assert_eq!(
            expected.trim_end(),
            actual.trim_end(),
            "lex error expectation mismatch for {}",
            path.display()
        );
        checked += 1;
    }

    assert!(
        checked > 0,
        "expected at least one lex error expectation fixture"
    );
}

#[test]
fn complex_nested_module_has_expected_token_sequence() {
    let source =
        fs::read_to_string("tests/fixtures/coflow/lexer/valid/100-complex-nested-module.cf")
            .expect("fixture should be readable");
    let output = lex(&source);
    assert_eq!(output.errors, []);

    let actual = output
        .tokens
        .into_iter()
        .map(|token| token.kind)
        .collect::<Vec<_>>();

    assert_eq!(
        &actual[..13],
        &[
            TokenKind::Import,
            TokenKind::Ident,
            TokenKind::Import,
            TokenKind::Ident,
            TokenKind::As,
            TokenKind::Ident,
            TokenKind::Enum,
            TokenKind::Ident,
            TokenKind::LBrace,
            TokenKind::Ident,
            TokenKind::Ident,
            TokenKind::Ident,
            TokenKind::RBrace,
        ]
    );

    assert_contains_token_window(
        &actual,
        &[
            TokenKind::LBrace,
            TokenKind::Ident,
            TokenKind::Colon,
            TokenKind::LBracket,
            TokenKind::LBrace,
            TokenKind::Ident,
            TokenKind::Colon,
            TokenKind::FloatLiteral,
            TokenKind::Comma,
            TokenKind::Ident,
            TokenKind::Colon,
            TokenKind::StringLiteral,
            TokenKind::Comma,
            TokenKind::RBrace,
            TokenKind::Comma,
            TokenKind::LBrace,
            TokenKind::Ident,
            TokenKind::Colon,
            TokenKind::FloatLiteral,
            TokenKind::Comma,
            TokenKind::Ident,
            TokenKind::Colon,
            TokenKind::StringLiteral,
            TokenKind::Comma,
            TokenKind::RBrace,
            TokenKind::Comma,
            TokenKind::RBracket,
            TokenKind::Comma,
            TokenKind::RBrace,
        ],
    );

    assert_contains_token_window(
        &actual,
        &[
            TokenKind::Ident,
            TokenKind::QuestionDot,
            TokenKind::Ident,
            TokenKind::QuestionQuestion,
            TokenKind::StringLiteral,
            TokenKind::Ident,
            TokenKind::QuestionQuestionEq,
            TokenKind::StringLiteral,
        ],
    );

    assert_contains_token_window(
        &actual,
        &[
            TokenKind::Co,
            TokenKind::Fn,
            TokenKind::Ident,
            TokenKind::LParen,
            TokenKind::Ident,
            TokenKind::Comma,
            TokenKind::Ident,
            TokenKind::RParen,
            TokenKind::LBrace,
            TokenKind::Var,
            TokenKind::Ident,
            TokenKind::Eq,
            TokenKind::IntLiteral,
            TokenKind::While,
        ],
    );

    assert_contains_token_window(
        &actual,
        &[
            TokenKind::StringLiteral,
            TokenKind::Colon,
            TokenKind::RawStringLiteral,
            TokenKind::Comma,
            TokenKind::StringLiteral,
            TokenKind::Colon,
            TokenKind::MultilineStringLiteral,
        ],
    );

    assert_eq!(
        &actual[actual.len() - 16..],
        &[
            TokenKind::Ident,
            TokenKind::Colon,
            TokenKind::Ident,
            TokenKind::Dot,
            TokenKind::Ident,
            TokenKind::Comma,
            TokenKind::Ident,
            TokenKind::Colon,
            TokenKind::Ident,
            TokenKind::Dot,
            TokenKind::Ident,
            TokenKind::LtEq,
            TokenKind::IntLiteral,
            TokenKind::Comma,
            TokenKind::RBrace,
            TokenKind::RBrace,
        ]
    );
}

fn assert_contains_token_window(actual: &[TokenKind], expected: &[TokenKind]) {
    assert!(
        actual
            .windows(expected.len())
            .any(|window| window == expected),
        "expected token window not found: {expected:?}"
    );
}
