use coflow::lexer::LexErrorKind;
use coflow::parser::{parse_module, ParseErrorKind};

use crate::common::{parse_error_kinds, parse_errors};

#[test]
fn lex_errors_are_reported_by_parser() {
    let errors = parse_error_kinds("a = ?");
    assert_eq!(
        errors,
        vec![ParseErrorKind::Lex(LexErrorKind::UnexpectedChar)]
    );
}

#[test]
fn parse_error_spans_point_to_the_problem_token() {
    let source = "value = 1 +";
    let errors = parse_errors(source);
    assert!(!errors.is_empty());
    assert_eq!(
        errors.first().map(|error| error.kind),
        Some(ParseErrorKind::ExpectedExpression)
    );
    assert!(errors[0].span.start <= source.len());
    assert!(errors[0].span.end <= source.len());
}

#[test]
fn malformed_input_does_not_return_successful_module_without_errors() {
    let output = parse_module("fn main( {");
    assert!(!output.errors.is_empty());
}

#[test]
fn unexpected_eof_is_reported_for_unclosed_block() {
    let errors = parse_error_kinds("fn main() { var x = 1");
    assert!(errors.contains(&ParseErrorKind::UnexpectedEof));
}
