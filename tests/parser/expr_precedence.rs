use coflow::ast::{BinaryExpr, BinaryOp, Expr, Item, UnaryExpr, UnaryOp};
use coflow::parser::ParseErrorKind;

use crate::common::{parse_error_kinds, parse_ok};

fn parse_fn_body_expr(source: &str) -> Vec<coflow::ast::Stmt> {
    use coflow::ast::FnBody;
    let module = parse_ok(&format!("fn main() {{\n{source}\n}}"));
    let coflow::ast::Item::Function(func) = &module.items[0] else {
        panic!("expected function");
    };
    let FnBody::Block(block) = &func.body else {
        panic!("expected block body");
    };
    block.stmts.clone()
}

fn parse_value_expr(source: &str) -> Expr {
    let module = parse_ok(&format!("value = {source}"));
    let Item::Config(config) = &module.items[0] else {
        panic!("expected config");
    };
    config.value.clone()
}

#[test]
fn multiplication_binds_tighter_than_addition() {
    let expr = parse_value_expr("1 + 2 * 3");
    let Expr::Binary(BinaryExpr {
        op: BinaryOp::Add,
        rhs,
        ..
    }) = expr
    else {
        panic!("expected addition");
    };
    assert!(matches!(
        *rhs,
        Expr::Binary(BinaryExpr {
            op: BinaryOp::Mul,
            ..
        })
    ));
}

#[test]
fn parentheses_override_precedence() {
    let expr = parse_value_expr("(1 + 2) * 3");
    let Expr::Binary(BinaryExpr {
        op: BinaryOp::Mul,
        lhs,
        ..
    }) = expr
    else {
        panic!("expected multiplication");
    };
    assert!(matches!(
        *lhs,
        Expr::Binary(BinaryExpr {
            op: BinaryOp::Add,
            ..
        })
    ));
}

#[test]
fn postfix_binds_tighter_than_unary() {
    let expr = parse_value_expr("-a.b[0]");
    let Expr::Unary(UnaryExpr {
        op: UnaryOp::Neg,
        expr,
        ..
    }) = expr
    else {
        panic!("expected unary negation");
    };
    assert!(matches!(*expr, Expr::Index(_)));
}

#[test]
fn unary_binds_tighter_than_multiplication() {
    let expr = parse_value_expr("-a * b");
    let Expr::Binary(BinaryExpr {
        op: BinaryOp::Mul,
        lhs,
        ..
    }) = expr
    else {
        panic!("expected multiplication");
    };
    assert!(matches!(
        *lhs,
        Expr::Unary(UnaryExpr {
            op: UnaryOp::Neg,
            ..
        })
    ));
}

#[test]
fn not_binds_tighter_than_and() {
    let expr = parse_value_expr("not a and b");
    let Expr::Binary(BinaryExpr {
        op: BinaryOp::And,
        lhs,
        ..
    }) = expr
    else {
        panic!("expected and");
    };
    assert!(matches!(
        *lhs,
        Expr::Unary(UnaryExpr {
            op: UnaryOp::Not,
            ..
        })
    ));
}

#[test]
fn and_binds_tighter_than_or() {
    let expr = parse_value_expr("a or b and c");
    let Expr::Binary(BinaryExpr {
        op: BinaryOp::Or,
        rhs,
        ..
    }) = expr
    else {
        panic!("expected or");
    };
    assert!(matches!(
        *rhs,
        Expr::Binary(BinaryExpr {
            op: BinaryOp::And,
            ..
        })
    ));
}

#[test]
fn null_coalesce_is_right_associative() {
    let expr = parse_value_expr("a ?? b ?? c");
    let Expr::Binary(BinaryExpr {
        op: BinaryOp::NullCoalesce,
        rhs,
        ..
    }) = expr
    else {
        panic!("expected null coalesce");
    };
    assert!(matches!(
        *rhs,
        Expr::Binary(BinaryExpr {
            op: BinaryOp::NullCoalesce,
            ..
        })
    ));
}

#[test]
fn comparison_binds_weaker_than_addition() {
    let expr = parse_value_expr("a + b == c");
    let Expr::Binary(BinaryExpr {
        op: BinaryOp::Eq,
        lhs,
        ..
    }) = expr
    else {
        panic!("expected equality");
    };
    assert!(matches!(
        *lhs,
        Expr::Binary(BinaryExpr {
            op: BinaryOp::Add,
            ..
        })
    ));
}

#[test]
fn parses_in_and_optional_field_chain() {
    let membership = parse_value_expr("item in items and not item.dead");
    assert!(matches!(
        membership,
        Expr::Binary(BinaryExpr {
            op: BinaryOp::And,
            ..
        })
    ));

    let optional = parse_value_expr("player?.profile?.name ?? \"unknown\"");
    assert!(matches!(
        optional,
        Expr::Binary(BinaryExpr {
            op: BinaryOp::NullCoalesce,
            ..
        })
    ));
}

