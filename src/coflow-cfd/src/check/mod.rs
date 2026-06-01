mod diagnostics;
mod runtime;
mod scope;

use self::diagnostics::{cond_failed, describe_expr, eval_error};
use self::runtime::{compare_values, EvalValue};
use self::scope::CheckScope;
use crate::ast::{
    BinOp, CheckExpr, CheckExprKind, CondStmt, QuantifierKind, TypePredicate, UnaryOp,
};
use crate::container::{CfdContainer, CfdResult, ModuleId};
use crate::error::{AllFailedItem, CheckError};
use crate::value::{CfdValue, CfdValueRef};
use std::collections::BTreeMap;

pub(crate) fn run(container: &CfdContainer, result: &CfdResult) -> Vec<CheckError> {
    CheckRunner::new(container, result).run()
}

struct CheckRunner<'a> {
    container: &'a CfdContainer,
    result: &'a CfdResult,
    errors: Vec<CheckError>,
}

impl<'a> CheckRunner<'a> {
    fn new(container: &'a CfdContainer, result: &'a CfdResult) -> Self {
        Self {
            container,
            result,
            errors: Vec::new(),
        }
    }

    fn run(mut self) -> Vec<CheckError> {
        self.run_top_level_checks();
        self.errors
    }

    fn run_top_level_checks(&mut self) {
        for (module_id, module_result) in self.result.modules() {
            let Some(module) = self.container.modules.get(module_id) else {
                continue;
            };
            let locals = module_result
                .values()
                .map(|(name, value)| (name.to_string(), EvalValue::Ref(value)))
                .collect::<BTreeMap<_, _>>();
            for block in &module.ast.checks {
                let mut scope = CheckScope::new();
                scope.push(locals.clone());
                self.eval_block(block.stmts.as_slice(), module_id, &mut scope, module_id.as_str());
            }
        }
    }

    fn eval_block(
        &mut self,
        stmts: &[CondStmt],
        module: &ModuleId,
        scope: &mut CheckScope,
        context: &str,
    ) {
        for stmt in stmts {
            if !self.eval_stmt(stmt, module, scope, context) {
                break;
            }
        }
    }

    fn eval_stmt(
        &mut self,
        stmt: &CondStmt,
        module: &ModuleId,
        scope: &mut CheckScope,
        context: &str,
    ) -> bool {
        match stmt {
            CondStmt::Expr(expr) => self.eval_condition(expr, module, scope, context),
            CondStmt::Quantifier {
                kind,
                binding,
                collection,
                body,
                span,
            } => {
                let Ok(value) = self.eval_expr(collection, scope) else {
                    self.errors.push(eval_error(
                        format!(
                            "failed to evaluate {} collection `{}`",
                            quantifier_name(*kind),
                            describe_expr(collection)
                        ),
                        module,
                        context,
                        collection.span,
                    ));
                    return false;
                };
                let Some(entries) = self.collection_entries(value, collection, module, context, *kind) else {
                    return false;
                };
                let total = entries.len();
                let mut failed = Vec::new();
                let mut passed = Vec::new();
                for (label, item) in entries {
                    scope.push(BTreeMap::from([(
                        binding.to_string(),
                        EvalValue::Ref(item),
                    )]));
                    let item_context = format!("{context} {binding}{label}");
                    let before = self.errors.len();
                    self.eval_block(body, module, scope, &item_context);
                    scope.pop();
                    let errors = self.errors.split_off(before);
                    if errors.is_empty() {
                        passed.push(format!("{binding}{label}"));
                        if *kind == QuantifierKind::Any {
                            return true;
                        }
                    } else {
                        failed.push(AllFailedItem {
                            key: format!("{binding}{label}"),
                            errors,
                        });
                    }
                }
                match kind {
                    QuantifierKind::All if !failed.is_empty() => {
                        self.errors.push(diagnostics::all_failed(
                            format!("all {binding} in {}", describe_expr(collection)),
                            module,
                            context,
                            total,
                            failed,
                            *span,
                        ));
                    }
                    QuantifierKind::Any => self.errors.push(cond_failed(
                        format!("any {binding} in {}", describe_expr(collection)),
                        module,
                        context,
                        *span,
                    )),
                    QuantifierKind::None if !passed.is_empty() => {
                        let failed = passed
                            .into_iter()
                            .map(|key| AllFailedItem {
                                key,
                                errors: Vec::new(),
                            })
                            .collect();
                        self.errors.push(diagnostics::all_failed(
                            format!("none {binding} in {}", describe_expr(collection)),
                            module,
                            context,
                            total,
                            failed,
                            *span,
                        ));
                    }
                    _ => {}
                }
                true
            }
        }
    }

