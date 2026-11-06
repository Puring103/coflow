#![allow(clippy::expect_used, clippy::panic)]

use coflow_language::cft::syntax::ast::{
    AnnotationArg, CheckExprKind, CheckFormatSegment, CheckMessageKind, CheckStmt, DefaultExprKind,
    Item,
};
use coflow_language::cft::syntax::parser::parse_module;
use coflow_language::cft::ModuleId;
use coflow_language::diagnostics::CftErrorCode;

#[test]
fn owned_token_text_survives_annotations_defaults_and_checks() {
    let source = r#"
        @label("配置\n名称")
        table 配置 {
            name: string = "默认\n文本";
            check 检查: "整块消息" {
                name != "拒绝值" : f"名称 {{escaped}} {name} / {name}";
                name == "允许值" : "单项消息";
            }
        }
        # 插值后的字符串和注释不能改变前面各个插值的边界。
        const 尾部: string = "末尾 }";
    "#;
    let ast = parse_module(&ModuleId::from("owned.cft"), source).expect("valid module");
    let Item::Type(definition) = &ast.items[0] else {
        panic!("type declaration expected");
    };
    assert_eq!(definition.name, "配置");
    assert!(matches!(
        &definition.annotations[0].args[0],
        AnnotationArg::String(value, _) if value == "配置\n名称"
    ));
    assert_eq!(definition.fields[0].name, "name");
    assert!(matches!(
        &definition.fields[0].default.as_ref().expect("default").kind,
        DefaultExprKind::String(value) if value == "默认\n文本"
    ));
    let check = &definition.checks[0];
    assert_eq!(check.name.as_deref(), Some("检查"));
    assert!(matches!(
        &check.message.as_ref().expect("block message").kind,
        CheckMessageKind::String(value) if value == "整块消息"
    ));
    let CheckStmt::Expr {
        condition,
        message: Some(message),
        ..
    } = &check.stmts[0]
    else {
        panic!("expression statement with message expected");
    };
    let CheckExprKind::CmpChain { first, rest } = &condition.kind else {
        panic!("comparison expected");
    };
    assert!(matches!(&first.kind, CheckExprKind::Name(value) if value == "name"));
    assert!(matches!(&rest[0].1.kind, CheckExprKind::String(value) if value == "拒绝值"));
    let CheckMessageKind::Formatted(segments) = &message.kind else {
        panic!("formatted message expected");
    };
    assert_eq!(segments.len(), 4);
    assert!(
        matches!(&segments[0], CheckFormatSegment::Text(value, _) if value == "名称 {escaped} ")
    );
    assert!(matches!(&segments[2], CheckFormatSegment::Text(value, _) if value == " / "));
    for segment in [&segments[1], &segments[3]] {
        assert!(matches!(segment, CheckFormatSegment::Expr(expression)
            if matches!(&expression.kind, CheckExprKind::Name(value) if value == "name")));
    }
    assert!(
        matches!(&check.stmts[1], CheckStmt::Expr { message: Some(message), .. }
        if matches!(&message.kind, CheckMessageKind::String(value) if value == "单项消息"))
    );
    let Item::Const(definition) = &ast.items[1] else {
        panic!("constant declaration expected");
    };
    assert_eq!(definition.name, "尾部");
    assert!(matches!(&definition.value.kind, DefaultExprKind::String(value) if value == "末尾 }"));
}

#[test]
fn recovery_keeps_brace_depth_after_consuming_owned_tokens() {
    let source = r#"
        table Broken {
            name: string = "已消费";
            check Invalid { name == "值" }
            table Fake { nested: ; }
        }
        @label("后续")
        table Following { missing: ; }
    "#;
    let diagnostics = parse_module(&ModuleId::from("recovery.cft"), source)
        .expect_err("both declarations contain syntax errors");
    let errors = &diagnostics.diagnostics;
    assert_eq!(errors.len(), 2, "{diagnostics:?}");
    assert_eq!(errors[0].code, CftErrorCode::InvalidCheckStatement);
    let first = errors[0].primary.as_ref().expect("first source label").span;
    let second = errors[1]
        .primary
        .as_ref()
        .expect("second source label")
        .span;
    assert_eq!(&source[first.start..first.end], "}");
    assert_eq!(&source[second.start..second.end], ";");
    assert!(second.start > source.find("Following").expect("following declaration"));
}
