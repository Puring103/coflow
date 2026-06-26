use super::builtins::Builtin;
use super::value::{
    comparable_key, dict_key_from_check_value, format_check_key_for_path, values_equal, CheckValue,
    LocatedCheckValue,
};
use crate::schema_view::{DimensionFieldMeta, SchemaView};
use crate::DimensionCheckContext;
use coflow_cft::{
    CftSchemaBinOp, CftSchemaCheckBlock, CftSchemaCheckExpr, CftSchemaCheckExprKind,
    CftSchemaCheckStmt, CftSchemaCmpOp, CftSchemaQuantifierKind, CftSchemaTypePredicate,
    CftSchemaTypeRef, CftSchemaUnaryOp,
};
use coflow_data_model::{
    CfdDataModel, CfdDiagnostic, CfdEnumValue, CfdErrorCode, CfdPath, CfdPathSegment, CfdRecordId,
};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};

use super::value::CheckRecordRef;

pub(super) struct CheckEvaluator<'a> {
    schema: &'a SchemaView,
    model: &'a CfdDataModel,
    root_record: Option<CfdRecordId>,
    root_path: CfdPath,
    current: CheckValue,
    scopes: Vec<BTreeMap<String, LocatedCheckValue>>,
    contexts: Vec<String>,
    pub(super) diagnostics: Vec<CfdDiagnostic>,
    /// When `true`, every traversal that resolves to a different top-level
    /// record id records a `reads_from` edge from the current root. The
    /// runner toggles this on for full check runs that produce a dep graph.
    pub(super) dep_collector_enabled: bool,
    pub(super) reads_from: BTreeSet<CfdRecordId>,
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

#[derive(Debug)]
struct CheckExplanation {
    code: CfdErrorCode,
    expression: String,
    actual: Option<String>,
    expected: Option<String>,
    context: Vec<String>,
    path: Option<CfdPath>,
}

impl CheckExplanation {
    fn new(code: CfdErrorCode, expression: impl Into<String>, path: Option<CfdPath>) -> Self {
        Self {
            code,
            expression: expression.into(),
            actual: None,
            expected: None,
            context: Vec::new(),
            path,
        }
    }

    fn with_actual(mut self, actual: impl Into<String>) -> Self {
        self.actual = Some(actual.into());
        self
    }

    fn with_expected(mut self, expected: impl Into<String>) -> Self {
        self.expected = Some(expected.into());
        self
    }

    fn with_context(mut self, context: &[String]) -> Self {
        self.context.extend(context.iter().cloned());
        self
    }

    fn message(&self) -> String {
        let mut out = format!("校验失败: {}", self.expression);
        if let Some(actual) = &self.actual {
            out.push_str("\n实际值: ");
            out.push_str(actual);
        }
        if let Some(expected) = &self.expected {
            out.push_str("\n期望: ");
            out.push_str(expected);
        }
        for context in &self.context {
            out.push_str("\n上下文: ");
            out.push_str(context);
        }
        out
    }
}

impl<'a> CheckEvaluator<'a> {
    pub(super) fn new(
        schema: &'a SchemaView,
        model: &'a CfdDataModel,
        root_record: Option<CfdRecordId>,
        root_path: CfdPath,
        current: CheckValue,
    ) -> Self {
        let mut reads_from = BTreeSet::new();
        let initial_top = match &current {
            CheckValue::Record(CheckRecordRef::Top(id)) => Some(*id),
            _ => None,
        };
        if let (Some(my), Some(other)) = (root_record, initial_top) {
            if my != other {
                reads_from.insert(other);
            }
        }
        Self {
            schema,
            model,
            root_record,
            root_path,
            current,
            scopes: Vec::new(),
            contexts: Vec::new(),
            diagnostics: Vec::new(),
            dep_collector_enabled: false,
            reads_from,
            dimension_context: None,
        }
    }