    fn eval_condition(
        &mut self,
        expr: &CheckExpr,
        module: &ModuleId,
        scope: &mut CheckScope,
        context: &str,
    ) -> bool {
        match self.eval_expr(expr, scope) {
            Ok(EvalValue::Bool(true)) => true,
            Ok(EvalValue::Bool(false)) => {
                self.errors
                    .push(cond_failed(describe_expr(expr), module, context, expr.span));
                true
            }
            Ok(other) => {
                self.errors.push(eval_error(
                    format!("condition must be bool, found {}", other.type_name()),
                    module,
                    context,
                    expr.span,
                ));
                false
            }
            Err(message) => {
                self.errors
                    .push(eval_error(message, module, context, expr.span));
                false
            }
        }
    }

    fn collection_entries(
        &mut self,
        value: EvalValue,
        collection: &CheckExpr,
        module: &ModuleId,
        context: &str,
        kind: QuantifierKind,
    ) -> Option<Vec<(String, CfdValueRef)>> {
        match value {
            EvalValue::Ref(value) => match &*value.borrow() {
                CfdValue::Array(items) => Some(
                    items
                        .iter()
                        .enumerate()
                        .map(|(index, value)| (format!("[{index}]"), value.clone()))
                        .collect(),
                ),
                CfdValue::Dict(items) => Some(
                    items
                        .iter()
                        .enumerate()
                        .map(|(index, (key, value))| {
                            (
                                format!("[{index}]"),
                                CfdValueRef::new(CfdValue::Object {
                                    type_name: None,
                                    fields: BTreeMap::from([
                                        ("key".to_string(), key.clone()),
                                        ("value".to_string(), value.clone()),
                                    ]),
                                }),
                            )
                        })
                        .collect(),
                ),
                other => {
                    self.errors.push(eval_error(
                        format!(
                            "{} collection must be array or dict, found {}",
                            quantifier_name(kind),
                            other.type_name()
                        ),
                        module,
                        context,
                        collection.span,
                    ));
                    None
                }
            },
            other => {
                self.errors.push(eval_error(
                    format!(
                        "{} collection must be array or dict, found {}",
                        quantifier_name(kind),
                        other.type_name()
                    ),
                    module,
                    context,
                    collection.span,
                ));
                None
            }
        }
    }

    fn eval_expr(&self, expr: &CheckExpr, scope: &CheckScope) -> Result<EvalValue, String> {
        match &expr.kind {
            CheckExprKind::Null => Ok(EvalValue::Null),
            CheckExprKind::Int(value) => Ok(EvalValue::Int(*value)),
            CheckExprKind::Float(value) => Ok(EvalValue::Float(*value)),
            CheckExprKind::Bool(value) => Ok(EvalValue::Bool(*value)),
            CheckExprKind::Str(value) => Ok(EvalValue::String(value.clone())),
            CheckExprKind::Name(name) => scope
                .lookup(name)
                .ok_or_else(|| format!("unknown name `{name}`")),
            CheckExprKind::Field { expr, name } => {
                let value = self.eval_expr(expr, scope)?.into_ref()?;
                let value = unwrap_union_ref(value);
                let borrowed = value.borrow();
                let CfdValue::Object { fields, .. } = &*borrowed else {
                    return Err(format!(
                        "cannot select field `{name}` from {}",
                        borrowed.type_name()
                    ));
                };
                fields
                    .get(name)
                    .cloned()
                    .map(EvalValue::Ref)
                    .ok_or_else(|| format!("missing field `{name}`"))
            }
            CheckExprKind::Index { expr, index } => {
                let base = self.eval_expr(expr, scope)?.into_ref()?;
                let base = unwrap_union_ref(base);
                let index = self.eval_expr(index, scope)?;
                let borrowed = base.borrow();
                match &*borrowed {
                    CfdValue::Array(items) => {
                        let index = index.into_i64()?;
                        let index = usize::try_from(index)
                            .map_err(|_| format!("array index `{index}` is out of bounds"))?;
                        items
                            .get(index)
                            .cloned()
                            .map(EvalValue::Ref)
                            .ok_or_else(|| format!("array index `{index}` is out of bounds"))
                    }
                    CfdValue::Dict(entries) => entries
                        .iter()
                        .find(|(key, _)| runtime::eval_value_equals_ref(&index, key))
                        .map(|(_, value)| EvalValue::Ref(value.clone()))
                        .ok_or_else(|| "dict key not found".to_string()),
                    other => Err(format!("cannot index {}", other.type_name())),
                }
            }
            CheckExprKind::Is { expr, predicate } => {
                let value = self.eval_expr(expr, scope)?;
                match predicate {
                    TypePredicate::Null => Ok(EvalValue::Bool(matches!(value, EvalValue::Null))),
                    TypePredicate::Type(ty) => {
                        let Ok(value) = value.into_ref() else {
                            return Ok(EvalValue::Bool(false));
                        };
                        let value = unwrap_union_ref(value);
                        let borrowed = value.borrow();
                        let CfdValue::Object {
                            type_name: Some(actual),
                            ..
                        } = &*borrowed
                        else {
                            return Ok(EvalValue::Bool(false));
                        };
                        let crate::ast::TypeName::Local(name) = ty;
                        Ok(EvalValue::Bool(actual.name == *name))
                    }
                }
            }
            CheckExprKind::Call { name, args } => self.eval_call(name, args, scope),
            CheckExprKind::BinOp { op, lhs, rhs } => self.eval_bin(*op, lhs, rhs, scope),
            CheckExprKind::Unary { op, expr } => self.eval_unary(*op, expr, scope),
            CheckExprKind::CmpChain { first, rest } => {
                let mut lhs = self.eval_expr(first, scope)?;
                for (op, rhs_expr) in rest {
                    let rhs = self.eval_expr(rhs_expr, scope)?;
                    if !compare_values(*op, &lhs, &rhs)? {
                        return Ok(EvalValue::Bool(false));
                    }
                    lhs = rhs;
                }
                Ok(EvalValue::Bool(true))
            }
        }
    }

