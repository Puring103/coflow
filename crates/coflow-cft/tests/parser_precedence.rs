#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

mod common;
use common::*;

use coflow_cft::{
    CftSchemaBinOp, CftSchemaCheckExpr, CftSchemaCheckExprKind, CftSchemaCheckStmt, CftSchemaCmpOp,
};

#[test]
fn logical_and_or_share_one_left_associative_precedence_level() {
    let container = compile_one(
        r"
            type Rule {
                check { true || false && false; }
            }
        ",
    )
    .unwrap();

    let expr = first_check_expr(&container, "Rule");
    let (op, lhs, rhs) = expect_binop(expr);
    assert_eq!(op, CftSchemaBinOp::And);
    assert!(matches!(rhs.kind, CftSchemaCheckExprKind::Bool(false)));

    let (lhs_op, lhs_lhs, lhs_rhs) = expect_binop(lhs);
    assert_eq!(lhs_op, CftSchemaBinOp::Or);
    assert!(matches!(lhs_lhs.kind, CftSchemaCheckExprKind::Bool(true)));
    assert!(matches!(lhs_rhs.kind, CftSchemaCheckExprKind::Bool(false)));
}

#[test]
fn bitwise_or_xor_and_share_one_left_associative_precedence_level() {
    let container = compile_one(
        r"
            type Rule {
                check { 1 | 2 ^ 3 & 4 == 0; }
            }
        ",
    )
    .unwrap();

    let expr = first_check_expr(&container, "Rule");
    let CftSchemaCheckExprKind::CmpChain { first, rest } = &expr.kind else {
        panic!("expected comparison chain, got {expr:?}");
    };
    assert_eq!(rest.len(), 1);
    assert_eq!(rest[0].0, CftSchemaCmpOp::Eq);

    let (op, lhs, rhs) = expect_binop(first);
    assert_eq!(op, CftSchemaBinOp::BitAnd);
    assert!(matches!(rhs.kind, CftSchemaCheckExprKind::Int(4)));

    let (lhs_op, lhs_lhs, lhs_rhs) = expect_binop(lhs);
    assert_eq!(lhs_op, CftSchemaBinOp::BitXor);
    assert!(matches!(lhs_rhs.kind, CftSchemaCheckExprKind::Int(3)));

    let (inner_op, inner_lhs, inner_rhs) = expect_binop(lhs_lhs);
    assert_eq!(inner_op, CftSchemaBinOp::BitOr);
    assert!(matches!(inner_lhs.kind, CftSchemaCheckExprKind::Int(1)));
    assert!(matches!(inner_rhs.kind, CftSchemaCheckExprKind::Int(2)));
}

fn first_check_expr<'a>(container: &'a CftContainer, type_name: &str) -> &'a CftSchemaCheckExpr {
    let ty = container.resolve_type(type_name).expect("type");
    let check = ty.check.as_ref().expect("check block");
    let CftSchemaCheckStmt::Expr(expr) = &check.stmts[0] else {
        panic!("expected expression statement");
    };
    expr
}

fn expect_binop(
    expr: &CftSchemaCheckExpr,
) -> (CftSchemaBinOp, &CftSchemaCheckExpr, &CftSchemaCheckExpr) {
    let CftSchemaCheckExprKind::BinOp { op, lhs, rhs } = &expr.kind else {
        panic!("expected binary expression, got {expr:?}");
    };
    (*op, lhs, rhs)
}