    /// Record a "this evaluator (rooted at `root_record`) read from another
    /// top-level record" edge. Self-references are ignored. The dep graph is
    /// built only when `dep_collector_enabled` is `true`.
    pub(super) fn note_read_from(&mut self, target: CfdRecordId) {
        if !self.dep_collector_enabled {
            return;
        }
        if let Some(my) = self.root_record {
            if my == target {
                return;
            }
        }
        self.reads_from.insert(target);
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
        let field = field.clone();
        let Some(record_key) = dimension_record_key(self.model, record, &field) else {
            self.diag_at(
                CfdErrorCode::CheckEvalTypeError,
                located.path.clone(),
                format!("维度字段 `{actual_type}.{field_name}` 无法定位合成记录 key"),
            );
            return Err(EvalAbort::Error);
        };
        let Some(variant_record_id) = self.model.lookup(&field.synthesized_type, &record_key)
        else {
            self.diag_at(
                CfdErrorCode::CheckEvalTypeError,
                located.path.clone(),
                format!(
                    "维度字段 `{actual_type}.{field_name}` 缺少合成记录 `{}:{record_key}`",
                    field.synthesized_type
                ),
            );
            return Err(EvalAbort::Error);
        };
        self.note_read_from(variant_record_id);
        let Some(variant_record) = self.model.record(variant_record_id) else {
            self.diag_at(
                CfdErrorCode::CheckEvalTypeError,
                located.path.clone(),
                format!(
                    "维度字段 `{actual_type}.{field_name}` 的合成记录 `{}:{record_key}` 不存在",
                    field.synthesized_type
                ),
            );
            return Err(EvalAbort::Error);
        };
        let Some(value) = variant_record.fields.get(&variant) else {
            self.diag_at(
                CfdErrorCode::CheckEvalTypeError,
                located.path.clone(),
                format!(
                    "维度字段 `{actual_type}.{field_name}` 的合成记录 `{}:{record_key}` 缺少 variant `{variant}`",
                    field.synthesized_type
                ),
            );
            return Err(EvalAbort::Error);
        };
        let path = located.path.clone();
        located.value = CheckValue::from_cfd_value_with_path(
            value,
            self.schema.field_type(&field.synthesized_type, &variant),
            path.clone(),
            self.model,
            Some(variant_record_id),
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
                let Some(items) = self.quantifier_items(collection_value) else {
                    return EvalFlow::HardStop;
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

    fn quantifier_items(
        &mut self,
        collection: LocatedCheckValue,
    ) -> Option<Vec<LocatedCheckValue>> {
        match collection.value {
            CheckValue::Array { items, .. } => Some(
                items
                    .into_iter()
                    .enumerate()
                    .map(|(index, item)| {
                        LocatedCheckValue::new(
                            item,
                            collection.path.clone().map(|path| path.index(index)),
                        )
                    })
                    .collect(),
            ),
            CheckValue::Dict { entries, .. } => Some(
                entries
                    .into_iter()
                    .enumerate()
                    .map(|(index, entry)| {
                        let key_label = match format_check_key_for_path(&entry.key) {
                            Some(label) => label,
                            None => index.to_string(),
                        };
                        let path = collection.path.clone().map(|path| path.dict_key(key_label));
                        LocatedCheckValue::new(CheckValue::Entry(Box::new(entry)), path)
                    })
                    .collect(),
            ),
            other => {
                self.diag(
                    CfdErrorCode::CheckEvalTypeError,
                    format!(
                        "量词目标不是集合: 实际为 {}",
                        format_value_for_message(&other)
                    ),
                );
                None
            }
        }
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
                    if !self.compare(*op, &lhs.value, &rhs.value, rhs.path.clone())? {
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
            CftSchemaCheckExprKind::Call { name, args } if name == "unique" && args.len() == 1 => {
                self.unique_failed_explanation(&rendered, &args[0], value.path.clone())
            }
            CftSchemaCheckExprKind::MethodCall {
                receiver,
                name,
                args,
            } if name == "unique" && args.is_empty() => {
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
                .compare(*op, &lhs.value, &rhs.value, rhs.path.clone())
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
                    if !self.compare(*op, &lhs.value, &rhs.value, rhs.path.clone())? {
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
        if let Some(value) = self.current_field(name)? {
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

    fn current_field(&mut self, name: &str) -> EvalResult<Option<LocatedCheckValue>> {
        let record = match &self.current {
            CheckValue::Record(record) => record.clone(),
            _ => return Ok(None),
        };
        if name == "id" {
            return Ok(self.virtual_id(&record, record.path()));
        }
        let field_type = self.field_type_for_record(&record, name);
        let mut result = record.field(self.model, field_type, name);
        if let Some(located) = &result {
            if let CheckValue::Record(CheckRecordRef::Top(id)) = &located.value {
                self.note_read_from(*id);
            }
        }
        if let Some(located) = result.as_mut() {
            self.apply_dimension_variant(&record, name, located)?;
        }
        Ok(result)
    }

    fn field_type_for_record(
        &self,
        record: &CheckRecordRef,
        name: &str,
    ) -> Option<&CftSchemaTypeRef> {
        record
            .actual_type(self.model)
            .and_then(|actual_type| self.schema.field_type(actual_type, name))
    }

    fn eval_field(
        &mut self,
        target: LocatedCheckValue,
        name: &str,
    ) -> EvalResult<LocatedCheckValue> {
        if matches!(target.value, CheckValue::Null) {
            self.diag_at(
                CfdErrorCode::CheckNullAccess,
                target.path,
                format!("不能访问 null 的字段: 尝试在 null 上读取 `.{name}`"),
            );
            return Err(EvalAbort::Error);
        }
        match target.value {
            CheckValue::Record(record) => {
                if name == "id" {
                    return self.virtual_id(&record, target.path).ok_or_else(|| {
                        self.diag_at(CfdErrorCode::CheckEvalTypeError, None, "记录没有虚拟 id");
                        EvalAbort::Error
                    });
                }
                let field_type = self.field_type_for_record(&record, name);
                let mut result = record.field(self.model, field_type, name).ok_or_else(|| {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        target.path,
                        format!("记录没有字段 `{name}`"),
                    );
                    EvalAbort::Error
                });
                if let Ok(located) = &result {
                    if let CheckValue::Record(CheckRecordRef::Top(id)) = &located.value {
                        self.note_read_from(*id);
                    }
                }
                if let Ok(located) = result.as_mut() {
                    self.apply_dimension_variant(&record, name, located)?;
                }
                result
            }
            CheckValue::Entry(entry) => match name {
                "key" => Ok(LocatedCheckValue::new(*entry.key, target.path)),
                "value" => Ok(LocatedCheckValue::new(entry.value, target.path)),
                _ => {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        target.path,
                        format!("dict entry 没有字段 `{name}`，只有 `key` 和 `value`"),
                    );
                    Err(EvalAbort::Error)
                }
            },
            other => {
                self.diag_at(
                    CfdErrorCode::CheckEvalTypeError,
                    target.path,
                    format!(
                        "字段访问目标不是对象: 读取 `.{name}` 时实际为 {}",
                        format_value_for_message(&other)
                    ),
                );
                Err(EvalAbort::Error)
            }
        }
    }

    fn virtual_id(
        &self,
        record: &CheckRecordRef,
        path: Option<CfdPath>,
    ) -> Option<LocatedCheckValue> {
        let key = record
            .key(self.model)
            .filter(|key| !key.is_empty())
            .or_else(|| {
                self.root_record
                    .and_then(|id| self.model.record(id).map(coflow_data_model::CfdRecord::key))
            })?;
        let key = key.to_string();
        let path = path
            .map(|path| path.field("id"))
            .or_else(|| Some(self.root_path.clone().field("id")));
        Some(LocatedCheckValue::new(CheckValue::String(key), path))
    }

    fn eval_index(
        &mut self,
        target: LocatedCheckValue,
        index: LocatedCheckValue,
    ) -> EvalResult<LocatedCheckValue> {
        if matches!(target.value, CheckValue::Null) {
            self.diag_at(
                CfdErrorCode::CheckNullAccess,
                target.path,
                format!(
                    "不能索引 null: 尝试在 null 上读取 [{}]",
                    format_value_for_message(&index.value)
                ),
            );
            return Err(EvalAbort::Error);
        }
        match target.value {
            CheckValue::Array { items, .. } => {
                let CheckValue::Int(idx) = index.value else {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        index.path,
                        format!(
                            "数组索引不是 int: 实际为 {}",
                            format_value_for_message(&index.value)
                        ),
                    );
                    return Err(EvalAbort::Error);
                };
                let len = items.len();
                let Ok(idx_us) = usize::try_from(idx) else {
                    self.diag_at(
                        CfdErrorCode::CheckIndexOutOfBounds,
                        target.path,
                        format!("数组索引为负数: 实际为 {idx}，长度为 {len}"),
                    );
                    return Err(EvalAbort::Error);
                };
                items
                    .get(idx_us)
                    .cloned()
                    .map(|value| {
                        LocatedCheckValue::new(
                            value,
                            target.path.clone().map(|path| path.index(idx_us)),
                        )
                    })
                    .ok_or_else(|| {
                        self.diag_at(
                            CfdErrorCode::CheckIndexOutOfBounds,
                            target.path,
                            format!("数组索引越界: 索引 {idx_us}，长度 {len}"),
                        );
                        EvalAbort::Error
                    })
            }
            CheckValue::Dict { entries, .. } => {
                let Some(key) = dict_key_from_check_value(&index.value) else {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        index.path,
                        format!(
                            "dict 索引不是有效 key: 实际为 {}",
                            format_value_for_message(&index.value)
                        ),
                    );
                    return Err(EvalAbort::Error);
                };
                let key_label = format_value_for_message(&index.value);
                entries
                    .into_iter()
                    .find(|entry| entry.key_key().is_some_and(|entry_key| entry_key == key))
                    .map(|entry| LocatedCheckValue::new(entry.value, target.path.clone()))
                    .ok_or_else(|| {
                        self.diag_at(
                            CfdErrorCode::CheckMissingDictKey,
                            target.path,
                            format!("dict key {key_label} 不存在"),
                        );
                        EvalAbort::Error
                    })
            }
            other => {
                self.diag_at(
                    CfdErrorCode::CheckEvalTypeError,
                    target.path,
                    format!(
                        "索引目标不是集合: 读取 [{}] 时实际为 {}",
                        format_value_for_message(&index.value),
                        format_value_for_message(&other),
                    ),
                );
                Err(EvalAbort::Error)
            }
        }
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
        if self.schema.enums.contains_key(name) {
            let arg = self.exactly_one_arg(args, "枚举构造函数需要 1 个参数")?;
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
            let enum_value = match self.schema.enum_value_from_int(name, value) {
                Some(enum_value) => enum_value,
                None => Self::anonymous_enum_value(name, value),
            };
            return Ok(LocatedCheckValue::value(CheckValue::Enum(enum_value)));
        }

        let Some(builtin) = Builtin::by_name(name) else {
            self.diag(
                CfdErrorCode::CheckEvalTypeError,
                format!("未知函数 `{name}`"),
            );
            return Err(EvalAbort::Error);
        };
        self.require_builtin_arity(builtin, args)?;

        match builtin {
            Builtin::Len => {
                let arg = &args[0];
                let arg_value = self.eval_expr(arg)?;
                match arg_value.value {
                    CheckValue::Array { items, .. } => Ok(LocatedCheckValue::new(
                        CheckValue::Int(items.len() as i64),
                        arg_value.path,
                    )),
                    CheckValue::Dict { entries, .. } => Ok(LocatedCheckValue::new(
                        CheckValue::Int(entries.len() as i64),
                        arg_value.path,
                    )),
                    other => {
                        self.diag_at(
                            CfdErrorCode::CheckEvalTypeError,
                            arg_value.path,
                            format!(
                                "len 需要 array 或 dict: 实际为 {}",
                                format_value_for_message(&other)
                            ),
                        );
                        Err(EvalAbort::Error)
                    }
                }
            }
            Builtin::Contains => {
                let collection = self.eval_expr(&args[0])?;
                let value = self.eval_expr(&args[1])?;
                Ok(LocatedCheckValue::new(
                    CheckValue::Bool(self.contains_value(&collection, &value.value)?),
                    collection.path.clone(),
                ))
            }
            Builtin::Unique => {
                let arg = &args[0];
                let arg_value = self.eval_expr(arg)?;
                let arg_kind = arg_value.value.clone();
                let CheckValue::Array { items, .. } = arg_value.value else {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        arg_value.path,
                        format!(
                            "unique 需要 array: 实际为 {}",
                            format_value_for_message(&arg_kind)
                        ),
                    );
                    return Err(EvalAbort::Error);
                };
                let mut seen = BTreeSet::new();
                for item in items {
                    let Some(key) = comparable_key(&item) else {
                        self.diag_at(
                            CfdErrorCode::CheckEvalTypeError,
                            arg_value.path.clone(),
                            format!(
                                "unique 元素不可比较: 实际为 {}",
                                format_value_for_message(&item)
                            ),
                        );
                        return Err(EvalAbort::Error);
                    };
                    if !seen.insert(key) {
                        return Ok(LocatedCheckValue::new(
                            CheckValue::Bool(false),
                            arg_value.path,
                        ));
                    }
                }
                Ok(LocatedCheckValue::new(
                    CheckValue::Bool(true),
                    arg_value.path,
                ))
            }
            Builtin::Min | Builtin::Max => self.eval_min_max(builtin, args),
            Builtin::Sum => self.eval_sum(args),
            Builtin::Keys => {
                let arg = &args[0];
                let arg_value = self.eval_expr(arg)?;
                let arg_kind = arg_value.value.clone();
                let CheckValue::Dict {
                    entries, key_type, ..
                } = arg_value.value
                else {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        arg_value.path,
                        format!(
                            "keys 需要 dict: 实际为 {}",
                            format_value_for_message(&arg_kind)
                        ),
                    );
                    return Err(EvalAbort::Error);
                };
                Ok(LocatedCheckValue::new(
                    CheckValue::Array {
                        items: entries.into_iter().map(|entry| *entry.key).collect(),
                        element_type: key_type,
                    },
                    arg_value.path,
                ))
            }
            Builtin::Values => {
                let arg = &args[0];
                let arg_value = self.eval_expr(arg)?;
                let arg_kind = arg_value.value.clone();
                let CheckValue::Dict {
                    entries,
                    value_type,
                    ..
                } = arg_value.value
                else {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        arg_value.path,
                        format!(
                            "values 需要 dict: 实际为 {}",
                            format_value_for_message(&arg_kind)
                        ),
                    );
                    return Err(EvalAbort::Error);
                };
                Ok(LocatedCheckValue::new(
                    CheckValue::Array {
                        items: entries.into_iter().map(|entry| entry.value).collect(),
                        element_type: value_type,
                    },
                    arg_value.path,
                ))
            }
            Builtin::Matches => {
                let value = self.eval_expr(&args[0])?;
                let value_kind = value.value.clone();
                let CheckValue::String(text) = value.value else {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        value.path,
                        format!(
                            "matches 的值不是 string: 实际为 {}",
                            format_value_for_message(&value_kind)
                        ),
                    );
                    return Err(EvalAbort::Error);
                };
                let CftSchemaCheckExprKind::String(pattern) = &args[1].kind else {
                    self.diag(
                        CfdErrorCode::CheckEvalTypeError,
                        "matches 的 pattern 必须是字符串字面量",
                    );
                    return Err(EvalAbort::Error);
                };
                let regex = Regex::new(pattern).map_err(|err| {
                    self.diag(
                        CfdErrorCode::CheckEvalTypeError,
                        format!("正则 pattern `{pattern}` 无法编译: {err}"),
                    );
                    EvalAbort::Error
                })?;
                Ok(LocatedCheckValue::new(
                    CheckValue::Bool(regex.is_match(&text)),
                    value.path,
                ))
            }
        }
    }

