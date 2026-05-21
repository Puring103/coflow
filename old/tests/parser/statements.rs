use coflow::ast::{AssignOp, AssignTarget, ElseBranch, FnBody, Item, Stmt, YieldStmt};
use coflow::parser::ParseErrorKind;

use crate::common::{parse_error_kinds, parse_ok};

fn parse_function_body(source: &str) -> Vec<Stmt> {
    let module = parse_ok(&format!("fn main() {{\n{source}\n}}"));
    let Item::Function(func) = &module.items[0] else {
        panic!("expected function");
    };
    let FnBody::Block(block) = &func.body else {
        panic!("expected block body");
    };
    block.stmts.clone()
}

#[test]
fn parses_var_declarations() {
    let stmts = parse_function_body("var hp = 100;\nvar name: string;\nvar alive: bool = true;");
    assert_eq!(stmts.len(), 3);
    assert!(matches!(stmts[0], Stmt::Var(_)));
    assert!(matches!(stmts[1], Stmt::Var(_)));
    assert!(matches!(stmts[2], Stmt::Var(_)));
}

#[test]
fn parses_local_function_declaration_in_block() {
    let stmts = parse_function_body(
        r#"
fn helper(x,) {
  return x;
}
"#,
    );
    assert!(matches!(stmts[0], Stmt::Function(_)));
}

#[test]
fn parses_expression_statement() {
    let stmts = parse_function_body("print(value);");
    assert!(matches!(stmts[0], Stmt::Expr(_)));
}

#[test]
fn parses_assignments_and_compound_assignments() {
    let stmts = parse_function_body(
        r#"
hp = 10;
target.hp = 10;
items[0] = value;
hp += 1;
hp -= 1;
hp *= 2;
hp /= 2;
hp %= 2;
name ??= "unknown";
"#,
    );

    let expected = [
        AssignOp::Assign,
        AssignOp::Assign,
        AssignOp::Assign,
        AssignOp::Add,
        AssignOp::Sub,
        AssignOp::Mul,
        AssignOp::Div,
        AssignOp::Rem,
        AssignOp::NullCoalesce,
    ];

    for (stmt, op) in stmts.iter().zip(expected) {
        assert!(matches!(stmt, Stmt::Assign(assign) if assign.op == op));
    }
}

#[test]
fn parses_assignment_targets() {
    let stmts = parse_function_body("name = 1;\ntarget.hp = 2;\nitems[0] = 3;");
    assert!(matches!(
        &stmts[0],
        Stmt::Assign(assign) if matches!(assign.target, AssignTarget::Name(_))
    ));
    assert!(matches!(
        &stmts[1],
        Stmt::Assign(assign) if matches!(assign.target, AssignTarget::Field { .. })
    ));
    assert!(matches!(
        &stmts[2],
        Stmt::Assign(assign) if matches!(assign.target, AssignTarget::Index { .. })
    ));
}

#[test]
fn parses_if_else_if_else() {
    let stmts = parse_function_body(
        r#"
if score >= 90 {
  rank = "S";
} else if score >= 60 {
  rank = "A";
} else {
  rank = "B";
}
"#,
    );

    let Stmt::If(if_stmt) = &stmts[0] else {
        panic!("expected if");
    };
    assert!(matches!(if_stmt.else_branch, Some(ElseBranch::If(_))));
}

#[test]
fn parses_while_and_for_in() {
    let stmts = parse_function_body(
        r#"
while running {
  update();
}
for item in items {
  print(item);
}
"#,
    );
    assert!(matches!(stmts[0], Stmt::While(_)));
    assert!(matches!(stmts[1], Stmt::ForIn(_)));
}

#[test]
fn parses_loop_and_until() {
    let stmts = parse_function_body(
        r#"
loop {
  update();
}
until done {
  tick();
}
"#,
    );
    assert!(matches!(stmts[0], Stmt::Loop(_)));
    assert!(matches!(stmts[1], Stmt::Until(_)));
}

#[test]
fn parses_control_transfer_statements() {
    let stmts = parse_function_body(
        r#"
break;
continue;
return value;
throw error("message");
"#,
    );
    assert!(matches!(stmts[0], Stmt::Break(_)));
    assert!(matches!(stmts[1], Stmt::Continue(_)));
    assert!(matches!(stmts[2], Stmt::Return(_)));
    assert!(matches!(stmts[3], Stmt::Throw(_)));
}

#[test]
fn parses_return_without_value() {
    let stmts = parse_function_body("return;");
    assert!(matches!(stmts[0], Stmt::Return(ref r) if r.value.is_none()));
}

#[test]
fn parses_try_catch() {
    let stmts = parse_function_body(
        r#"
try {
  risky();
} catch err {
  print(err.message);
}
"#,
    );
    assert!(matches!(stmts[0], Stmt::TryCatch(_)));
}

#[test]
fn parses_yield_statements() {
    let module = parse_ok(
        r#"
iter fn stream(source) {
  yield value;
  yield from source;
  return;
}
"#,
    );
    let Item::Function(func) = &module.items[0] else {
        panic!("expected function");
    };
    let FnBody::Block(block) = &func.body else {
        panic!("expected block body");
    };
    assert!(matches!(
        block.stmts[0],
        Stmt::Yield(YieldStmt::Value { .. })
    ));
    assert!(matches!(
        block.stmts[1],
        Stmt::Yield(YieldStmt::From { .. })
    ));
    assert!(matches!(
        block.stmts[2],
        Stmt::Return(ref r) if r.value.is_none()
    ));
}

#[test]
fn parses_bare_return() {
    let stmts = parse_function_body("return;");
    assert!(matches!(stmts[0], Stmt::Return(ref r) if r.value.is_none()));
}

#[test]
fn rejects_throw_without_value() {
    let errors = parse_error_kinds("fn main() { throw; }");
    assert!(errors.contains(&ParseErrorKind::ExpectedExpression));
}

#[test]
fn rejects_try_without_catch() {
    let errors = parse_error_kinds("fn main() { try { risky(); } }");
    assert!(errors.contains(&ParseErrorKind::MissingCatch));
}

#[test]
fn rejects_catch_without_error_name() {
    let errors = parse_error_kinds("fn main() { try {} catch { } }");
    assert!(errors.contains(&ParseErrorKind::ExpectedIdentifier));
}

#[test]
fn rejects_malformed_for_in() {
    let errors = parse_error_kinds("fn main() { for item items {} }");
    assert!(errors.contains(&ParseErrorKind::ExpectedToken));
}

#[test]
fn rejects_invalid_assignment_target() {
    let errors = parse_error_kinds("fn main() { a + b = 1; }");
    assert!(errors.contains(&ParseErrorKind::InvalidAssignmentTarget));
}

#[test]
fn rejects_break_with_value() {
    let errors = parse_error_kinds("fn main() { break value; }");
    assert!(errors.contains(&ParseErrorKind::UnexpectedToken));
}

#[test]
fn rejects_yield_from_without_value() {
    let errors = parse_error_kinds("iter fn main() { yield from; }");
    assert!(errors.contains(&ParseErrorKind::ExpectedExpression));
}
