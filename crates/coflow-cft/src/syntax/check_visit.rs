use super::ast::{
    CheckBlock, CheckExpr, CheckExprKind, CheckFormatSegment, CheckMessageKind, CheckStmt, NameRef,
};

pub trait CheckVisitor {
    type Error;

    fn visit_block(&mut self, block: &CheckBlock) -> Result<(), Self::Error> {
        for stmt in &block.stmts {
            self.visit_stmt(stmt)?;
        }
        Ok(())
    }

    fn visit_stmt(&mut self, stmt: &CheckStmt) -> Result<(), Self::Error> {
        self.walk_stmt(stmt)
    }

    fn walk_stmt(&mut self, stmt: &CheckStmt) -> Result<(), Self::Error> {
        match stmt {
            CheckStmt::Expr {
                condition, message, ..
            } => {
                self.visit_expr(condition)?;
                if let Some(message) = message {
                    match &message.kind {
                        CheckMessageKind::String(_) => {}
                        CheckMessageKind::Formatted(segments) => self.visit_segments(segments)?,
                    }
                }
            }
            CheckStmt::Quantifier {
                bindings,
                collection,
                body,
                ..
            } => {
                self.visit_expr(collection)?;
                self.enter_quantifier_body(bindings)?;
                for stmt in body {
                    self.visit_stmt(stmt)?;
                }
                self.exit_quantifier_body(bindings)?;
            }
            CheckStmt::When {
                condition, body, ..
            } => {
                self.visit_expr(condition)?;
                for stmt in body {
                    self.visit_stmt(stmt)?;
                }
            }
        }
        Ok(())
    }

    fn enter_quantifier_body(&mut self, _bindings: &[NameRef]) -> Result<(), Self::Error> {
        Ok(())
    }

    fn exit_quantifier_body(&mut self, _bindings: &[NameRef]) -> Result<(), Self::Error> {
        Ok(())
    }

    fn visit_expr(&mut self, expr: &CheckExpr) -> Result<(), Self::Error> {
        self.walk_expr(expr)
    }

    fn walk_expr(&mut self, expr: &CheckExpr) -> Result<(), Self::Error> {
        match &expr.kind {
            CheckExprKind::Int(_)
            | CheckExprKind::Float(_)
            | CheckExprKind::Bool(_)
            | CheckExprKind::Null
            | CheckExprKind::String(_)
            | CheckExprKind::Name(_)
            | CheckExprKind::Records { .. } => {}
            CheckExprKind::FormattedString(segments) => self.visit_segments(segments)?,
            CheckExprKind::Field { expr, .. }
            | CheckExprKind::SafeField { expr, .. }
            | CheckExprKind::Is { expr, .. }
            | CheckExprKind::Unary { expr, .. } => self.visit_expr(expr)?,
            CheckExprKind::Index { expr, index }
            | CheckExprKind::SafeIndex { expr, index } => {
                self.visit_expr(expr)?;
                self.visit_expr(index)?;
            }
            CheckExprKind::Coalesce { lhs, rhs } | CheckExprKind::BinOp { lhs, rhs, .. } => {
                self.visit_expr(lhs)?;
                self.visit_expr(rhs)?;
            }
            CheckExprKind::Call { args, .. } => {
                for arg in args {
                    self.visit_expr(arg)?;
                }
            }
            CheckExprKind::MethodCall {
                receiver, args, ..
            } => {
                self.visit_expr(receiver)?;
                for arg in args {
                    self.visit_expr(arg)?;
                }
            }
            CheckExprKind::CmpChain { first, rest } => {
                self.visit_expr(first)?;
                for (_, expr) in rest {
                    self.visit_expr(expr)?;
                }
            }
        }
        Ok(())
    }

    fn visit_segments(&mut self, segments: &[CheckFormatSegment]) -> Result<(), Self::Error> {
        for segment in segments {
            match segment {
                CheckFormatSegment::Text(_, _) => {}
                CheckFormatSegment::Expr(expr) => self.visit_expr(expr)?,
            }
        }
        Ok(())
    }
}
