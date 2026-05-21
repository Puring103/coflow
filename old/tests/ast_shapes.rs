use coflow::ast::{
    BinaryExpr, BinaryOp, Expr, FnBody, FnDecl, Ident, Literal, Module, Span, Stmt, VarDecl,
};

#[test]
fn ast_nodes_carry_spans_and_raw_literals() {
    let name = Ident {
        text: "hp".to_string(),
        span: Span { start: 4, end: 6 },
    };
    let value = Expr::Literal(Literal::Int {
        raw: "1_000".to_string(),
        span: Span { start: 9, end: 14 },
    });

    let stmt = Stmt::Var(VarDecl {
        local: false,
        name: name.clone(),
        ty: None,
        init: Some(value),
        span: Span { start: 0, end: 14 },
    });

    let module = Module {
        items: Vec::new(),
        span: Span { start: 0, end: 14 },
    };

    assert_eq!(name.text, "hp");
    assert_eq!(module.span, Span { start: 0, end: 14 });
    assert!(matches!(stmt, Stmt::Var(_)));
}

#[test]
fn blocks_can_contain_named_function_declarations() {
    let name = Ident {
        text: "helper".to_string(),
        span: Span { start: 3, end: 9 },
    };

    let stmt = Stmt::Function(FnDecl {
        local: false,
        iter: false,
        name,
        params: Vec::new(),
        return_type: None,
        body: FnBody::Block(coflow::ast::Block {
            stmts: Vec::new(),
            span: Span { start: 12, end: 14 },
        }),
        span: Span { start: 0, end: 14 },
    });

    assert!(matches!(stmt, Stmt::Function(_)));
}

#[test]
fn expression_tree_preserves_syntax_shape() {
    let lhs = Expr::Name(Ident {
        text: "a".to_string(),
        span: Span { start: 0, end: 1 },
    });
    let rhs = Expr::Name(Ident {
        text: "b".to_string(),
        span: Span { start: 4, end: 5 },
    });

    let expr = Expr::Binary(BinaryExpr {
        lhs: Box::new(lhs),
        op: BinaryOp::Add,
        rhs: Box::new(rhs),
        span: Span { start: 0, end: 5 },
    });

    assert!(matches!(
        expr,
        Expr::Binary(BinaryExpr {
            op: BinaryOp::Add,
            ..
        })
    ));
}