#[test]
fn parses_call_field_index_chain_with_trailing_arguments() {
    let expr = parse_value_expr("fx.spawn(skill.effects[0].id, target.position,)");
    let Expr::Call(call) = expr else {
        panic!("expected call");
    };
    assert_eq!(call.args.len(), 2);
    assert!(matches!(*call.callee, Expr::Field(_)));
}

#[test]
fn rejects_missing_rhs_expression() {
    let errors = parse_error_kinds("value = 1 +");
    assert!(errors.contains(&ParseErrorKind::ExpectedExpression));
}

#[test]
fn rejects_expression_starting_with_binary_operator() {
    let errors = parse_error_kinds("value = * 1");
    assert!(errors.contains(&ParseErrorKind::ExpectedExpression));
}

#[test]
fn rejects_split_optional_field_operator() {
    let errors = parse_error_kinds("value = a ? . b");
    assert!(errors.contains(&ParseErrorKind::Lex(
        coflow::lexer::LexErrorKind::UnexpectedChar
    )));
}

#[test]
fn rejects_field_access_without_name() {
    let errors = parse_error_kinds("value = a.");
    assert!(errors.contains(&ParseErrorKind::ExpectedIdentifier));
}

#[test]
fn rejects_empty_index_expression() {
    let errors = parse_error_kinds("value = a[]");
    assert!(errors.contains(&ParseErrorKind::ExpectedExpression));
}

#[test]
fn rejects_empty_call_argument_before_comma() {
    let errors = parse_error_kinds("value = call(, x)");
    assert!(errors.contains(&ParseErrorKind::ExpectedExpression));
}

#[test]
fn power_is_right_associative() {
    let expr = parse_value_expr("2 ** 3 ** 4");
    let Expr::Binary(BinaryExpr {
        op: BinaryOp::Pow,
        rhs,
        ..
    }) = expr
    else {
        panic!("expected power");
    };
    assert!(matches!(
        *rhs,
        Expr::Binary(BinaryExpr {
            op: BinaryOp::Pow,
            ..
        })
    ));
}

#[test]
fn power_binds_tighter_than_multiplication() {
    let expr = parse_value_expr("a * b ** c");
    let Expr::Binary(BinaryExpr {
        op: BinaryOp::Mul,
        rhs,
        ..
    }) = expr
    else {
        panic!("expected multiplication");
    };
    assert!(matches!(
        *rhs,
        Expr::Binary(BinaryExpr {
            op: BinaryOp::Pow,
            ..
        })
    ));
}

#[test]
fn int_div_has_same_precedence_as_mul() {
    let expr = parse_value_expr("a + b // c");
    let Expr::Binary(BinaryExpr {
        op: BinaryOp::Add,
        rhs,
        ..
    }) = expr
    else {
        panic!("expected addition");
    };
    assert!(matches!(
        *rhs,
        Expr::Binary(BinaryExpr {
            op: BinaryOp::IntDiv,
            ..
        })
    ));
}

#[test]
fn bitwise_and_binds_tighter_than_bitwise_or() {
    let expr = parse_value_expr("a | b & c");
    let Expr::Binary(BinaryExpr {
        op: BinaryOp::BitOr,
        rhs,
        ..
    }) = expr
    else {
        panic!("expected bitwise or");
    };
    assert!(matches!(
        *rhs,
        Expr::Binary(BinaryExpr {
            op: BinaryOp::BitAnd,
            ..
        })
    ));
}

#[test]
fn bitwise_xor_precedence_between_and_and_or() {
    // a | b ^ c  =>  a | (b ^ c)
    let expr = parse_value_expr("a | b ^ c");
    let Expr::Binary(BinaryExpr {
        op: BinaryOp::BitOr,
        rhs,
        ..
    }) = expr
    else {
        panic!("expected bitwise or");
    };
    assert!(matches!(
        *rhs,
        Expr::Binary(BinaryExpr {
            op: BinaryOp::BitXor,
            ..
        })
    ));
}

#[test]
fn bitwise_binds_weaker_than_comparison() {
    // a & b == c  =>  a & (b == c)  because comparison bp > bitwise bp
    let expr = parse_value_expr("a & b == c");
    let Expr::Binary(BinaryExpr {
        op: BinaryOp::BitAnd,
        rhs,
        ..
    }) = expr
    else {
        panic!("expected bitwise and");
    };
    assert!(matches!(
        *rhs,
        Expr::Binary(BinaryExpr {
            op: BinaryOp::Eq,
            ..
        })
    ));
}