    fn eval_call(
        &self,
        name: &str,
        args: &[CheckExpr],
        scope: &CheckScope,
    ) -> Result<EvalValue, String> {
        match name {
            "len" => {
                expect_arity(name, args, 1)?;
                let value = self.eval_expr(&args[0], scope)?.into_ref()?;
                let len = match &*value.borrow() {
                    CfdValue::Array(items) => items.len(),
                    CfdValue::Dict(entries) => entries.len(),
                    other => {
                        return Err(format!(
                            "len() expects array or dict, found {}",
                            other.type_name()
                        ))
                    }
                };
                i64::try_from(len)
                    .map(EvalValue::Int)
                    .map_err(|_| "len() result is out of range".to_string())
            }
            "contains" => {
                expect_arity(name, args, 2)?;
                let collection = self.eval_expr(&args[0], scope)?.into_ref()?;
                let needle = self.eval_expr(&args[1], scope)?;
                let contains = match &*collection.borrow() {
                    CfdValue::Array(items) => items
                        .iter()
                        .any(|item| runtime::eval_value_equals_ref(&needle, item)),
                    CfdValue::Dict(entries) => entries
                        .iter()
                        .any(|(key, _)| runtime::eval_value_equals_ref(&needle, key)),
                    other => {
                        return Err(format!(
                            "contains() expects array or dict, found {}",
                            other.type_name()
                        ));
                    }
                };
                Ok(EvalValue::Bool(contains))
            }
            _ => Err(format!("unknown builtin function `{name}`")),
        }
    }

    fn eval_unary(
        &self,
        op: UnaryOp,
        expr: &CheckExpr,
        scope: &CheckScope,
    ) -> Result<EvalValue, String> {
        let value = self.eval_expr(expr, scope)?;
        match op {
            UnaryOp::Not => Ok(EvalValue::Bool(!value.into_bool()?)),
            UnaryOp::BitNot => Ok(EvalValue::Int(!value.into_i64()?)),
            UnaryOp::Neg => match value {
                EvalValue::Int(value) => Ok(EvalValue::Int(-value)),
                EvalValue::Float(value) => Ok(EvalValue::Float(-value)),
                other => Err(format!(
                    "expected numeric value, found {}",
                    other.type_name()
                )),
            },
        }
    }

