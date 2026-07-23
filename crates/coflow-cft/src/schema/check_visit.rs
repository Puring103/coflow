use super::{
    CftSchemaCheckBlock, CftSchemaCheckExpr, CftSchemaCheckExprKind, CftSchemaCheckFormatSegment,
    CftSchemaCheckMessageKind, CftSchemaCheckStmt,
};

pub(crate) trait CheckVisitor {
    fn visit_block(&mut self, block: &CftSchemaCheckBlock) {
        for stmt in &block.stmts {
            self.visit_stmt(stmt);
        }
    }

    fn visit_stmt(&mut self, stmt: &CftSchemaCheckStmt) {
        match stmt {
            CftSchemaCheckStmt::Expr {
                condition, message, ..
            } => {
                self.visit_expr(condition);
                if let Some(message) = message {
                    match &message.kind {
                        CftSchemaCheckMessageKind::String(_) => {}
                        CftSchemaCheckMessageKind::Formatted(segments) => {
                            self.visit_segments(segments);
                        }
                    }
                }
            }
            CftSchemaCheckStmt::Quantifier {
                collection, body, ..
            } => {
                self.visit_expr(collection);
                for stmt in body {
                    self.visit_stmt(stmt);
                }
            }
            CftSchemaCheckStmt::When {
                condition, body, ..
            } => {
                self.visit_expr(condition);
                for stmt in body {
                    self.visit_stmt(stmt);
                }
            }
        }
    }

    fn visit_expr(&mut self, expr: &CftSchemaCheckExpr) {
        match &expr.kind {
            CftSchemaCheckExprKind::Int(_)
            | CftSchemaCheckExprKind::Float(_)
            | CftSchemaCheckExprKind::Bool(_)
            | CftSchemaCheckExprKind::Null
            | CftSchemaCheckExprKind::String(_)
            | CftSchemaCheckExprKind::Name(_) => {}
            CftSchemaCheckExprKind::Records { type_name } => self.visit_records(type_name),
            CftSchemaCheckExprKind::FormattedString(segments) => self.visit_segments(segments),
            CftSchemaCheckExprKind::Field { expr, .. }
            | CftSchemaCheckExprKind::SafeField { expr, .. }
            | CftSchemaCheckExprKind::Is { expr, .. }
            | CftSchemaCheckExprKind::Unary { expr, .. } => self.visit_expr(expr),
            CftSchemaCheckExprKind::Index { expr, index }
            | CftSchemaCheckExprKind::SafeIndex { expr, index } => {
                self.visit_expr(expr);
                self.visit_expr(index);
            }
            CftSchemaCheckExprKind::Coalesce { lhs, rhs }
            | CftSchemaCheckExprKind::BinOp { lhs, rhs, .. } => {
                self.visit_expr(lhs);
                self.visit_expr(rhs);
            }
            CftSchemaCheckExprKind::Call { args, .. } => {
                for arg in args {
                    self.visit_expr(arg);
                }
            }
            CftSchemaCheckExprKind::MethodCall {
                receiver, args, ..
            } => {
                self.visit_expr(receiver);
                for arg in args {
                    self.visit_expr(arg);
                }
            }
            CftSchemaCheckExprKind::CmpChain { first, rest } => {
                self.visit_expr(first);
                for (_, expr) in rest {
                    self.visit_expr(expr);
                }
            }
        }
    }

    fn visit_records(&mut self, _type_name: &crate::TypeName) {}

    fn visit_segments(&mut self, segments: &[CftSchemaCheckFormatSegment]) {
        for segment in segments {
            match segment {
                CftSchemaCheckFormatSegment::Text(_, _) => {}
                CftSchemaCheckFormatSegment::Expr(expr) => self.visit_expr(expr),
            }
        }
    }
}
