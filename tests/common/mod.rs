#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use std::fmt::Write;

use coflow::ast::{
    AssignOp, AssignTarget, BinaryOp, Block, ElseBranch, Expr, FnBody, Item, Literal, Module,
    RecordEntry, RecordKey, Stmt, StringKind, TypeExpr, YieldStmt,
};
use coflow::lexer::{LexError, Token};
use coflow::parser::{parse_module, ParseError, ParseErrorKind};

pub fn fixture_files(root: impl AsRef<Path>) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_fixture_files(root.as_ref(), &mut files);
    files.sort();
    files
}

pub fn render_tokens(source: &str, tokens: &[Token]) -> String {
    tokens
        .iter()
        .map(|token| {
            format!(
                "{:?} {:?}",
                token.kind,
                &source[token.span.start..token.span.end]
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn render_lex_errors(source: &str, errors: &[LexError]) -> String {
    errors
        .iter()
        .map(|error| {
            format!(
                "{:?} [{}..{}] {:?}",
                error.kind,
                error.span.start,
                error.span.end,
                &source[error.span.start..error.span.end]
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn render_parse_errors(_source: &str, errors: &[ParseError]) -> String {
    errors
        .iter()
        .map(|error| format!("{:?}", error.kind))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn render_ast(module: &Module) -> String {
    let mut out = String::new();
    writeln!(&mut out, "Module").unwrap();
    for item in &module.items {
        render_item(&mut out, item, 1);
    }
    out.trim_end().to_string()
}

pub fn parse_ok(source: &str) -> Module {
    let output = parse_module(source);
    assert_eq!(output.errors, [], "source should parse cleanly:\n{source}");
    output.module.expect("parser should return a module")
}

pub fn parse_error_kinds(source: &str) -> Vec<ParseErrorKind> {
    parse_module(source)
        .errors
        .into_iter()
        .map(|error| error.kind)
        .collect()
}

pub fn parse_errors(source: &str) -> Vec<ParseError> {
    parse_module(source).errors
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

fn render_item(out: &mut String, item: &Item, indent: usize) {
    match item {
        Item::Import(import) => {
            line(
                out,
                indent,
                &format!("Import {}", render_path_segments(&import.module.segments)),
            );
            if let Some(alias) = &import.alias {
                line(out, indent + 1, &format!("Alias {}", alias.text));
            }
        }
        Item::Class(class) => {
            line(
                out,
                indent,
                &format!(
                    "{}Class {}",
                    if class.local { "Local " } else { "" },
                    class.name.text
                ),
            );
            for field in &class.fields {
                line(out, indent + 1, &format!("Field {}", field.name.text));
                render_type(out, &field.ty, indent + 2);
                if let Some(default) = &field.default {
                    line(out, indent + 2, "Default");
                    render_expr(out, default, indent + 3);
                }
            }
            if !class.checks.is_empty() {
                line(out, indent + 1, "Check");
                for arm in &class.checks {
                    line(out, indent + 2, "Arm");
                    render_expr(out, &arm.condition, indent + 3);
                    render_expr(out, &arm.message, indent + 3);
                }
            }
        }
        Item::Enum(enum_decl) => {
            line(
                out,
                indent,
                &format!(
                    "{}Enum {}",
                    if enum_decl.local { "Local " } else { "" },
                    enum_decl.name.text
                ),
            );
            for variant in &enum_decl.variants {
                if let Some(v) = variant.value {
                    line(out, indent + 1, &format!("Variant {} = {}", variant.name.text, v));
                } else {
                    line(out, indent + 1, &format!("Variant {}", variant.name.text));
                }
            }
        }
        Item::Function(func) => {
            line(
                out,
                indent,
                &format!(
                    "{}{}Fn {}",
                    if func.local { "Local " } else { "" },
                    if func.iter { "Iter " } else { "" },
                    func.name.text
                ),
            );
            for param in &func.params {
                line(out, indent + 1, &format!("Param {}", param.name.text));
                if let Some(ty) = &param.ty {
                    render_type(out, ty, indent + 2);
                }
            }
            if let Some(ret_ty) = &func.return_type {
                line(out, indent + 1, "ReturnType");
                render_type(out, ret_ty, indent + 2);
            }
            render_fn_body(out, &func.body, indent + 1);
        }
        Item::Var(var) => {
            line(
                out,
                indent,
                &format!(
                    "{}Var {}",
                    if var.local { "Local " } else { "" },
                    var.name.text
                ),
            );
            if let Some(ty) = &var.ty {
                render_type(out, ty, indent + 1);
            }
            if let Some(init) = &var.init {
                render_expr(out, init, indent + 1);
            }
        }
        Item::Config(config) => {
            line(out, indent, &format!("Config {}", config.name.text));
            if let Some(ty) = &config.ty {
                render_type(out, ty, indent + 1);
            }
            render_expr(out, &config.value, indent + 1);
        }
    }
}

fn render_fn_body(out: &mut String, body: &FnBody, indent: usize) {
    match body {
        FnBody::Block(block) => render_block(out, block, indent),
        FnBody::Expr(expr) => {
            line(out, indent, "ExprBody");
            render_expr(out, expr, indent + 1);
        }
    }
}

fn render_block(out: &mut String, block: &Block, indent: usize) {
    line(out, indent, "Block");
    for stmt in &block.stmts {
        render_stmt(out, stmt, indent + 1);
    }
}

fn render_stmt(out: &mut String, stmt: &Stmt, indent: usize) {
    match stmt {
        Stmt::Function(func) => render_item(out, &Item::Function(func.clone()), indent),
        Stmt::Var(var) => render_item(out, &Item::Var(var.clone()), indent),
        Stmt::Assign(assign) => {
            line(
                out,
                indent,
                &format!("Assign {}", render_assign_op(assign.op)),
            );
            render_assign_target(out, &assign.target, indent + 1);
            render_expr(out, &assign.value, indent + 1);
        }
        Stmt::Expr(expr) => {
            line(out, indent, "ExprStmt");
            render_expr(out, expr, indent + 1);
        }
        Stmt::If(if_stmt) => {
            line(out, indent, "If");
            render_expr(out, &if_stmt.condition, indent + 1);
            render_block(out, &if_stmt.then_block, indent + 1);
            if let Some(else_branch) = &if_stmt.else_branch {
                match else_branch {
                    ElseBranch::If(nested) => {
                        line(out, indent + 1, "ElseIf");
                        render_stmt(out, &Stmt::If((**nested).clone()), indent + 2);
                    }
                    ElseBranch::Block(block) => {
                        line(out, indent + 1, "Else");
                        render_block(out, block, indent + 2);
                    }
                }
            }
        }
        Stmt::While(while_stmt) => {
            line(out, indent, "While");
            render_expr(out, &while_stmt.condition, indent + 1);
            render_block(out, &while_stmt.body, indent + 1);
        }
        Stmt::Until(until_stmt) => {
            line(out, indent, "Until");
            render_expr(out, &until_stmt.condition, indent + 1);
            render_block(out, &until_stmt.body, indent + 1);
        }
        Stmt::Loop(loop_stmt) => {
            line(out, indent, "Loop");
            render_block(out, &loop_stmt.body, indent + 1);
        }
        Stmt::ForIn(for_stmt) => {
            line(out, indent, &format!("ForIn {}", for_stmt.item.text));
            render_expr(out, &for_stmt.iterable, indent + 1);
            render_block(out, &for_stmt.body, indent + 1);
        }
        Stmt::Break(_) => line(out, indent, "Break"),
        Stmt::Continue(_) => line(out, indent, "Continue"),
        Stmt::Return(ret) => {
            line(out, indent, "Return");
            if let Some(value) = &ret.value {
                render_expr(out, value, indent + 1);
            }
        }
        Stmt::Throw(throw) => {
            line(out, indent, "Throw");
            render_expr(out, &throw.value, indent + 1);
        }
        Stmt::TryCatch(try_catch) => {
            line(
                out,
                indent,
                &format!("TryCatch {}", try_catch.error_name.text),
            );
            render_block(out, &try_catch.try_block, indent + 1);
            render_block(out, &try_catch.catch_block, indent + 1);
        }
        Stmt::Yield(yield_stmt) => match yield_stmt {
            YieldStmt::Value { value, .. } => {
                line(out, indent, "Yield");
                render_expr(out, value, indent + 1);
            }
            YieldStmt::From { value, .. } => {
                line(out, indent, "YieldFrom");
                render_expr(out, value, indent + 1);
            }
        },
    }
}

fn render_expr(out: &mut String, expr: &Expr, indent: usize) {
    match expr {
        Expr::Literal(literal) => render_literal(out, literal, indent),
        Expr::Name(name) => line(out, indent, &format!("Name {}", name.text)),
        Expr::Array(array) => {
            line(out, indent, "Array");
            for element in &array.elements {
                render_expr(out, element, indent + 1);
            }
        }
        Expr::Record(record) => {
            line(out, indent, "Record");
            for entry in &record.entries {
                match entry {
                    RecordEntry::Field { key, value, .. } => {
                        line(
                            out,
                            indent + 1,
                            &format!("Entry {}", render_record_key(key)),
                        );
                        render_expr(out, value, indent + 2);
                    }
                    RecordEntry::Spread { expr, .. } => {
                        line(out, indent + 1, "Spread");
                        render_expr(out, expr, indent + 2);
                    }
                }
            }
        }
        Expr::Fn(func) => {
            line(out, indent, if func.iter { "Iter FnExpr" } else { "FnExpr" });
            for param in &func.params {
                line(out, indent + 1, &format!("Param {}", param.name.text));
                if let Some(ty) = &param.ty {
                    render_type(out, ty, indent + 2);
                }
            }
            if let Some(ret_ty) = &func.return_type {
                line(out, indent + 1, "ReturnType");
                render_type(out, ret_ty, indent + 2);
            }
            render_fn_body(out, &func.body, indent + 1);
        }
        Expr::Lambda(lambda) => {
            line(out, indent, "Lambda");
            for param in &lambda.params {
                line(out, indent + 1, &format!("Param {}", param.name.text));
            }
            render_fn_body(out, &lambda.body, indent + 1);
        }
        Expr::Range(range) => {
            line(out, indent, if range.inclusive { "RangeInclusive" } else { "Range" });
            render_expr(out, &range.start, indent + 1);
            render_expr(out, &range.end, indent + 1);
        }
        Expr::Unary(unary) => {
            line(out, indent, &format!("Unary {:?}", unary.op));
            render_expr(out, &unary.expr, indent + 1);
        }
        Expr::Binary(binary) => {
            line(
                out,
                indent,
                &format!("Binary {}", render_binary_op(binary.op)),
            );
            render_expr(out, &binary.lhs, indent + 1);
            render_expr(out, &binary.rhs, indent + 1);
        }
        Expr::Call(call) => {
            line(out, indent, "Call");
            render_expr(out, &call.callee, indent + 1);
            for arg in &call.args {
                if let Some(name) = &arg.name {
                    line(out, indent + 1, &format!("Arg {}", name.text));
                } else {
                    line(out, indent + 1, "Arg");
                }
                render_expr(out, &arg.value, indent + 2);
            }
        }
        Expr::Field(field) => {
            line(out, indent, &format!("Field {}", field.field.text));
            render_expr(out, &field.object, indent + 1);
        }
        Expr::OptionalField(field) => {
            line(out, indent, &format!("OptionalField {}", field.field.text));
            render_expr(out, &field.object, indent + 1);
        }
        Expr::Index(index) => {
            line(out, indent, "Index");
            render_expr(out, &index.object, indent + 1);
            render_expr(out, &index.index, indent + 1);
        }
        Expr::OptionalIndex(index) => {
            line(out, indent, "OptionalIndex");
            render_expr(out, &index.object, indent + 1);
            render_expr(out, &index.index, indent + 1);
        }
        Expr::If(if_expr) => {
            line(out, indent, "IfExpr");
            render_expr(out, &if_expr.condition, indent + 1);
            render_expr(out, &if_expr.then_expr, indent + 1);
            render_expr(out, &if_expr.else_expr, indent + 1);
        }
    }
}

fn render_type(out: &mut String, ty: &TypeExpr, indent: usize) {
    match ty {
        TypeExpr::Name(path) => line(
            out,
            indent,
            &format!("Type {}", render_path_segments(&path.segments)),
        ),
        TypeExpr::Array { element, .. } => {
            line(out, indent, "TypeArray");
            render_type(out, element, indent + 1);
        }
        TypeExpr::Dict { key, value, .. } => {
            line(out, indent, "TypeDict");
            render_type(out, key, indent + 1);
            render_type(out, value, indent + 1);
        }
    }
}

fn render_literal(out: &mut String, literal: &Literal, indent: usize) {
    match literal {
        Literal::Int { raw, .. } => line(out, indent, &format!("Int {raw}")),
        Literal::Float { raw, .. } => line(out, indent, &format!("Float {raw}")),
        Literal::String(string) => line(
            out,
            indent,
            &format!(
                "String {} {:?}",
                render_string_kind(string.kind),
                string.raw
            ),
        ),
        Literal::Bool { value, .. } => line(out, indent, &format!("Bool {value}")),
        Literal::Null { .. } => line(out, indent, "Null"),
    }
}

fn render_assign_target(out: &mut String, target: &AssignTarget, indent: usize) {
    match target {
        AssignTarget::Name(name) => line(out, indent, &format!("TargetName {}", name.text)),
        AssignTarget::Field { object, field, .. } => {
            line(out, indent, &format!("TargetField {}", field.text));
            render_expr(out, object, indent + 1);
        }
        AssignTarget::Index { object, index, .. } => {
            line(out, indent, "TargetIndex");
            render_expr(out, object, indent + 1);
            render_expr(out, index, indent + 1);
        }
    }
}

fn render_record_key(key: &RecordKey) -> String {
    match key {
        RecordKey::Ident(ident) => ident.text.clone(),
        RecordKey::String(string) => format!("{:?}", string.raw),
    }
}

fn render_path_segments(segments: &[coflow::ast::Ident]) -> String {
    segments
        .iter()
        .map(|segment| segment.text.as_str())
        .collect::<Vec<_>>()
        .join(".")
}

fn render_binary_op(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "Add",
        BinaryOp::Sub => "Sub",
        BinaryOp::Mul => "Mul",
        BinaryOp::Div => "Div",
        BinaryOp::IntDiv => "IntDiv",
        BinaryOp::Rem => "Rem",
        BinaryOp::Pow => "Pow",
        BinaryOp::Eq => "Eq",
        BinaryOp::NotEq => "NotEq",
        BinaryOp::Lt => "Lt",
        BinaryOp::LtEq => "LtEq",
        BinaryOp::Gt => "Gt",
        BinaryOp::GtEq => "GtEq",
        BinaryOp::And => "And",
        BinaryOp::Or => "Or",
        BinaryOp::NullCoalesce => "NullCoalesce",
        BinaryOp::In => "In",
        BinaryOp::NotIn => "NotIn",
        BinaryOp::BitAnd => "BitAnd",
        BinaryOp::BitOr => "BitOr",
        BinaryOp::BitXor => "BitXor",
        BinaryOp::Shl => "Shl",
        BinaryOp::Shr => "Shr",
    }
}

fn render_assign_op(op: AssignOp) -> &'static str {
    match op {
        AssignOp::Assign => "Assign",
        AssignOp::Add => "Add",
        AssignOp::Sub => "Sub",
        AssignOp::Mul => "Mul",
        AssignOp::Div => "Div",
        AssignOp::IntDiv => "IntDiv",
        AssignOp::Rem => "Rem",
        AssignOp::Pow => "Pow",
        AssignOp::NullCoalesce => "NullCoalesce",
        AssignOp::BitAnd => "BitAnd",
        AssignOp::BitOr => "BitOr",
        AssignOp::BitXor => "BitXor",
        AssignOp::Shl => "Shl",
        AssignOp::Shr => "Shr",
    }
}

fn render_string_kind(kind: StringKind) -> &'static str {
    match kind {
        StringKind::Normal => "Normal",
        StringKind::Raw => "Raw",
        StringKind::Multiline => "Multiline",
        StringKind::RawMultiline => "RawMultiline",
    }
}

fn line(out: &mut String, indent: usize, text: &str) {
    for _ in 0..indent {
        out.push_str("  ");
    }
    writeln!(out, "{text}").unwrap();
}
