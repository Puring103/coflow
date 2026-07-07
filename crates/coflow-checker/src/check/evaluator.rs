use super::access;
use super::builtin_calls::{self, CallSignature, CallSignatureError, CallTarget};
use super::builtin_values;
use super::builtins::Builtin;
use super::deps::DependencyCollector;
use super::diagnostics::{
    cmp_op_str, dimension_lookup_error_message, format_cfd_path_for_message,
    format_value_for_message, one_line_message, render_expr, render_stmt, CheckExplanation,
};
use super::enum_values;
use super::fields;
use super::ops::{self, OpsResult};
use super::quantifiers;
use super::value::{
    comparable_key, CheckValue, LocatedCheckValue,
};
use crate::DimensionCheckContext;
use coflow_cft::{
    CftContainer, CftSchemaBinOp, CftSchemaCheckBlock, CftSchemaCheckExpr, CftSchemaCheckExprKind,
    CftSchemaCheckStmt, CftSchemaCmpOp, CftSchemaQuantifierKind, CftSchemaTypePredicate,
    CftSchemaUnaryOp, CftSchemaView,
};
use coflow_data_model::{
    CfdDataModel, CfdDiagnostic, CfdEnumValue, CfdErrorCode, CfdPath, CfdRecordId,
};
use std::collections::BTreeMap;

use super::value::CheckRecordRef;

