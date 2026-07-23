use super::{
    CftSchemaCheckBlock, CftSchemaCheckExpr, CftSchemaCheckExprKind, CftSchemaCheckFormatSegment,
    CftSchemaCheckMessageKind, CftSchemaCheckStmt, CftSchemaQuantifierBindings,
};

pub(crate) trait CheckVisitor {
    type Error;

    fn visit_block(&mut self, block: &CftSchemaCheckBlock) -> Result<(), Self::Error> {
        for stmt in &block.stmts {
            self.visit_stmt(stmt)?;
        }
        Ok(())
    }

    fn visit_stmt(&mut self, stmt: &CftSchemaCheckStmt) -> Result<(), Self::Error> {
        self.walk_stmt(stmt)
    }

    fn walk_stmt(&mut self, stmt: &CftSchemaCheckStmt) -> Result<(), Self::Error> {
        match stmt {
            CftSchemaCheckStmt::Expr {
                condition, message, ..
            } => {
                self.visit_expr(condition)?;
                if let Some(message) = message {
                    match &message.kind {
                        CftSchemaCheckMessageKind::String(_) => {}
                        CftSchemaCheckMessageKind::Formatted(segments) => {
                            self.visit_segments(segments)?;
                        }
                    }
                }
            }
            CftSchemaCheckStmt::Quantifier {
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
            CftSchemaCheckStmt::When {
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

    fn enter_quantifier_body(
        &mut self,
        _bindings: &CftSchemaQuantifierBindings,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    fn exit_quantifier_body(
        &mut self,
        _bindings: &CftSchemaQuantifierBindings,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    fn visit_expr(&mut self, expr: &CftSchemaCheckExpr) -> Result<(), Self::Error> {
        self.walk_expr(expr)
    }

    fn walk_expr(&mut self, expr: &CftSchemaCheckExpr) -> Result<(), Self::Error> {
        match &expr.kind {
            CftSchemaCheckExprKind::Int(_)
            | CftSchemaCheckExprKind::Float(_)
            | CftSchemaCheckExprKind::Bool(_)
            | CftSchemaCheckExprKind::Null
            | CftSchemaCheckExprKind::String(_) => {}
            CftSchemaCheckExprKind::Name(name) => self.visit_name(name)?,
            CftSchemaCheckExprKind::Records { type_name } => self.visit_records(type_name)?,
            CftSchemaCheckExprKind::FormattedString(segments) => self.visit_segments(segments)?,
            CftSchemaCheckExprKind::Field { expr, .. }
            | CftSchemaCheckExprKind::SafeField { expr, .. }
            | CftSchemaCheckExprKind::Is { expr, .. }
            | CftSchemaCheckExprKind::Unary { expr, .. } => self.visit_expr(expr)?,
            CftSchemaCheckExprKind::Index { expr, index }
            | CftSchemaCheckExprKind::SafeIndex { expr, index } => {
                self.visit_expr(expr)?;
                self.visit_expr(index)?;
            }
            CftSchemaCheckExprKind::Coalesce { lhs, rhs }
            | CftSchemaCheckExprKind::BinOp { lhs, rhs, .. } => {
                self.visit_expr(lhs)?;
                self.visit_expr(rhs)?;
            }
            CftSchemaCheckExprKind::Call { args, .. } => {
                for arg in args {
                    self.visit_expr(arg)?;
                }
            }
            CftSchemaCheckExprKind::MethodCall {
                receiver, args, ..
            } => {
                self.visit_expr(receiver)?;
                for arg in args {
                    self.visit_expr(arg)?;
                }
            }
            CftSchemaCheckExprKind::CmpChain { first, rest } => {
                self.visit_expr(first)?;
                for (_, expr) in rest {
                    self.visit_expr(expr)?;
                }
            }
        }
        Ok(())
    }

    fn visit_name(&mut self, _name: &str) -> Result<(), Self::Error> {
        Ok(())
    }

    fn visit_records(&mut self, _type_name: &crate::TypeName) -> Result<(), Self::Error> {
        Ok(())
    }

    fn visit_segments(
        &mut self,
        segments: &[CftSchemaCheckFormatSegment],
    ) -> Result<(), Self::Error> {
        for segment in segments {
            match segment {
                CftSchemaCheckFormatSegment::Text(_, _) => {}
                CftSchemaCheckFormatSegment::Expr(expr) => self.visit_expr(expr)?,
            }
        }
        Ok(())
    }
}