    fn eval_method_call(
        &mut self,
        receiver: &CftSchemaCheckExpr,
        name: &str,
        args: &[CftSchemaCheckExpr],
    ) -> EvalResult<LocatedCheckValue> {
        let Some(builtin) = Builtin::by_name(name) else {
            self.diag(
                CfdErrorCode::CheckEvalTypeError,
                format!("未知函数 `{name}`"),
            );
            return Err(EvalAbort::Error);
        };
        let expected_args = builtin.arity().saturating_sub(1);
        if args.len() != expected_args {
            self.diag(
                CfdErrorCode::CheckEvalTypeError,
                format!("{} 需要 {} 个参数", builtin.name(), expected_args),
            );
            return Err(EvalAbort::Error);
        }

        let receiver_value = self.eval_expr(receiver)?;
        match builtin {
            Builtin::Len => match receiver_value.value {
                CheckValue::Array { items, .. } => Ok(LocatedCheckValue::new(
                    CheckValue::Int(items.len() as i64),
                    receiver_value.path,
                )),
                CheckValue::Dict { entries, .. } => Ok(LocatedCheckValue::new(
                    CheckValue::Int(entries.len() as i64),
                    receiver_value.path,
                )),
                other => {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        receiver_value.path,
                        format!(
                            "len 需要 array 或 dict: 实际为 {}",
                            format_value_for_message(&other)
                        ),
                    );
                    Err(EvalAbort::Error)
                }
            },
            Builtin::Contains => {
                let value = self.eval_expr(&args[0])?;
                Ok(LocatedCheckValue::new(
                    CheckValue::Bool(self.contains_value(&receiver_value, &value.value)?),
                    receiver_value.path.clone(),
                ))
            }
            Builtin::Unique => {
                let arg_kind = receiver_value.value.clone();
                let CheckValue::Array { items, .. } = receiver_value.value else {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        receiver_value.path,
                        format!(
                            "unique 需要 array: 实际为 {}",
                            format_value_for_message(&arg_kind)
                        ),
                    );
                    return Err(EvalAbort::Error);
                };
                let mut seen = BTreeSet::new();
                for item in items {
                    let Some(key) = comparable_key(&item) else {
                        self.diag_at(
                            CfdErrorCode::CheckEvalTypeError,
                            receiver_value.path.clone(),
                            format!(
                                "unique 元素不可比较: 实际为 {}",
                                format_value_for_message(&item)
                            ),
                        );
                        return Err(EvalAbort::Error);
                    };
                    if !seen.insert(key) {
                        return Ok(LocatedCheckValue::new(
                            CheckValue::Bool(false),
                            receiver_value.path,
                        ));
                    }
                }
                Ok(LocatedCheckValue::new(
                    CheckValue::Bool(true),
                    receiver_value.path,
                ))
            }
            Builtin::Min | Builtin::Max => self.eval_min_max_value(builtin, receiver_value),
            Builtin::Sum => self.eval_sum_value(receiver_value),
            Builtin::Keys => {
                let arg_kind = receiver_value.value.clone();
                let CheckValue::Dict {
                    entries, key_type, ..
                } = receiver_value.value
                else {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        receiver_value.path,
                        format!(
                            "keys 需要 dict: 实际为 {}",
                            format_value_for_message(&arg_kind)
                        ),
                    );
                    return Err(EvalAbort::Error);
                };
                Ok(LocatedCheckValue::new(
                    CheckValue::Array {
                        items: entries.into_iter().map(|entry| *entry.key).collect(),
                        element_type: key_type,
                    },
                    receiver_value.path,
                ))
            }
            Builtin::Values => {
                let arg_kind = receiver_value.value.clone();
                let CheckValue::Dict {
                    entries,
                    value_type,
                    ..
                } = receiver_value.value
                else {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        receiver_value.path,
                        format!(
                            "values 需要 dict: 实际为 {}",
                            format_value_for_message(&arg_kind)
                        ),
                    );
                    return Err(EvalAbort::Error);
                };
                Ok(LocatedCheckValue::new(
                    CheckValue::Array {
                        items: entries.into_iter().map(|entry| entry.value).collect(),
                        element_type: value_type,
                    },
                    receiver_value.path,
                ))
            }
            Builtin::Matches => {
                let value_kind = receiver_value.value.clone();
                let CheckValue::String(text) = receiver_value.value else {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        receiver_value.path,
                        format!(
                            "matches 的值不是 string: 实际为 {}",
                            format_value_for_message(&value_kind)
                        ),
                    );
                    return Err(EvalAbort::Error);
                };
                let CftSchemaCheckExprKind::String(pattern) = &args[0].kind else {
                    self.diag(
                        CfdErrorCode::CheckEvalTypeError,
                        "matches 的 pattern 必须是字符串字面量",
                    );
                    return Err(EvalAbort::Error);
                };
                let regex = Regex::new(pattern).map_err(|err| {
                    self.diag(
                        CfdErrorCode::CheckEvalTypeError,
                        format!("正则 pattern `{pattern}` 无法编译: {err}"),
                    );
                    EvalAbort::Error
                })?;
                Ok(LocatedCheckValue::new(
                    CheckValue::Bool(regex.is_match(&text)),
                    receiver_value.path,
                ))
            }
        }
    }

    fn require_builtin_arity(
        &mut self,
        builtin: Builtin,
        args: &[CftSchemaCheckExpr],
    ) -> EvalResult<()> {
        if args.len() == builtin.arity() {
            return Ok(());
        }
        self.diag(
            CfdErrorCode::CheckEvalTypeError,
            format!("{} 需要 {} 个参数", builtin.name(), builtin.arity()),
        );
        Err(EvalAbort::Error)
    }

    fn exactly_one_arg<'b>(
        &mut self,
        args: &'b [CftSchemaCheckExpr],
        message: &str,
    ) -> EvalResult<&'b CftSchemaCheckExpr> {
        let [arg] = args else {
            self.diag(CfdErrorCode::CheckEvalTypeError, message);
            return Err(EvalAbort::Error);
        };
        Ok(arg)
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
        let arg_kind = arg_value.value.clone();
        let CheckValue::Array { items, .. } = arg_value.value else {
            self.diag_at(
                CfdErrorCode::CheckEvalTypeError,
                arg_value.path,
                format!(
                    "{} 需要 array: 实际为 {}",
                    builtin.name(),
                    format_value_for_message(&arg_kind)
                ),
            );
            return Err(EvalAbort::Error);
        };
        if items.is_empty() {
            self.diag_at(
                CfdErrorCode::CheckEmptyMinMax,
                arg_value.path,
                format!("{} 不能作用于空数组", builtin.name()),
            );
            return Err(EvalAbort::Error);
        }
        let mut non_null_items = items
            .iter()
            .filter(|item| !matches!(item, CheckValue::Null));
        let Some(mut out) = non_null_items.next().cloned() else {
            self.diag_at(
                CfdErrorCode::CheckEmptyMinMax,
                arg_value.path,
                format!(
                    "{} 不能作用于全 null 数组，长度为 {}",
                    builtin.name(),
                    items.len()
                ),
            );
            return Err(EvalAbort::Error);
        };
        for item in non_null_items {
            let ord = self.compare_order(&out, item, arg_value.path.clone())?;
            if (builtin == Builtin::Min && ord.is_gt()) || (builtin == Builtin::Max && ord.is_lt())
            {
                out = item.clone();
            }
        }
        Ok(LocatedCheckValue::new(out, arg_value.path))
    }

    fn eval_sum(&mut self, args: &[CftSchemaCheckExpr]) -> EvalResult<LocatedCheckValue> {
        let arg_value = self.eval_expr(&args[0])?;
        self.eval_sum_value(arg_value)
    }

    fn eval_sum_value(&mut self, arg_value: LocatedCheckValue) -> EvalResult<LocatedCheckValue> {
        let arg_kind = arg_value.value.clone();
        let CheckValue::Array {
            items,
            element_type,
        } = arg_value.value
        else {
            self.diag_at(
                CfdErrorCode::CheckEvalTypeError,
                arg_value.path,
                format!(
                    "sum 需要 array: 实际为 {}",
                    format_value_for_message(&arg_kind)
                ),
            );
            return Err(EvalAbort::Error);
        };
        let mut int_sum = 0_i64;
        let mut float_sum = 0.0_f64;
        let mut saw_float = false;
        let mut saw_numeric = false;
        for item in items {
            match item {
                CheckValue::Int(value) if !saw_float => {
                    saw_numeric = true;
                    let Some(next) = int_sum.checked_add(value) else {
                        self.diag_at(
                            CfdErrorCode::CheckEvalTypeError,
                            arg_value.path.clone(),
                            format!("整数求和溢出: {int_sum} + {value}"),
                        );
                        return Err(EvalAbort::Error);
                    };
                    int_sum = next;
                }
                CheckValue::Int(value) => {
                    saw_numeric = true;
                    float_sum += value as f64;
                }
                CheckValue::Float(value) => {
                    saw_numeric = true;
                    if !saw_float {
                        saw_float = true;
                        float_sum = int_sum as f64;
                    }
                    float_sum += value;
                }
                CheckValue::Null => {}
                other => {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        arg_value.path.clone(),
                        format!(
                            "sum 元素不是数值: 实际为 {}",
                            format_value_for_message(&other)
                        ),
                    );
                    return Err(EvalAbort::Error);
                }
            }
        }
        if saw_float || (!saw_numeric && type_ref_is_float(element_type.as_ref())) {
            Ok(LocatedCheckValue::new(
                CheckValue::Float(float_sum),
                arg_value.path,
            ))
        } else {
            Ok(LocatedCheckValue::new(
                CheckValue::Int(int_sum),
                arg_value.path,
            ))
        }
    }

    fn contains_value(
        &mut self,
        collection: &LocatedCheckValue,
        value: &CheckValue,
    ) -> EvalResult<bool> {
        match &collection.value {
            CheckValue::Array { items, .. } => {
                Ok(items.iter().any(|item| values_equal(item, value)))
            }
            CheckValue::Dict { entries, .. } => {
                let Some(key) = dict_key_from_check_value(value) else {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        collection.path.clone(),
                        format!(
                            "contains 的 dict key 无效: 实际为 {}",
                            format_value_for_message(value)
                        ),
                    );
                    return Err(EvalAbort::Error);
                };
                Ok(entries
                    .iter()
                    .any(|entry| entry.key_key() == Some(key.clone())))
            }
            other => {
                self.diag_at(
                    CfdErrorCode::CheckEvalTypeError,
                    collection.path.clone(),
                    format!(
                        "contains 需要 array 或 dict: 实际为 {}",
                        format_value_for_message(other)
                    ),
                );
                Err(EvalAbort::Error)
            }
        }
    }

    fn eval_unary(
        &mut self,
        op: CftSchemaUnaryOp,
        value: LocatedCheckValue,
    ) -> EvalResult<LocatedCheckValue> {
        let path = value.path;
        match (op, value.value) {
            (CftSchemaUnaryOp::Not, CheckValue::Bool(value)) => {
                Ok(LocatedCheckValue::new(CheckValue::Bool(!value), path))
            }
            (CftSchemaUnaryOp::Neg, CheckValue::Int(value)) => self.checked_int(
                value.checked_neg(),
                path,
                format!("整数取负溢出: -({value})"),
            ),
            (CftSchemaUnaryOp::Neg, CheckValue::Float(value)) => {
                Ok(LocatedCheckValue::new(CheckValue::Float(-value), path))
            }
            (CftSchemaUnaryOp::BitNot, CheckValue::Int(value)) => {
                Ok(LocatedCheckValue::new(CheckValue::Int(!value), path))
            }
            (CftSchemaUnaryOp::BitNot, CheckValue::Enum(value)) => Ok(LocatedCheckValue::new(
                CheckValue::Enum(self.enum_with_value(&value.enum_name, !value.value)),
                path,
            )),
            (op, value) => {
                self.diag_at(
                    CfdErrorCode::CheckEvalTypeError,
                    path,
                    format!(
                        "不支持的一元运算: {} 作用于 {}",
                        unary_op_str(op),
                        format_value_for_message(&value)
                    ),
                );
                Err(EvalAbort::Error)
            }
        }
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
                self.eval_eager_bin_op(op, lhs.value, rhs.value, path)
            }
        }
    }

    fn eval_eager_bin_op(
        &mut self,
        op: CftSchemaBinOp,
        lhs: CheckValue,
        rhs: CheckValue,
        path: Option<CfdPath>,
    ) -> EvalResult<LocatedCheckValue> {
        if matches!(lhs, CheckValue::Null) || matches!(rhs, CheckValue::Null) {
            self.diag_at(
                CfdErrorCode::CheckNullAccess,
                path,
                format!(
                    "不能对 null 执行二元运算: {} {} {}",
                    format_value_for_message(&lhs),
                    bin_op_str(op),
                    format_value_for_message(&rhs)
                ),
            );
            return Err(EvalAbort::Error);
        }
        match (op, lhs, rhs) {
            (CftSchemaBinOp::Add, CheckValue::Int(lhs), CheckValue::Int(rhs)) => self.checked_int(
                lhs.checked_add(rhs),
                path,
                format!("整数加法溢出: {lhs} + {rhs}"),
            ),
            (CftSchemaBinOp::Sub, CheckValue::Int(lhs), CheckValue::Int(rhs)) => self.checked_int(
                lhs.checked_sub(rhs),
                path,
                format!("整数减法溢出: {lhs} - {rhs}"),
            ),
            (CftSchemaBinOp::Mul, CheckValue::Int(lhs), CheckValue::Int(rhs)) => self.checked_int(
                lhs.checked_mul(rhs),
                path,
                format!("整数乘法溢出: {lhs} * {rhs}"),
            ),
            (CftSchemaBinOp::Div, CheckValue::Int(lhs), CheckValue::Int(rhs)) => self.checked_int(
                lhs.checked_div(rhs),
                path,
                format!("整数除法失败: {lhs} / {rhs}"),
            ),
            (CftSchemaBinOp::IntDiv, CheckValue::Int(lhs), CheckValue::Int(rhs)) => self
                .checked_int(
                    lhs.checked_div(rhs),
                    path,
                    format!("整数整除失败: {lhs} // {rhs}"),
                ),
            (CftSchemaBinOp::Mod, CheckValue::Int(lhs), CheckValue::Int(rhs)) => self.checked_int(
                lhs.checked_rem(rhs),
                path,
                format!("整数取模失败: {lhs} % {rhs}"),
            ),
            (CftSchemaBinOp::Pow, CheckValue::Int(lhs), CheckValue::Int(rhs)) => {
                match rhs.try_into().ok().and_then(|rhs| lhs.checked_pow(rhs)) {
                    Some(value) => Ok(LocatedCheckValue::new(CheckValue::Int(value), path)),
                    None => {
                        self.diag_at(
                            CfdErrorCode::CheckEvalTypeError,
                            path,
                            format!("整数幂运算失败: {lhs} ** {rhs}"),
                        );
                        Err(EvalAbort::Error)
                    }
                }
            }
            (CftSchemaBinOp::Shl, CheckValue::Int(lhs), CheckValue::Int(rhs)) => self
                .checked_shift(
                    i64::checked_shl,
                    lhs,
                    rhs,
                    path,
                    format!("整数左移失败: {lhs} << {rhs}"),
                ),
            (CftSchemaBinOp::Shr, CheckValue::Int(lhs), CheckValue::Int(rhs)) => self
                .checked_shift(
                    i64::checked_shr,
                    lhs,
                    rhs,
                    path,
                    format!("整数右移失败: {lhs} >> {rhs}"),
                ),
            (CftSchemaBinOp::Add, CheckValue::Float(lhs), CheckValue::Float(rhs)) => {
                Ok(LocatedCheckValue::new(CheckValue::Float(lhs + rhs), path))
            }
            (CftSchemaBinOp::Sub, CheckValue::Float(lhs), CheckValue::Float(rhs)) => {
                Ok(LocatedCheckValue::new(CheckValue::Float(lhs - rhs), path))
            }
            (CftSchemaBinOp::Mul, CheckValue::Float(lhs), CheckValue::Float(rhs)) => {
                Ok(LocatedCheckValue::new(CheckValue::Float(lhs * rhs), path))
            }
            (CftSchemaBinOp::Div, CheckValue::Float(lhs), CheckValue::Float(rhs)) => {
                Ok(LocatedCheckValue::new(CheckValue::Float(lhs / rhs), path))
            }
            (CftSchemaBinOp::Pow, CheckValue::Float(lhs), CheckValue::Float(rhs)) => Ok(
                LocatedCheckValue::new(CheckValue::Float(lhs.powf(rhs)), path),
            ),
            (CftSchemaBinOp::BitOr, CheckValue::Int(lhs), CheckValue::Int(rhs)) => {
                Ok(LocatedCheckValue::new(CheckValue::Int(lhs | rhs), path))
            }
            (CftSchemaBinOp::BitXor, CheckValue::Int(lhs), CheckValue::Int(rhs)) => {
                Ok(LocatedCheckValue::new(CheckValue::Int(lhs ^ rhs), path))
            }
            (CftSchemaBinOp::BitAnd, CheckValue::Int(lhs), CheckValue::Int(rhs)) => {
                Ok(LocatedCheckValue::new(CheckValue::Int(lhs & rhs), path))
            }
            (CftSchemaBinOp::BitOr, CheckValue::Enum(lhs), CheckValue::Enum(rhs))
                if lhs.enum_name == rhs.enum_name =>
            {
                let value = lhs.value | rhs.value;
                Ok(LocatedCheckValue::new(
                    CheckValue::Enum(self.enum_with_value(&lhs.enum_name, value)),
                    path,
                ))
            }
            (CftSchemaBinOp::BitXor, CheckValue::Enum(lhs), CheckValue::Enum(rhs))
                if lhs.enum_name == rhs.enum_name =>
            {
                let value = lhs.value ^ rhs.value;
                Ok(LocatedCheckValue::new(
                    CheckValue::Enum(self.enum_with_value(&lhs.enum_name, value)),
                    path,
                ))
            }
            (CftSchemaBinOp::BitAnd, CheckValue::Enum(lhs), CheckValue::Enum(rhs))
                if lhs.enum_name == rhs.enum_name =>
            {
                let value = lhs.value & rhs.value;
                Ok(LocatedCheckValue::new(
                    CheckValue::Enum(self.enum_with_value(&lhs.enum_name, value)),
                    path,
                ))
            }
            (op, lhs, rhs) => {
                self.diag_at(
                    CfdErrorCode::CheckEvalTypeError,
                    path,
                    format!(
                        "不支持的二元运算: {} {} {}",
                        format_value_for_message(&lhs),
                        bin_op_str(op),
                        format_value_for_message(&rhs)
                    ),
                );
                Err(EvalAbort::Error)
            }
        }
    }

    fn checked_int(
        &mut self,
        value: Option<i64>,
        path: Option<CfdPath>,
        message: impl Into<String>,
    ) -> EvalResult<LocatedCheckValue> {
        value
            .map(|value| LocatedCheckValue::new(CheckValue::Int(value), path.clone()))
            .ok_or_else(|| {
                self.diag_at(CfdErrorCode::CheckEvalTypeError, path, message);
                EvalAbort::Error
            })
    }

    fn checked_shift(
        &mut self,
        op: fn(i64, u32) -> Option<i64>,
        lhs: i64,
        rhs: i64,
        path: Option<CfdPath>,
        message: impl Into<String>,
    ) -> EvalResult<LocatedCheckValue> {
        let Some(rhs) = rhs.try_into().ok() else {
            self.diag_at(CfdErrorCode::CheckEvalTypeError, path, message);
            return Err(EvalAbort::Error);
        };
        self.checked_int(op(lhs, rhs), path, message)
    }

    fn compare(
        &mut self,
        op: CftSchemaCmpOp,
        lhs: &CheckValue,
        rhs: &CheckValue,
        path: Option<CfdPath>,
    ) -> EvalResult<bool> {
        Ok(match op {
            CftSchemaCmpOp::Eq => values_equal(lhs, rhs),
            CftSchemaCmpOp::Ne => !values_equal(lhs, rhs),
            CftSchemaCmpOp::Lt => self.compare_order(lhs, rhs, path)?.is_lt(),
            CftSchemaCmpOp::Le => !self.compare_order(lhs, rhs, path)?.is_gt(),
            CftSchemaCmpOp::Gt => self.compare_order(lhs, rhs, path)?.is_gt(),
            CftSchemaCmpOp::Ge => !self.compare_order(lhs, rhs, path)?.is_lt(),
        })
    }

    fn compare_order(
        &mut self,
        lhs: &CheckValue,
        rhs: &CheckValue,
        path: Option<CfdPath>,
    ) -> EvalResult<std::cmp::Ordering> {
        if matches!(lhs, CheckValue::Null) || matches!(rhs, CheckValue::Null) {
            self.diag_at(
                CfdErrorCode::CheckNullAccess,
                path,
                format!(
                    "不能对 null 做有序比较: {} cmp {}",
                    format_value_for_message(lhs),
                    format_value_for_message(rhs)
                ),
            );
            return Err(EvalAbort::Error);
        }
        match (lhs, rhs) {
            (CheckValue::Int(lhs), CheckValue::Int(rhs)) => Ok(lhs.cmp(rhs)),
            (CheckValue::Float(lhs), CheckValue::Float(rhs)) => {
                lhs.partial_cmp(rhs).ok_or_else(|| {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        path,
                        format!("float 比较失败: {lhs} cmp {rhs}"),
                    );
                    EvalAbort::Error
                })
            }
            (CheckValue::Enum(lhs), CheckValue::Enum(rhs)) if lhs.enum_name == rhs.enum_name => {
                Ok(lhs.value.cmp(&rhs.value))
            }
            _ => {
                self.diag_at(
                    CfdErrorCode::CheckEvalTypeError,
                    path,
                    format!(
                        "值不可做有序比较: {} cmp {}",
                        format_value_for_message(lhs),
                        format_value_for_message(rhs)
                    ),
                );
                Err(EvalAbort::Error)
            }
        }
    }

    fn enum_with_value(&self, enum_name: &str, value: i64) -> CfdEnumValue {
        match self.schema.enum_value_from_int(enum_name, value) {
            Some(enum_value) => enum_value,
            None => Self::anonymous_enum_value(enum_name, value),
        }
    }

    fn anonymous_enum_value(enum_name: &str, value: i64) -> CfdEnumValue {
        CfdEnumValue {
            enum_name: enum_name.to_string(),
            variant: None,
            value,
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

fn dimension_record_key(
    model: &CfdDataModel,
    record: &CheckRecordRef,
    field: &DimensionFieldMeta,
) -> Option<String> {
    if field.is_singleton {
        Some(field.source_field.clone())
    } else {
        record.key(model).map(str::to_string)
    }
}

fn unary_op_str(op: CftSchemaUnaryOp) -> &'static str {
    match op {
        CftSchemaUnaryOp::Not => "!",
        CftSchemaUnaryOp::BitNot => "~",
        CftSchemaUnaryOp::Neg => "-",
    }
}

fn bin_op_str(op: CftSchemaBinOp) -> &'static str {
    match op {
        CftSchemaBinOp::Or => "||",
        CftSchemaBinOp::And => "&&",
        CftSchemaBinOp::BitOr => "|",
        CftSchemaBinOp::BitXor => "^",
        CftSchemaBinOp::BitAnd => "&",
        CftSchemaBinOp::Add => "+",
        CftSchemaBinOp::Sub => "-",
        CftSchemaBinOp::Shl => "<<",
        CftSchemaBinOp::Shr => ">>",
        CftSchemaBinOp::Mul => "*",
        CftSchemaBinOp::Div => "/",
        CftSchemaBinOp::IntDiv => "//",
        CftSchemaBinOp::Mod => "%",
        CftSchemaBinOp::Pow => "**",
    }
}