pub(super) struct CheckEvaluator<'a> {
    schema: &'a CftSchemaView,
    source_schema: &'a CftContainer,
    model: &'a CfdDataModel,
    root_record: Option<CfdRecordId>,
    root_path: CfdPath,
    current: CheckValue,
    scopes: Vec<BTreeMap<String, LocatedCheckValue>>,
    contexts: Vec<String>,
    pub(super) diagnostics: Vec<CfdDiagnostic>,
    deps: DependencyCollector,
    pub(super) dimension_context: Option<DimensionCheckContext>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EvalFlow {
    Continue,
    Skipped,
    HardStop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EvalAbort {
    Skipped,
    Error,
}

type EvalResult<T> = Result<T, EvalAbort>;

impl<'a> CheckEvaluator<'a> {
    pub(super) fn new(
        schema: &'a CftSchemaView,
        source_schema: &'a CftContainer,
        model: &'a CfdDataModel,
        root_record: Option<CfdRecordId>,
        root_path: CfdPath,
        current: CheckValue,
        mut deps: DependencyCollector,
    ) -> Self {
        let initial_top = match &current {
            CheckValue::Record(CheckRecordRef::Top(id)) => Some(*id),
            _ => None,
        };
        if let Some(record_id) = initial_top {
            deps.note_read_from(record_id);
        }
        Self {
            schema,
            source_schema,
            model,
            root_record,
            root_path,
            current,
            scopes: Vec::new(),
            contexts: Vec::new(),
            diagnostics: Vec::new(),
            deps,
            dimension_context: None,
        }
    }

    pub(super) fn into_outputs(self) -> (Vec<CfdDiagnostic>, DependencyCollector) {
        (self.diagnostics, self.deps)
    }

    pub(super) fn note_read_from(&mut self, target: CfdRecordId) {
        self.deps.note_read_from(target);
    }

    fn from_ops<T>(&mut self, result: OpsResult<T>) -> EvalResult<T> {
        result.map_err(|err| {
            let (code, path, message) = err.into_parts();
            self.diag_at(code, path, message);
            EvalAbort::Error
        })
    }

    fn apply_dimension_variant(
        &mut self,
        record: &CheckRecordRef,
        field_name: &str,
        located: &mut LocatedCheckValue,
    ) -> EvalResult<()> {
        let Some(context) = self.dimension_context.as_ref() else {
            return Ok(());
        };
        if !matches!(record, CheckRecordRef::Top(_)) {
            return Ok(());
        }
        let context_dimension = context.dimension.clone();
        let Some(variant) = context.variant.clone() else {
            return Ok(());
        };
        let Some(actual_type) = record.actual_type(self.model) else {
            return Ok(());
        };
        let Some(field) = self.schema.dimension_field(actual_type, field_name) else {
            return Ok(());
        };
        if field.dimension != context_dimension {
            return Ok(());
        }
        let CheckRecordRef::Top(source_record_id) = record else {
            return Ok(());
        };
        let resolved = match self.model.dimension_field_value(
            self.source_schema,
            *source_record_id,
            field_name,
            &context_dimension,
            &variant,
        ) {
            Ok(resolved) => resolved,
            Err(err) => {
                self.diag_at(
                    CfdErrorCode::CheckEvalTypeError,
                    located.path.clone(),
                    dimension_lookup_error_message(actual_type, field_name, &variant, err),
                );
                return Err(EvalAbort::Error);
            }
        };
        if let Some(record_id) = resolved.record {
            self.note_read_from(record_id);
        }
        let path = located.path.clone();
        located.value = CheckValue::from_cfd_value_with_path(
            resolved.value,
            resolved.field_type.as_ref(),
            path.clone(),
            self.model,
            resolved.record,
        );
        if matches!(located.value, CheckValue::Null) {
            return Err(EvalAbort::Skipped);
        }
        located.path = path;
        Ok(())
    }

    pub(super) fn eval_check_block(&mut self, check: &CftSchemaCheckBlock) -> EvalFlow {
        self.eval_stmts(&check.stmts)
    }

    fn eval_stmts(&mut self, stmts: &[CftSchemaCheckStmt]) -> EvalFlow {
        let mut skipped = false;
        for stmt in stmts {
            match self.eval_stmt(stmt) {
                EvalFlow::Continue => {}
                EvalFlow::Skipped => skipped = true,
                EvalFlow::HardStop => return EvalFlow::HardStop,
            }
        }
        if skipped {
            EvalFlow::Skipped
        } else {
            EvalFlow::Continue
        }
    }

    fn eval_stmt(&mut self, stmt: &CftSchemaCheckStmt) -> EvalFlow {
        match stmt {
            CftSchemaCheckStmt::Expr(expr) => match self.eval_expr_explained(expr) {
                Ok((value, _)) if matches!(value.value, CheckValue::Bool(true)) => {
                    EvalFlow::Continue
                }
                Ok((value, explanation)) if matches!(value.value, CheckValue::Bool(false)) => {
                    let explanation = self
                        .explain_false_expr(expr, &value)
                        .unwrap_or_else(|| {
                            let mut fallback = CheckExplanation::new(
                                CfdErrorCode::CheckFailed,
                                render_expr(expr),
                                value.path.clone(),
                            );
                            if let Some(detail) = explanation {
                                fallback = fallback.with_actual(detail);
                            }
                            fallback
                        })
                        .with_context(&self.contexts);
                    let message = explanation.message();
                    self.diag_at(explanation.code, explanation.path, message);
                    EvalFlow::Continue
                }
                Ok((value, _)) => {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        value.path,
                        "check 表达式没有求值为 bool",
                    );
                    EvalFlow::HardStop
                }
                Err(EvalAbort::Skipped) => EvalFlow::Skipped,
                Err(EvalAbort::Error) => EvalFlow::HardStop,
            },
            CftSchemaCheckStmt::When {
                condition, body, ..
            } => match self.eval_expr(condition) {
                Ok(value) if matches!(value.value, CheckValue::Bool(true)) => {
                    self.contexts
                        .push(format!("在 when {} 内", render_expr(condition)));
                    let flow = self.eval_stmts(body);
                    let _ = self.contexts.pop();
                    flow
                }
                Ok(value) if matches!(value.value, CheckValue::Bool(false)) => EvalFlow::Continue,
                Ok(value) => {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        value.path,
                        "when 条件没有求值为 bool",
                    );
                    EvalFlow::HardStop
                }
                Err(EvalAbort::Skipped) => EvalFlow::Skipped,
                Err(EvalAbort::Error) => EvalFlow::HardStop,
            },
            CftSchemaCheckStmt::Quantifier {
                kind,
                binding,
                collection,
                body,
                ..
            } => {
                let collection_expr = collection;
                let collection_value = match self.eval_expr(collection_expr) {
                    Ok(value) => value,
                    Err(EvalAbort::Skipped) => return EvalFlow::Skipped,
                    Err(EvalAbort::Error) => return EvalFlow::HardStop,
                };
                let items = match self.from_ops(quantifiers::quantifier_items(collection_value)) {
                    Ok(items) => items,
                    Err(EvalAbort::Skipped) => return EvalFlow::Skipped,
                    Err(EvalAbort::Error) => return EvalFlow::HardStop,
                };
                self.eval_quantifier(*kind, binding, &items, body, collection_expr, stmt)
            }
        }
    }

    fn eval_quantifier(
        &mut self,
        kind: CftSchemaQuantifierKind,
        binding: &str,
        items: &[LocatedCheckValue],
        body: &[CftSchemaCheckStmt],
        collection: &CftSchemaCheckExpr,
        stmt: &CftSchemaCheckStmt,
    ) -> EvalFlow {
        let quantifier_diagnostic_start = self.diagnostics.len();
        let mut matched = 0_usize;
        let mut any_failures = Vec::new();
        let mut none_match_paths = Vec::new();
        for item in items {
            let diagnostic_start = self.diagnostics.len();
            let mut scope = BTreeMap::new();
            scope.insert(binding.to_string(), item.clone());
            self.scopes.push(scope);
            let item_context = format!(
                "绑定 {binding} 位于 {}",
                item.path
                    .as_ref()
                    .map_or_else(|| render_expr(collection), format_cfd_path_for_message)
            );
            self.contexts.push(item_context);
            let flow = self.eval_stmts(body);
            let passed = flow == EvalFlow::Continue && self.diagnostics.len() == diagnostic_start;
            let _ = self.contexts.pop();
            let _ = self.scopes.pop();

            match flow {
                EvalFlow::Continue => {}
                EvalFlow::Skipped => {
                    self.diagnostics.truncate(diagnostic_start);
                    continue;
                }
                EvalFlow::HardStop => return EvalFlow::HardStop,
            }

            match kind {
                CftSchemaQuantifierKind::All => {}
                CftSchemaQuantifierKind::Any => {
                    let trial_failures = self.diagnostics.split_off(diagnostic_start);
                    if !passed {
                        any_failures.extend(trial_failures);
                    }
                }
                CftSchemaQuantifierKind::None => {
                    self.diagnostics.truncate(diagnostic_start);
                    if passed {
                        none_match_paths.push(item.path.clone());
                    }
                }
            }

            if passed {
                matched += 1;
            }
        }

        match kind {
            CftSchemaQuantifierKind::All => {
                for diagnostic in &mut self.diagnostics[quantifier_diagnostic_start..] {
                    if diagnostic.code != CfdErrorCode::CheckBoolExpectedTrue
                        && diagnostic.code != CfdErrorCode::CheckComparisonFailed
                        && diagnostic.code != CfdErrorCode::CheckNegationFailed
                        && diagnostic.code != CfdErrorCode::CheckAndFailed
                        && diagnostic.code != CfdErrorCode::CheckOrFailed
                        && diagnostic.code != CfdErrorCode::CheckTypePredicateFailed
                        && diagnostic.code != CfdErrorCode::CheckNullPredicateFailed
                        && diagnostic.code != CfdErrorCode::CheckContainsFailed
                        && diagnostic.code != CfdErrorCode::CheckUniqueFailed
                        && diagnostic.code != CfdErrorCode::CheckMatchesFailed
                        && diagnostic.code != CfdErrorCode::CheckFailed
                    {
                        continue;
                    }
                    diagnostic.code = CfdErrorCode::CheckAllQuantifierFailed;
                    diagnostic.message =
                        format!("校验失败: {}\n{}", render_stmt(stmt), diagnostic.message);
                }
            }
            CftSchemaQuantifierKind::Any if matched == 0 => {
                let mut context = self.contexts.clone();
                if let Some(diagnostic) = any_failures.first() {
                    context.push(format!(
                        "失败样例: {}",
                        one_line_message(&diagnostic.message)
                    ));
                }
                let explanation = CheckExplanation::new(
                    CfdErrorCode::CheckAnyQuantifierFailed,
                    render_stmt(stmt),
                    items.first().and_then(|item| item.path.clone()),
                )
                .with_actual(format!("0 / {} 个元素匹配", items.len()))
                .with_expected("至少 1 个元素满足")
                .with_context(&context);
                self.diag_at(
                    explanation.code,
                    explanation.path.clone(),
                    explanation.message(),
                );
            }
            CftSchemaQuantifierKind::Any => {}
            CftSchemaQuantifierKind::None if matched > 0 => {
                for path in none_match_paths {
                    let explanation = CheckExplanation::new(
                        CfdErrorCode::CheckNoneQuantifierFailed,
                        render_stmt(stmt),
                        path.clone(),
                    )
                    .with_actual(format!(
                        "{} 已匹配",
                        path.as_ref()
                            .map_or_else(|| render_expr(collection), format_cfd_path_for_message)
                    ))
                    .with_expected("没有元素满足")
                    .with_context(&self.contexts);
                    let message = explanation.message();
                    self.diag_at(explanation.code, explanation.path, message);
                }
            }
            CftSchemaQuantifierKind::None => {}
        }
        EvalFlow::Continue
    }

    /// Evaluates a top-level check expression and, if it produced `false`,
    /// returns a human-readable detail describing *why* it failed (which side
    /// of a comparison was what value, etc). Returns `None` when the
    /// expression isn't one of the shapes we know how to explain.
    fn eval_expr_explained(
        &mut self,
        expr: &CftSchemaCheckExpr,
    ) -> EvalResult<(LocatedCheckValue, Option<String>)> {
        match &expr.kind {
            CftSchemaCheckExprKind::CmpChain { first, rest } => {
                // Re-implement CmpChain so we can capture the failing pair.
                let mut lhs = self.eval_expr(first)?;
                for (op, rhs_expr) in rest {
                    let rhs = self.eval_expr(rhs_expr)?;
                    let path = lhs.path.clone().or_else(|| rhs.path.clone());
                    if !self.from_ops(ops::compare(
                        *op,
                        &lhs.value,
                        &rhs.value,
                        rhs.path.clone(),
                    ))? {
                        let detail = format!(
                            "{} {} {}",
                            format_value_for_message(&lhs.value),
                            cmp_op_str(*op),
                            format_value_for_message(&rhs.value),
                        );
                        return Ok((
                            LocatedCheckValue::new(CheckValue::Bool(false), path),
                            Some(detail),
                        ));
                    }
                    lhs = rhs;
                }
                Ok((LocatedCheckValue::value(CheckValue::Bool(true)), None))
            }
            CftSchemaCheckExprKind::Unary {
                op: CftSchemaUnaryOp::Not,
                expr: inner,
            } => {
                let inner_val = self.eval_expr(inner)?;
                if matches!(inner_val.value, CheckValue::Bool(true)) {
                    let detail = format!(
                        "期望 !{}，但内部表达式为 true",
                        format_value_for_message(&inner_val.value),
                    );
                    return Ok((
                        LocatedCheckValue::new(CheckValue::Bool(false), inner_val.path),
                        Some(detail),
                    ));
                }
                self.eval_unary(CftSchemaUnaryOp::Not, inner_val)
                    .map(|v| (v, None))
            }
            CftSchemaCheckExprKind::BinOp {
                op: CftSchemaBinOp::And,
                lhs,
                rhs,
            } => {
                // Short-circuit AND: report which conjunct failed.
                let lv = self.eval_expr(lhs)?;
                if matches!(lv.value, CheckValue::Bool(false)) {
                    return Ok((
                        LocatedCheckValue::new(CheckValue::Bool(false), lv.path),
                        Some("左侧条件为 false".to_string()),
                    ));
                }
                let rv = self.eval_expr(rhs)?;
                if matches!(rv.value, CheckValue::Bool(false)) {
                    return Ok((
                        LocatedCheckValue::new(CheckValue::Bool(false), rv.path),
                        Some("右侧条件为 false".to_string()),
                    ));
                }
                let path = lv.path.or(rv.path);
                Ok((LocatedCheckValue::new(CheckValue::Bool(true), path), None))
            }
            _ => self.eval_expr(expr).map(|v| (v, None)),
        }
    }

    fn explain_false_expr(
        &mut self,
        expr: &CftSchemaCheckExpr,
        value: &LocatedCheckValue,
    ) -> Option<CheckExplanation> {
        let rendered = render_expr(expr);
        match &expr.kind {
            CftSchemaCheckExprKind::Name(name) => Some(
                CheckExplanation::new(
                    CfdErrorCode::CheckBoolExpectedTrue,
                    rendered,
                    value.path.clone(),
                )
                .with_actual(format!("{name} = false"))
                .with_expected("true"),
            ),
            CftSchemaCheckExprKind::Bool(false) => Some(
                CheckExplanation::new(
                    CfdErrorCode::CheckBoolExpectedTrue,
                    rendered,
                    value.path.clone(),
                )
                .with_actual("false")
                .with_expected("true"),
            ),
            CftSchemaCheckExprKind::Field { .. }
            | CftSchemaCheckExprKind::Index { .. }
            | CftSchemaCheckExprKind::Call { .. }
            | CftSchemaCheckExprKind::MethodCall { .. }
                if matches!(value.value, CheckValue::Bool(false)) =>
            {
                Some(self.explain_false_value_expr(expr, value, rendered))
            }
            CftSchemaCheckExprKind::Unary {
                op: CftSchemaUnaryOp::Not,
                expr: inner,
            } => Some(
                CheckExplanation::new(
                    CfdErrorCode::CheckNegationFailed,
                    rendered,
                    value.path.clone(),
                )
                .with_actual(format!("{} = true", render_expr(inner)))
                .with_expected("false"),
            ),
            CftSchemaCheckExprKind::BinOp {
                op: CftSchemaBinOp::And,
                lhs,
                rhs,
            } => {
                let left = self.eval_expr(lhs).ok();
                let right = self.eval_expr(rhs).ok();
                let failed = match (&left, &right) {
                    (Some(left), Some(right))
                        if matches!(left.value, CheckValue::Bool(false))
                            && matches!(right.value, CheckValue::Bool(false)) =>
                    {
                        format!("{} = false, {} = false", render_expr(lhs), render_expr(rhs))
                    }
                    (Some(left), _) if matches!(left.value, CheckValue::Bool(false)) => {
                        format!("{} = false", render_expr(lhs))
                    }
                    (_, Some(right)) if matches!(right.value, CheckValue::Bool(false)) => {
                        format!("{} = false", render_expr(rhs))
                    }
                    _ => "至少一个操作数为 false".to_string(),
                };
                Some(
                    CheckExplanation::new(
                        CfdErrorCode::CheckAndFailed,
                        rendered,
                        value.path.clone(),
                    )
                    .with_actual(failed)
                    .with_expected("两侧都为 true"),
                )
            }
            CftSchemaCheckExprKind::BinOp {
                op: CftSchemaBinOp::Or,
                lhs,
                rhs,
            } => Some(
                CheckExplanation::new(CfdErrorCode::CheckOrFailed, rendered, value.path.clone())
                    .with_actual(format!(
                        "{} = false, {} = false",
                        render_expr(lhs),
                        render_expr(rhs)
                    ))
                    .with_expected("至少一侧为 true"),
            ),
            CftSchemaCheckExprKind::Is {
                expr: inner,
                predicate,
            } => match predicate {
                CftSchemaTypePredicate::Null => {
                    let actual = self.eval_expr(inner).ok().map_or_else(
                        || format!("{} 不是 null", render_expr(inner)),
                        |actual| {
                            format!(
                                "{} = {}",
                                render_expr(inner),
                                format_value_for_message(&actual.value)
                            )
                        },
                    );
                    Some(
                        CheckExplanation::new(
                            CfdErrorCode::CheckNullPredicateFailed,
                            rendered,
                            value.path.clone(),
                        )
                        .with_actual(actual)
                        .with_expected("null"),
                    )
                }
                CftSchemaTypePredicate::Type(type_name) => {
                    let actual = self
                        .eval_expr(inner)
                        .ok()
                        .and_then(|actual| actual.value.actual_type(self.model).map(str::to_string))
                        .unwrap_or_else(|| "非对象".to_string());
                    Some(
                        CheckExplanation::new(
                            CfdErrorCode::CheckTypePredicateFailed,
                            rendered,
                            value.path.clone(),
                        )
                        .with_actual(format!("实际类型 = {actual}"))
                        .with_expected(format!("类型为 {type_name}")),
                    )
                }
            },
            CftSchemaCheckExprKind::CmpChain { first, rest } => {
                self.explain_failed_comparison(&rendered, first, rest, value.path.clone())
            }
            _ => None,
        }
    }

    fn explain_false_value_expr(
        &mut self,
        expr: &CftSchemaCheckExpr,
        value: &LocatedCheckValue,
        rendered: String,
    ) -> CheckExplanation {
        match &expr.kind {
            CftSchemaCheckExprKind::Call { name, args }
                if name == "contains" && args.len() == 2 =>
            {
                CheckExplanation::new(
                    CfdErrorCode::CheckContainsFailed,
                    rendered,
                    value.path.clone(),
                )
                .with_actual(self.value_expr_actual(&args[0]))
                .with_expected(format!("包含 {}", render_expr(&args[1])))
            }
            CftSchemaCheckExprKind::MethodCall {
                receiver,
                name,
                args,
            } if name == "contains" && args.len() == 1 => CheckExplanation::new(
                CfdErrorCode::CheckContainsFailed,
                rendered,
                value.path.clone(),
            )
            .with_actual(self.value_expr_actual(receiver))
            .with_expected(format!("包含 {}", render_expr(&args[0]))),
            CftSchemaCheckExprKind::Call { name, args }
                if name == "isUnique" && args.len() == 1 =>
            {
                self.unique_failed_explanation(&rendered, &args[0], value.path.clone())
            }
            CftSchemaCheckExprKind::MethodCall {
                receiver,
                name,
                args,
            } if name == "isUnique" && args.is_empty() => {
                self.unique_failed_explanation(&rendered, receiver, value.path.clone())
            }
            CftSchemaCheckExprKind::Call { name, args } if name == "matches" && args.len() == 2 => {
                CheckExplanation::new(
                    CfdErrorCode::CheckMatchesFailed,
                    rendered,
                    value.path.clone(),
                )
                .with_actual(self.value_expr_actual(&args[0]))
                .with_expected(format!("匹配 {}", render_expr(&args[1])))
            }
            CftSchemaCheckExprKind::MethodCall {
                receiver,
                name,
                args,
            } if name == "matches" && args.len() == 1 => CheckExplanation::new(
                CfdErrorCode::CheckMatchesFailed,
                rendered,
                value.path.clone(),
            )
            .with_actual(self.value_expr_actual(receiver))
            .with_expected(format!("匹配 {}", render_expr(&args[0]))),
            _ => CheckExplanation::new(
                CfdErrorCode::CheckBoolExpectedTrue,
                rendered,
                value.path.clone(),
            )
            .with_actual("false")
            .with_expected("true"),
        }
    }

    fn explain_failed_comparison(
        &mut self,
        rendered: &str,
        first: &CftSchemaCheckExpr,
        rest: &[(CftSchemaCmpOp, CftSchemaCheckExpr)],
        fallback_path: Option<CfdPath>,
    ) -> Option<CheckExplanation> {
        let mut lhs_expr = first;
        let mut lhs = self.eval_expr(first).ok()?;
        for (op, rhs_expr) in rest {
            let rhs = self.eval_expr(rhs_expr).ok()?;
            let path = lhs
                .path
                .clone()
                .or_else(|| rhs.path.clone())
                .or_else(|| fallback_path.clone());
            if !self
                .from_ops(ops::compare(*op, &lhs.value, &rhs.value, rhs.path.clone()))
                .ok()?
            {
                let null_predicate =
                    matches!(lhs.value, CheckValue::Null) || matches!(rhs.value, CheckValue::Null);
                let code =
                    if null_predicate && matches!(op, CftSchemaCmpOp::Eq | CftSchemaCmpOp::Ne) {
                        CfdErrorCode::CheckNullPredicateFailed
                    } else {
                        CfdErrorCode::CheckComparisonFailed
                    };
                let actual_expr = if lhs.path.is_some() {
                    lhs_expr
                } else {
                    rhs_expr
                };
                let actual_value = if lhs.path.is_some() {
                    &lhs.value
                } else {
                    &rhs.value
                };
                return Some(
                    CheckExplanation::new(code, rendered.to_string(), path)
                        .with_actual(format!(
                            "{} = {}",
                            render_expr(actual_expr),
                            format_value_for_message(actual_value)
                        ))
                        .with_expected(format!("{} {}", cmp_op_str(*op), render_expr(rhs_expr))),
                );
            }
            lhs_expr = rhs_expr;
            lhs = rhs;
        }
        None
    }

    fn value_expr_actual(&mut self, expr: &CftSchemaCheckExpr) -> String {
        self.eval_expr(expr).map_or_else(
            |_| render_expr(expr),
            |value| {
                format!(
                    "{} = {}",
                    render_expr(expr),
                    format_value_for_message(&value.value)
                )
            },
        )
    }

    fn unique_failed_explanation(
        &mut self,
        rendered: &str,
        collection: &CftSchemaCheckExpr,
        path: Option<CfdPath>,
    ) -> CheckExplanation {
        let mut explanation =
            CheckExplanation::new(CfdErrorCode::CheckUniqueFailed, rendered.to_string(), path)
                .with_actual(self.value_expr_actual(collection))
                .with_expected("所有元素唯一");

        if let Ok(value) = self.eval_expr(collection) {
            if let CheckValue::Array { items, .. } = value.value {
                let mut seen = BTreeMap::new();
                for (index, item) in items.iter().enumerate() {
                    if let Some(key) = comparable_key(item) {
                        if let Some(first_index) = seen.insert(key, index) {
                            explanation = explanation.with_actual(format!(
                                "重复值 {} 出现在索引 {first_index} 和 {index}",
                                format_value_for_message(item)
                            ));
                            break;
                        }
                    }
                }
            }
        }
        explanation
    }

    #[allow(clippy::too_many_lines)]
    fn eval_expr(&mut self, expr: &CftSchemaCheckExpr) -> EvalResult<LocatedCheckValue> {
        match &expr.kind {
            CftSchemaCheckExprKind::Int(value) => {
                Ok(LocatedCheckValue::value(CheckValue::Int(*value)))
            }
            CftSchemaCheckExprKind::Float(value) => {
                Ok(LocatedCheckValue::value(CheckValue::Float(*value)))
            }
            CftSchemaCheckExprKind::Bool(value) => {
                Ok(LocatedCheckValue::value(CheckValue::Bool(*value)))
            }
            CftSchemaCheckExprKind::Null => Ok(LocatedCheckValue::value(CheckValue::Null)),
            CftSchemaCheckExprKind::String(value) => {
                Ok(LocatedCheckValue::value(CheckValue::String(value.clone())))
            }
            CftSchemaCheckExprKind::Name(name) => self.eval_name(name),
            CftSchemaCheckExprKind::Field { expr: inner, name } => {
                if let CftSchemaCheckExprKind::Name(enum_name) = &inner.kind {
                    if let Some(enum_value) = self.schema.enum_variant_value(enum_name, name) {
                        return Ok(LocatedCheckValue::value(CheckValue::Enum(CfdEnumValue {
                            enum_name: enum_name.clone(),
                            variant: Some(name.clone()),
                            value: enum_value,
                        })));
                    }
                }
                let target = self.eval_expr(inner)?;
                self.eval_field(target, name)
            }
            CftSchemaCheckExprKind::Index { expr: inner, index } => {
                let target = self.eval_expr(inner)?;
                let index = self.eval_expr(index)?;
                let result = self.eval_index(target, index)?;
                if let CheckValue::Record(CheckRecordRef::Top(id)) = &result.value {
                    self.note_read_from(*id);
                }
                Ok(result)
            }
            CftSchemaCheckExprKind::Is {
                expr: inner,
                predicate,
            } => {
                let value = self.eval_expr(inner)?;
                Ok(LocatedCheckValue::new(
                    CheckValue::Bool(self.eval_is(&value.value, predicate)),
                    value.path,
                ))
            }
            CftSchemaCheckExprKind::Call { name, args } => self.eval_call(name, args),
            CftSchemaCheckExprKind::MethodCall {
                receiver,
                name,
                args,
            } => self.eval_method_call(receiver, name, args),
            CftSchemaCheckExprKind::BinOp { op, lhs, rhs } => self.eval_bin_op(*op, lhs, rhs),
            CftSchemaCheckExprKind::Unary { op, expr: inner } => {
                let value = self.eval_expr(inner)?;
                self.eval_unary(*op, value)
            }
            CftSchemaCheckExprKind::CmpChain { first, rest } => {
                let mut lhs = self.eval_expr(first)?;
                for (op, rhs_expr) in rest {
                    let rhs = self.eval_expr(rhs_expr)?;
                    let path = lhs.path.clone().or_else(|| rhs.path.clone());
                    if !self.from_ops(ops::compare(
                        *op,
                        &lhs.value,
                        &rhs.value,
                        rhs.path.clone(),
                    ))? {
                        return Ok(LocatedCheckValue::new(CheckValue::Bool(false), path));
                    }
                    lhs = rhs;
                }
                Ok(LocatedCheckValue::value(CheckValue::Bool(true)))
            }
        }
    }

    fn eval_name(&mut self, name: &str) -> EvalResult<LocatedCheckValue> {
        for scope in self.scopes.iter().rev() {
            if let Some(value) = scope.get(name) {
                return Ok(value.clone());
            }
        }
        if let Some(mut value) = fields::current_field(
            self.schema,
            self.model,
            self.root_record,
            &self.root_path,
            &self.current,
            name,
        ) {
            if let CheckValue::Record(CheckRecordRef::Top(id)) = &value.value {
                self.note_read_from(*id);
            }
            if let CheckValue::Record(record) = self.current.clone() {
                self.apply_dimension_variant(&record, name, &mut value)?;
            }
            return Ok(value);
        }
        if let Some(value) = self.schema.consts.get(name) {
            return Ok(LocatedCheckValue::value(CheckValue::from_const(value)));
        }
        if self.schema.enums.contains_key(name) {
            return Ok(LocatedCheckValue::value(CheckValue::EnumNamespace(
                name.to_string(),
            )));
        }
        self.diag(
            CfdErrorCode::CheckEvalTypeError,
            format!("未知 check 值 `{name}`"),
        );
        Err(EvalAbort::Error)
    }

    fn eval_field(
        &mut self,
        target: LocatedCheckValue,
        name: &str,
    ) -> EvalResult<LocatedCheckValue> {
        let target_record = match &target.value {
            CheckValue::Record(record) => Some(record.clone()),
            _ => None,
        };
        let mut result = self.from_ops(fields::field_value(
            self.schema,
            self.model,
            self.root_record,
            &self.root_path,
            target,
            name,
        ))?;
        if let CheckValue::Record(CheckRecordRef::Top(id)) = &result.value {
            self.note_read_from(*id);
        }
        if let Some(record) = target_record {
            self.apply_dimension_variant(&record, name, &mut result)?;
        }
        Ok(result)
    }

    fn eval_index(
        &mut self,
        target: LocatedCheckValue,
        index: LocatedCheckValue,
    ) -> EvalResult<LocatedCheckValue> {
        self.from_ops(access::index_value(target, index))
    }

    fn eval_is(&self, value: &CheckValue, predicate: &CftSchemaTypePredicate) -> bool {
        match predicate {
            CftSchemaTypePredicate::Null => matches!(value, CheckValue::Null),
            CftSchemaTypePredicate::Type(type_name) => value
                .actual_type(self.model)
                .is_some_and(|actual| self.schema.is_assignable(actual, type_name)),
        }
    }

    fn eval_call(
        &mut self,
        name: &str,
        args: &[CftSchemaCheckExpr],
    ) -> EvalResult<LocatedCheckValue> {
        let signature = self.resolve_call_signature(CallSignature::resolve_function(
            name,
            args.len(),
            self.schema.enums.contains_key(name),
        ))?;

        match signature.target {
            CallTarget::EnumConstructor => {
                let arg = &args[0];
                let arg_value = self.eval_expr(arg)?;
                let arg_kind = arg_value.value.clone();
                let CheckValue::Int(value) = arg_value.value else {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        arg_value.path,
                        format!(
                            "枚举构造函数参数不是 int: 实际为 {}",
                            format_value_for_message(&arg_kind)
                        ),
                    );
                    return Err(EvalAbort::Error);
                };
                Ok(LocatedCheckValue::value(CheckValue::Enum(
                    enum_values::enum_with_value(self.schema, name, value),
                )))
            }
            CallTarget::Builtin(builtin) => self.eval_builtin_call(builtin, args),
        }
    }

    fn eval_builtin_call(
        &mut self,
        builtin: Builtin,
        args: &[CftSchemaCheckExpr],
    ) -> EvalResult<LocatedCheckValue> {
        match builtin {
            Builtin::Len => {
                let arg = &args[0];
                let arg_value = self.eval_expr(arg)?;
                self.from_ops(builtin_values::len_value(arg_value))
            }
            Builtin::Contains => {
                let collection = self.eval_expr(&args[0])?;
                let value = self.eval_expr(&args[1])?;
                Ok(LocatedCheckValue::new(
                    CheckValue::Bool(
                        self.from_ops(builtin_values::contains_value(&collection, &value.value))?,
                    ),
                    collection.path.clone(),
                ))
            }
            Builtin::Unique => {
                let arg = &args[0];
                let arg_value = self.eval_expr(arg)?;
                self.from_ops(builtin_values::unique_value(arg_value))
            }
            Builtin::Min | Builtin::Max => self.eval_min_max(builtin, args),
            Builtin::Sum => self.eval_sum(args),
            Builtin::Keys => {
                let arg = &args[0];
                let arg_value = self.eval_expr(arg)?;
                self.from_ops(builtin_values::keys_value(arg_value))
            }
            Builtin::Values => {
                let arg = &args[0];
                let arg_value = self.eval_expr(arg)?;
                self.from_ops(builtin_values::values_value(arg_value))
            }
            Builtin::Matches => {
                let value = self.eval_expr(&args[0])?;
                let pattern =
                    self.resolve_call_signature(builtin_calls::matches_pattern_arg(&args[1]))?;
                self.from_ops(builtin_values::matches_value(value, pattern))
            }
        }
    }

    fn eval_method_call(
        &mut self,
        receiver: &CftSchemaCheckExpr,
        name: &str,
        args: &[CftSchemaCheckExpr],
    ) -> EvalResult<LocatedCheckValue> {
        let signature =
            self.resolve_call_signature(CallSignature::resolve_method(name, args.len()))?;
        let CallTarget::Builtin(builtin) = signature.target else {
            unreachable!("method calls cannot resolve to enum constructors");
        };

        let receiver_value = self.eval_expr(receiver)?;
        match builtin {
            Builtin::Len => self.from_ops(builtin_values::len_value(receiver_value)),
            Builtin::Contains => {
                let value = self.eval_expr(&args[0])?;
                Ok(LocatedCheckValue::new(
                    CheckValue::Bool(self.from_ops(builtin_values::contains_value(
                        &receiver_value,
                        &value.value,
                    ))?),
                    receiver_value.path.clone(),
                ))
            }
            Builtin::Unique => self.from_ops(builtin_values::unique_value(receiver_value)),
            Builtin::Min | Builtin::Max => self.eval_min_max_value(builtin, receiver_value),
            Builtin::Sum => self.eval_sum_value(receiver_value),
            Builtin::Keys => self.from_ops(builtin_values::keys_value(receiver_value)),
            Builtin::Values => self.from_ops(builtin_values::values_value(receiver_value)),
            Builtin::Matches => {
                let pattern =
                    self.resolve_call_signature(builtin_calls::matches_pattern_arg(&args[0]))?;
                self.from_ops(builtin_values::matches_value(receiver_value, pattern))
            }
        }
    }

    fn resolve_call_signature<T>(
        &mut self,
        result: Result<T, CallSignatureError>,
    ) -> EvalResult<T> {
        match result {
            Ok(value) => Ok(value),
            Err(CallSignatureError::UnknownFunction { name }) => {
                self.diag(
                    CfdErrorCode::CheckEvalTypeError,
                    format!("未知函数 `{name}`"),
                );
                Err(EvalAbort::Error)
            }
            Err(CallSignatureError::Arity { message }) => {
                self.diag(CfdErrorCode::CheckEvalTypeError, message);
                Err(EvalAbort::Error)
            }
        }
    }

    fn eval_min_max(
        &mut self,
        builtin: Builtin,
        args: &[CftSchemaCheckExpr],
    ) -> EvalResult<LocatedCheckValue> {
        let arg_value = self.eval_expr(&args[0])?;
        self.eval_min_max_value(builtin, arg_value)
    }

    fn eval_min_max_value(
        &mut self,
        builtin: Builtin,
        arg_value: LocatedCheckValue,
    ) -> EvalResult<LocatedCheckValue> {
        self.from_ops(builtin_values::min_max_value(builtin, arg_value))
    }

    fn eval_sum(&mut self, args: &[CftSchemaCheckExpr]) -> EvalResult<LocatedCheckValue> {
        let arg_value = self.eval_expr(&args[0])?;
        self.eval_sum_value(arg_value)
    }

    fn eval_sum_value(&mut self, arg_value: LocatedCheckValue) -> EvalResult<LocatedCheckValue> {
        self.from_ops(builtin_values::sum_value(arg_value))
    }

    fn eval_unary(
        &mut self,
        op: CftSchemaUnaryOp,
        value: LocatedCheckValue,
    ) -> EvalResult<LocatedCheckValue> {
        self.from_ops(ops::unary_op(self.schema, op, value))
    }

    #[allow(clippy::similar_names)]
    fn eval_bin_op(
        &mut self,
        op: CftSchemaBinOp,
        lhs: &CftSchemaCheckExpr,
        rhs: &CftSchemaCheckExpr,
    ) -> EvalResult<LocatedCheckValue> {
        match op {
            CftSchemaBinOp::Or => {
                let lhs = self.eval_expr(lhs)?;
                let lhs_path = lhs.path.clone();
                let bad_lhs_value = lhs.value.clone();
                let CheckValue::Bool(lhs) = lhs.value else {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        lhs_path,
                        format!(
                            "左操作数不是 bool: 实际为 {}",
                            format_value_for_message(&bad_lhs_value)
                        ),
                    );
                    return Err(EvalAbort::Error);
                };
                if lhs {
                    return Ok(LocatedCheckValue::new(CheckValue::Bool(true), lhs_path));
                }
                let rhs = self.eval_expr(rhs)?;
                let rhs_path = rhs.path.clone();
                let bad_rhs_value = rhs.value.clone();
                let CheckValue::Bool(rhs) = rhs.value else {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        rhs_path,
                        format!(
                            "右操作数不是 bool: 实际为 {}",
                            format_value_for_message(&bad_rhs_value)
                        ),
                    );
                    return Err(EvalAbort::Error);
                };
                Ok(LocatedCheckValue::new(CheckValue::Bool(rhs), rhs_path))
            }
            CftSchemaBinOp::And => {
                let lhs = self.eval_expr(lhs)?;
                let lhs_path = lhs.path.clone();
                let bad_lhs_value = lhs.value.clone();
                let CheckValue::Bool(lhs) = lhs.value else {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        lhs_path,
                        format!(
                            "左操作数不是 bool: 实际为 {}",
                            format_value_for_message(&bad_lhs_value)
                        ),
                    );
                    return Err(EvalAbort::Error);
                };
                if !lhs {
                    return Ok(LocatedCheckValue::new(CheckValue::Bool(false), lhs_path));
                }
                let rhs = self.eval_expr(rhs)?;
                let rhs_path = rhs.path.clone();
                let bad_rhs_value = rhs.value.clone();
                let CheckValue::Bool(rhs) = rhs.value else {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        rhs_path,
                        format!(
                            "右操作数不是 bool: 实际为 {}",
                            format_value_for_message(&bad_rhs_value)
                        ),
                    );
                    return Err(EvalAbort::Error);
                };
                Ok(LocatedCheckValue::new(CheckValue::Bool(rhs), rhs_path))
            }
            _ => {
                let lhs = self.eval_expr(lhs)?;
                let rhs = self.eval_expr(rhs)?;
                let path = lhs.path.clone().or_else(|| rhs.path.clone());
                self.from_ops(ops::eager_bin_op(
                    self.schema,
                    op,
                    lhs.value,
                    rhs.value,
                    path,
                ))
            }
        }
    }

    fn diag(&mut self, code: CfdErrorCode, message: impl Into<String>) {
        self.diag_at(code, None, message);
    }

    fn diag_at(&mut self, code: CfdErrorCode, path: Option<CfdPath>, message: impl Into<String>) {
        let path = match path {
            Some(path) => path,
            None => self.root_path.clone(),
        };
        let mut message = message.into();
        if !self.contexts.is_empty() && !message.contains("\n上下文: ") {
            for context in &self.contexts {
                message.push_str("\n上下文: ");
                message.push_str(context);
            }
        }
        self.diagnostics
            .push(CfdDiagnostic::error(code, message).with_primary(self.root_record, path));
    }
}