    fn eval_bin(
        &self,
        op: BinOp,
        lhs: &CheckExpr,
        rhs: &CheckExpr,
        scope: &CheckScope,
    ) -> Result<EvalValue, String> {
        match op {
            BinOp::Or => {
                let lhs = self.eval_expr(lhs, scope)?.into_bool()?;
                if lhs {
                    return Ok(EvalValue::Bool(true));
                }
                Ok(EvalValue::Bool(self.eval_expr(rhs, scope)?.into_bool()?))
            }
            BinOp::And => {
                let lhs = self.eval_expr(lhs, scope)?.into_bool()?;
                if !lhs {
                    return Ok(EvalValue::Bool(false));
                }
                Ok(EvalValue::Bool(self.eval_expr(rhs, scope)?.into_bool()?))
            }
            BinOp::BitOr => Ok(EvalValue::Int(
                self.eval_expr(lhs, scope)?.into_i64()? | self.eval_expr(rhs, scope)?.into_i64()?,
            )),
            BinOp::BitXor => Ok(EvalValue::Int(
                self.eval_expr(lhs, scope)?.into_i64()? ^ self.eval_expr(rhs, scope)?.into_i64()?,
            )),
            BinOp::BitAnd => Ok(EvalValue::Int(
                self.eval_expr(lhs, scope)?.into_i64()? & self.eval_expr(rhs, scope)?.into_i64()?,
            )),
            BinOp::Add => runtime::numeric_bin(
                self.eval_expr(lhs, scope)?,
                self.eval_expr(rhs, scope)?,
                i64::checked_add,
                |a, b| a + b,
                "addition",
            ),
            BinOp::Sub => runtime::numeric_bin(
                self.eval_expr(lhs, scope)?,
                self.eval_expr(rhs, scope)?,
                i64::checked_sub,
                |a, b| a - b,
                "subtraction",
            ),
            BinOp::Mul => runtime::numeric_bin(
                self.eval_expr(lhs, scope)?,
                self.eval_expr(rhs, scope)?,
                i64::checked_mul,
                |a, b| a * b,
                "multiplication",
            ),
            BinOp::Div => {
                let lhs = self.eval_expr(lhs, scope)?.into_f64()?;
                let rhs = self.eval_expr(rhs, scope)?.into_f64()?;
                if rhs == 0.0 {
                    return Err("division by zero".to_string());
                }
                Ok(EvalValue::Float(lhs / rhs))
            }
            BinOp::IntDiv => {
                let lhs = self.eval_expr(lhs, scope)?.into_i64()?;
                let rhs = self.eval_expr(rhs, scope)?.into_i64()?;
                if rhs == 0 {
                    return Err("division by zero".to_string());
                }
                lhs.checked_div(rhs)
                    .map(EvalValue::Int)
                    .ok_or_else(|| "integer division overflow".to_string())
            }
            BinOp::Mod => {
                let lhs = self.eval_expr(lhs, scope)?.into_i64()?;
                let rhs = self.eval_expr(rhs, scope)?.into_i64()?;
                if rhs == 0 {
                    return Err("modulo by zero".to_string());
                }
                lhs.checked_rem(rhs)
                    .map(EvalValue::Int)
                    .ok_or_else(|| "integer remainder overflow".to_string())
            }
            BinOp::Pow => {
                let lhs = self.eval_expr(lhs, scope)?.into_f64()?;
                let rhs = self.eval_expr(rhs, scope)?.into_f64()?;
                Ok(EvalValue::Float(lhs.powf(rhs)))
            }
            BinOp::Shl => Ok(EvalValue::Int(
                self.eval_expr(lhs, scope)?.into_i64()? << runtime::shift_amount(self.eval_expr(rhs, scope)?.into_i64()?)?,
            )),
            BinOp::Shr => Ok(EvalValue::Int(
                self.eval_expr(lhs, scope)?.into_i64()? >> runtime::shift_amount(self.eval_expr(rhs, scope)?.into_i64()?)?,
            )),
        }
    }
}

fn quantifier_name(kind: QuantifierKind) -> &'static str {
    match kind {
        QuantifierKind::All => "all",
        QuantifierKind::Any => "any",
        QuantifierKind::None => "none",
    }
}

fn expect_arity(name: &str, args: &[CheckExpr], expected: usize) -> Result<(), String> {
    if args.len() == expected {
        Ok(())
    } else {
        Err(format!(
            "`{name}` expects {expected} argument{}, found {}",
            if expected == 1 { "" } else { "s" },
            args.len()
        ))
    }
}

fn unwrap_union_ref(mut value: CfdValueRef) -> CfdValueRef {
    while let Some(inner) = {
        let borrowed = value.borrow();
        match &*borrowed {
            CfdValue::Union { value, .. } => Some(value.clone()),
            _ => None,
        }
    } {
        value = inner;
    }
    value
}