fn cmp_op_str(op: CftSchemaCmpOp) -> &'static str {
    match op {
        CftSchemaCmpOp::Eq => "==",
        CftSchemaCmpOp::Ne => "!=",
        CftSchemaCmpOp::Lt => "<",
        CftSchemaCmpOp::Le => "<=",
        CftSchemaCmpOp::Gt => ">",
        CftSchemaCmpOp::Ge => ">=",
    }
}

fn render_stmt(stmt: &CftSchemaCheckStmt) -> String {
    match stmt {
        CftSchemaCheckStmt::Expr(expr) => render_expr(expr),
        CftSchemaCheckStmt::Quantifier {
            kind,
            binding,
            collection,
            body,
            ..
        } => {
            let kind = match kind {
                CftSchemaQuantifierKind::All => "all",
                CftSchemaQuantifierKind::Any => "any",
                CftSchemaQuantifierKind::None => "none",
            };
            let body = body.iter().map(render_stmt).collect::<Vec<_>>().join("; ");
            format!(
                "{kind} {binding} in {} {{ {body}; }}",
                render_expr(collection)
            )
        }
        CftSchemaCheckStmt::When {
            condition, body, ..
        } => {
            let body = body.iter().map(render_stmt).collect::<Vec<_>>().join("; ");
            format!("when {} {{ {body}; }}", render_expr(condition))
        }
    }
}

