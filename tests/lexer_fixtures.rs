use std::fs;
use std::path::{Path, PathBuf};

use coflow::lexer::{lex, TokenKind};

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

#[test]
fn complex_nested_module_has_expected_token_sequence() {
    let source = fs::read_to_string("tests/fixtures/coflow/valid/100-complex-nested-module.cf")
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

fn assert_contains_token_window(actual: &[TokenKind], expected: &[TokenKind]) {
    assert!(
        actual
            .windows(expected.len())
            .any(|window| window == expected),
        "expected token window not found: {expected:?}"
    );
}
