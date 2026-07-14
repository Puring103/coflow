#![allow(clippy::expect_used, clippy::panic_in_result_fn)]

use coflow_cft::parser::{parse_module, parse_module_with_options};
use coflow_cft::{CftErrorCode, CftParseOptions, ModuleId, Span, StructuralLimits};

const fn options(max_depth: u64, max_nodes: u64) -> CftParseOptions {
    CftParseOptions {
        structural_limits: StructuralLimits::new(max_depth, max_nodes, 10_000),
    }
}

fn parse(source: &str, max_depth: u64, max_nodes: u64) -> Result<(), coflow_cft::CftDiagnostics> {
    parse_module_with_options(
        &ModuleId::from("main"),
        source,
        options(max_depth, max_nodes),
    )
    .map(|_| ())
}

fn structural_error(source: &str, max_depth: u64, max_nodes: u64) -> coflow_cft::CftDiagnostic {
    parse(source, max_depth, max_nodes)
        .expect_err("source should exceed parser budget")
        .diagnostics
        .into_iter()
        .find(|diagnostic| diagnostic.code == CftErrorCode::SyntaxStructureLimitExceeded)
        .expect("structural limit diagnostic")
}

#[test]
fn default_entry_matches_explicit_default_options() {
    let module = ModuleId::from("main");
    let source = "type Item { value: [int]; check { value.len() >= 0; } }";
    let implicit = parse_module(&module, source).expect("default parser entry");
    let explicit = parse_module_with_options(&module, source, CftParseOptions::default())
        .expect("explicit default parser entry");

    assert_eq!(format!("{implicit:?}"), format!("{explicit:?}"));
}

#[test]
fn type_ref_and_default_depth_accept_boundary_and_reject_next_wrapper() {
    let type_source = "type Item { value: [[[int]]]; }";
    parse(type_source, 4, 100).expect("type ref depth four fits");
    let type_error = structural_error(type_source, 3, 100);
    assert_eq!(
        type_error.message,
        "type ref exceeds structural depth limit 3 (observed 4)"
    );
    let outer_array = type_source.find('[').expect("outer array token");
    assert_eq!(
        type_error.primary.map(|label| label.span),
        Some(Span::new(outer_array, outer_array + 1))
    );

    let default_source = "type Item { value: int = [[[1]]]; }";
    parse(default_source, 4, 100).expect("default depth four fits");
    let default_error = structural_error(default_source, 3, 100);
    assert_eq!(
        default_error.message,
        "default value exceeds structural depth limit 3 (observed 4)"
    );
}

#[test]
fn loop_built_check_expression_depth_is_bounded_before_parent_is_created() {
    let binary = "type Item { check { 1 + 2 + 3 + 4 + 5 + 6; } }";
    parse(binary, 8, 100).expect("expression, statement, and block fit depth eight");
    let binary_error = structural_error(binary, 4, 100);
    assert_eq!(
        binary_error.message,
        "check AST exceeds structural depth limit 4 (observed 5)"
    );

    let postfix = "type Item { check { a.b.c.d.e; } }";
    parse(postfix, 7, 100).expect("postfix chain and wrappers fit depth seven");
    let postfix_error = structural_error(postfix, 4, 100);
    assert_eq!(
        postfix_error.message,
        "check AST exceeds structural depth limit 4 (observed 5)"
    );
}

#[test]
fn recursive_parentheses_and_statement_bodies_share_the_depth_policy() {
    let parentheses = "type Item { check { ((((true)))); } }";
    parse(parentheses, 4, 100).expect("four nested parentheses fit");
    let deeper_parentheses = "type Item { check { (((((true))))); } }";
    structural_error(deeper_parentheses, 4, 100);

    let statements = "type Item { check { when true { when true { when true { true; } } } } }";
    parse(statements, 6, 100).expect("nested statement tree fits depth six");
    structural_error(statements, 4, 100);
}

#[test]
fn flat_syntax_nodes_use_the_same_module_budget() {
    let source = "enum State { A, B, C, }";
    parse(source, 10, 4).expect("three variants and enum item fit four nodes");
    let error = structural_error(source, 10, 3);
    assert_eq!(
        error.message,
        "syntax AST exceeds structural nodes limit 3 (observed 4)"
    );
}