fn render_expr(expr: &CftSchemaCheckExpr) -> String {
    match &expr.kind {
        CftSchemaCheckExprKind::Int(value) => value.to_string(),
        CftSchemaCheckExprKind::Float(value) => value.to_string(),
        CftSchemaCheckExprKind::Bool(value) => value.to_string(),
        CftSchemaCheckExprKind::Null => "null".to_string(),
        CftSchemaCheckExprKind::String(value) => format!("\"{value}\""),
        CftSchemaCheckExprKind::Name(name) => name.clone(),
        CftSchemaCheckExprKind::Field { expr, name } => {
            format!("{}.{}", render_expr(expr), name)
        }
        CftSchemaCheckExprKind::Index { expr, index } => {
            format!("{}[{}]", render_expr(expr), render_expr(index))
        }
        CftSchemaCheckExprKind::Is { expr, predicate } => {
            let predicate = match predicate {
                CftSchemaTypePredicate::Type(name) => name.as_str(),
                CftSchemaTypePredicate::Null => "null",
            };
            format!("{} is {predicate}", render_expr(expr))
        }
        CftSchemaCheckExprKind::Call { name, args } => {
            let args = args.iter().map(render_expr).collect::<Vec<_>>().join(", ");
            format!("{name}({args})")
        }
        CftSchemaCheckExprKind::MethodCall {
            receiver,
            name,
            args,
        } => {
            let args = args.iter().map(render_expr).collect::<Vec<_>>().join(", ");
            format!("{}.{name}({args})", render_expr(receiver))
        }
        CftSchemaCheckExprKind::BinOp { op, lhs, rhs } => {
            format!(
                "{} {} {}",
                render_expr(lhs),
                bin_op_str(*op),
                render_expr(rhs)
            )
        }
        CftSchemaCheckExprKind::Unary { op, expr } => {
            format!("{}{}", unary_op_str(*op), render_expr(expr))
        }
        CftSchemaCheckExprKind::CmpChain { first, rest } => {
            let mut out = render_expr(first);
            for (op, expr) in rest {
                out.push(' ');
                out.push_str(cmp_op_str(*op));
                out.push(' ');
                out.push_str(&render_expr(expr));
            }
            out
        }
    }
}