#[test]
fn shift_binds_tighter_than_addition() {
    let expr = parse_value_expr("a + b << c");
    let Expr::Binary(BinaryExpr {
        op: BinaryOp::Add,
        rhs,
        ..
    }) = expr
    else {
        panic!("expected addition");
    };
    assert!(matches!(
        *rhs,
        Expr::Binary(BinaryExpr {
            op: BinaryOp::Shl,
            ..
        })
    ));
}

#[test]
fn bitnot_is_prefix_operator() {
    let expr = parse_value_expr("~a & b");
    let Expr::Binary(BinaryExpr {
        op: BinaryOp::BitAnd,
        lhs,
        ..
    }) = expr
    else {
        panic!("expected bitwise and");
    };
    assert!(matches!(
        *lhs,
        Expr::Unary(UnaryExpr {
            op: UnaryOp::BitNot,
            ..
        })
    ));
}

#[test]
fn chained_comparison_desugars_to_and() {
    // 0 < x <= 10  =>  (0 < x) and (x <= 10)
    let expr = parse_value_expr("0 < x <= 10");
    let Expr::Binary(BinaryExpr {
        op: BinaryOp::And,
        lhs,
        rhs,
        ..
    }) = expr
    else {
        panic!("expected and");
    };
    assert!(matches!(
        *lhs,
        Expr::Binary(BinaryExpr {
            op: BinaryOp::Lt,
            ..
        })
    ));
    assert!(matches!(
        *rhs,
        Expr::Binary(BinaryExpr {
            op: BinaryOp::LtEq,
            ..
        })
    ));
}

#[test]
fn named_arguments_parse() {
    let expr = parse_value_expr("spawn(name: \"goblin\", hp: 100)");
    let Expr::Call(call) = expr else {
        panic!("expected call");
    };
    assert_eq!(call.args.len(), 2);
    assert_eq!(call.args[0].name.as_ref().map(|n| n.text.as_str()), Some("name"));
    assert_eq!(call.args[1].name.as_ref().map(|n| n.text.as_str()), Some("hp"));
}

#[test]
fn mixed_positional_and_named_args_parse() {
    let expr = parse_value_expr("spawn(\"goblin\", hp: 500)");
    let Expr::Call(call) = expr else {
        panic!("expected call");
    };
    assert_eq!(call.args.len(), 2);
    assert!(call.args[0].name.is_none());
    assert!(call.args[1].name.is_some());
}

#[test]
fn not_in_parses_as_notin_op() {
    let expr = parse_value_expr("item not in items");
    assert!(matches!(
        expr,
        Expr::Binary(BinaryExpr {
            op: BinaryOp::NotIn,
            ..
        })
    ));
}

#[test]
fn optional_index_parses() {
    let expr = parse_value_expr("items?[0]");
    assert!(matches!(expr, Expr::OptionalIndex(_)));
}

#[test]
fn if_expr_parses() {
    let expr = parse_value_expr("if x > 0 { 1 } else { 0 }");
    assert!(matches!(expr, Expr::If(_)));
}

#[test]
fn if_expr_missing_else_is_error() {
    let errors = parse_error_kinds("value = if x > 0 { 1 }");
    assert!(!errors.is_empty());
}

#[test]
fn compound_assign_intdiv_parses() {
    let stmts = parse_fn_body_expr("x //= 2");
    assert!(matches!(
        stmts[0],
        coflow::ast::Stmt::Assign(ref a) if a.op == coflow::ast::AssignOp::IntDiv
    ));
}

#[test]
fn compound_assign_pow_parses() {
    let stmts = parse_fn_body_expr("x **= 3");
    assert!(matches!(
        stmts[0],
        coflow::ast::Stmt::Assign(ref a) if a.op == coflow::ast::AssignOp::Pow
    ));
}

#[test]
fn compound_assign_bitwise_parses() {
    let cases = [
        ("x &= mask",  coflow::ast::AssignOp::BitAnd),
        ("x |= flag",  coflow::ast::AssignOp::BitOr),
        ("x ^= bits",  coflow::ast::AssignOp::BitXor),
        ("x <<= 2",    coflow::ast::AssignOp::Shl),
        ("x >>= 1",    coflow::ast::AssignOp::Shr),
    ];
    for (src, expected_op) in cases {
        let stmts = parse_fn_body_expr(src);
        assert!(
            matches!(stmts[0], coflow::ast::Stmt::Assign(ref a) if a.op == expected_op),
            "failed for: {src}"
        );
    }
}

#[test]
fn dict_is_keyword_not_identifier() {
    use coflow::lexer::{lex, TokenKind};
    let output = lex("dict");
    assert_eq!(output.tokens[0].kind, TokenKind::Dict);
}
