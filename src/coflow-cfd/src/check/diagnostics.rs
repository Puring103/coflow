use crate::ast::{BinOp, CheckExpr, CheckExprKind, CmpOp, TypePredicate, UnaryOp};
use crate::error::{AllFailedItem, CheckError, CheckErrorKind};
use crate::span::Span;
use crate::ModuleId;

pub(super) fn describe_expr(expr: &CheckExpr) -> String {
    match &expr.kind {
        CheckExprKind::Int(value) => value.to_string(),
        CheckExprKind::Float(value) => value.to_string(),
        CheckExprKind::Bool(value) => value.to_string(),
        CheckExprKind::Null => "null".to_string(),
        CheckExprKind::Str(value) => format!("{value:?}"),
        CheckExprKind::Name(name) => name.clone(),
        CheckExprKind::Field { expr, name } => format!("{}.{}", describe_expr(expr), name),
        CheckExprKind::Index { expr, index } => {
            format!("{}[{}]", describe_expr(expr), describe_expr(index))
        }
        CheckExprKind::Is { expr, predicate } => {
            format!(
                "{} is {}",
                describe_expr(expr),
                describe_type_predicate(predicate)
            )
        }
        CheckExprKind::Call { name, args } => {
            let args = args.iter().map(describe_expr).collect::<Vec<_>>().join(", ");
            format!("{name}({args})")
        }
        CheckExprKind::BinOp { op, lhs, rhs } => {
            format!("{} {} {}", describe_expr(lhs), bin_op_name(*op), describe_expr(rhs))
        }
        CheckExprKind::Unary { op, expr } => {
            format!("{}{}", unary_op_name(*op), describe_expr(expr))
        }
        CheckExprKind::CmpChain { first, rest } => {
            let mut out = describe_expr(first);
            for (op, expr) in rest {
                out.push(' ');
                out.push_str(cmp_op_name(*op));
                out.push(' ');
                out.push_str(&describe_expr(expr));
            }
            out
        }
    }
}

fn describe_type_predicate(predicate: &TypePredicate) -> String {
    match predicate {
        TypePredicate::Type(crate::ast::TypeName::Local(name)) => name.clone(),
        TypePredicate::Null => "null".to_string(),
    }
}

pub(super) fn cond_failed(
    source: String,
    module: &ModuleId,
    context: &str,
    span: Span,
) -> CheckError {
    let message = format!("check failed [{context}]: {source}");
    CheckError {
        message,
        span: Some(span),
        module: Some(module.as_str().to_string()),
        kind: CheckErrorKind::CondFailed {
            evaluated: source.clone(),
            source,
            context: context.to_string(),
        },
    }
}

pub(super) fn all_failed(
    source: String,
    module: &ModuleId,
    context: &str,
    total: usize,
    failed: Vec<AllFailedItem>,
    span: Span,
) -> CheckError {
    let message = format!(
        "check failed [{context}]: {source} ({}/{total} failed)",
        failed.len()
    );
    CheckError {
        message,
        span: Some(span),
        module: Some(module.as_str().to_string()),
        kind: CheckErrorKind::AllFailed {
            source,
            context: context.to_string(),
            total,
            failed,
        },
    }
}

pub(super) fn eval_error(
    message: String,
    module: &ModuleId,
    context: &str,
    span: Span,
) -> CheckError {
    CheckError {
        message: format!("check eval error [{context}]: {message}"),
        span: Some(span),
        module: Some(module.as_str().to_string()),
        kind: CheckErrorKind::EvalError {
            message,
            context: context.to_string(),
        },
    }
}

fn bin_op_name(op: BinOp) -> &'static str {
    match op {
        BinOp::Or => "||",
        BinOp::And => "&&",
        BinOp::BitOr => "|",
        BinOp::BitXor => "^",
        BinOp::BitAnd => "&",
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::IntDiv => "//",
        BinOp::Mod => "%",
        BinOp::Pow => "**",
        BinOp::Shl => "<<",
        BinOp::Shr => ">>",
    }
}

fn unary_op_name(op: UnaryOp) -> &'static str {
    match op {
        UnaryOp::Not => "!",
        UnaryOp::BitNot => "~",
        UnaryOp::Neg => "-",
    }
}

fn cmp_op_name(op: CmpOp) -> &'static str {
    match op {
        CmpOp::Eq => "==",
        CmpOp::Ne => "!=",
        CmpOp::Lt => "<",
        CmpOp::Le => "<=",
        CmpOp::Gt => ">",
        CmpOp::Ge => ">=",
    }
}