fn format_cfd_path_for_message(path: &CfdPath) -> String {
    let mut out = String::new();
    for segment in &path.segments {
        match segment {
            CfdPathSegment::Field(name) => {
                if !out.is_empty() {
                    out.push('.');
                }
                out.push_str(name);
            }
            CfdPathSegment::Index(index) => {
                out.push('[');
                out.push_str(&index.to_string());
                out.push(']');
            }
            CfdPathSegment::DictKey(key) => {
                out.push('[');
                out.push_str(key);
                out.push(']');
            }
        }
    }
    if out.is_empty() {
        ".".to_string()
    } else {
        out
    }
}

fn one_line_message(message: &str) -> String {
    message
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("; ")
}

/// Render a `CheckValue` as a short token for inclusion in a diagnostic
/// message — strings are quoted, collections summarize, records show their key.
fn format_value_for_message(value: &CheckValue) -> String {
    match value {
        CheckValue::Null => "null".to_string(),
        CheckValue::Bool(v) => v.to_string(),
        CheckValue::Int(v) => v.to_string(),
        CheckValue::Float(v) => v.to_string(),
        CheckValue::String(s) => format!("\"{s}\""),
        CheckValue::Enum(e) => match &e.variant {
            Some(variant) => format!("{}.{}", e.enum_name, variant),
            None => format!("{}({})", e.enum_name, e.value),
        },
        CheckValue::EnumNamespace(name) => name.clone(),
        CheckValue::Record(_) => "<record>".to_string(),
        CheckValue::Entry(entry) => {
            format!("<entry key={}>", format_value_for_message(&entry.key))
        }
        CheckValue::Array { items, .. } => format!("<array len={}>", items.len()),
        CheckValue::Dict { entries, .. } => format!("<dict len={}>", entries.len()),
    }
}

fn type_ref_is_float(ty: Option<&CftSchemaTypeRef>) -> bool {
    match ty {
        Some(CftSchemaTypeRef::Float) => true,
        Some(CftSchemaTypeRef::Nullable(inner)) => type_ref_is_float(Some(inner)),
        _ => false,
    }
}
