use super::{CheckTypeAnalyzer, InferredType};
use crate::diagnostics::CftErrorCode;
use crate::schema::CftValueType;
use crate::syntax::ast::{BinOp, CheckExpr, CheckExprKind, TypePredicate};
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct ConditionFacts {
    bindings: HashMap<String, InferredType>,
    refinements: HashMap<Vec<String>, InferredType>,
}

impl CheckTypeAnalyzer<'_, '_> {
    // 只有肯定成立的模式与 && 链传播绑定；其他运算不能向外泄漏局部变量。
    pub(super) fn check_condition(&mut self, expr: &CheckExpr) -> (InferredType, ConditionFacts) {
        let mut facts = ConditionFacts::default();
        match &expr.kind {
            CheckExprKind::BinOp {
                op: BinOp::And,
                lhs,
                rhs,
            } => {
                let (lhs_ty, mut left) = self.check_condition(lhs);
                self.push_condition_scope(&left);
                let (rhs_ty, right) = self.check_condition(rhs);
                self.pop_condition_scope();
                left.refinements
                    .retain(|path, _| !right.bindings.contains_key(&path[0]));
                left.bindings.extend(right.bindings);
                left.refinements.extend(right.refinements);
                (
                    self.check_binop(BinOp::And, &lhs_ty, &rhs_ty, expr.span),
                    left,
                )
            }
            CheckExprKind::Is {
                expr: inner,
                predicate,
            } => {
                let ty = self.check_expr_value(inner);
                self.check_is(&ty, predicate, expr.span);
                match predicate {
                    TypePredicate::Some { binding, .. } => {
                        if crate::is_cft_reserved_identifier(&binding.name) {
                            self.diag(
                                CftErrorCode::ReservedIdentifier,
                                binding.span,
                                format!("`{}` is a reserved identifier", binding.name),
                            );
                        }
                        if self
                            .locals
                            .iter()
                            .any(|scope| scope.contains_key(&binding.name))
                        {
                            self.diag(
                                CftErrorCode::InvalidQuantifierBindings,
                                binding.span,
                                format!(
                                    "pattern binding `{}` shadows an outer binding",
                                    binding.name
                                ),
                            );
                        }
                        if let InferredType::Value(CftValueType::Option(value)) = ty {
                            facts
                                .bindings
                                .insert(binding.name.clone(), InferredType::Value(*value));
                        }
                    }
                    TypePredicate::Type(name) => {
                        let mut target = self
                            .schema
                            .resolved_aliases
                            .get(&name.name)
                            .and_then(InferredType::object_name)
                            .cloned()
                            .unwrap_or_else(|| crate::TypeName::from_validated(name.name.clone()));
                        if let InferredType::Value(
                            CftValueType::Object(current) | CftValueType::RecordRef(current),
                        ) = &ty
                        {
                            if self
                                .schema
                                .inheritance_chains
                                .get(current.as_str())
                                .is_some_and(|chain| {
                                    chain.iter().any(|ancestor| ancestor == target.as_str())
                                })
                            {
                                target = current.clone();
                            }
                        }
                        let narrowed = match ty {
                            InferredType::Value(CftValueType::Object(_)) => {
                                Some(CftValueType::Object(target))
                            }
                            InferredType::Value(CftValueType::RecordRef(_)) => {
                                Some(CftValueType::RecordRef(target))
                            }
                            _ => None,
                        };
                        if let (Some(path), Some(ty)) = (expression_path(inner), narrowed) {
                            facts.refinements.insert(path, InferredType::Value(ty));
                        }
                    }
                }
                (InferredType::bool(), facts)
            }
            _ => (self.check_expr_value(expr), facts),
        }
    }

    pub(super) fn push_condition_scope(&mut self, facts: &ConditionFacts) {
        self.locals.push(facts.bindings.clone());
        self.refinements.push(facts.refinements.clone());
    }

    pub(super) fn pop_condition_scope(&mut self) {
        self.locals.pop();
        self.refinements.pop();
    }

    pub(super) fn refined_type(&self, path: &[String]) -> Option<&InferredType> {
        for (locals, refinements) in self.locals.iter().zip(&self.refinements).rev() {
            if let Some(ty) = refinements.get(path) {
                return Some(ty);
            }
            // 新绑定遮蔽同名字段时，外层字段的类型判断不能作用于该局部变量。
            if locals.contains_key(&path[0]) {
                return None;
            }
        }
        None
    }
}

pub(super) fn expression_path(expr: &CheckExpr) -> Option<Vec<String>> {
    match &expr.kind {
        CheckExprKind::Name(name) => Some(vec![name.clone()]),
        CheckExprKind::Field { expr, name } => {
            let mut path = expression_path(expr)?;
            path.push(name.name.clone());
            Some(path)
        }
        _ => None,
    }
}
