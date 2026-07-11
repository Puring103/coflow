use std::collections::BTreeMap;

use coflow_cft::{
    CftSchemaBinOp, CftSchemaCheckExpr, CftSchemaCheckExprKind, CftSchemaCmpOp,
    CftSchemaTypePredicate,
};
use coflow_data_model::{CfdDataModel, CfdPath};

use super::diagnostics::format_value_for_message;
use super::value::{CheckValue, LocatedCheckValue};

type ExprKey = usize;

#[derive(Debug, Clone, Copy, Default)]
struct CaptureRequest {
    display: bool,
    bool_value: bool,
    actual_type: bool,
}

impl CaptureRequest {
    const DISPLAY: Self = Self {
        display: true,
        bool_value: false,
        actual_type: false,
    };
    const BOOL: Self = Self {
        display: false,
        bool_value: true,
        actual_type: false,
    };
    const ACTUAL_TYPE: Self = Self {
        display: false,
        bool_value: false,
        actual_type: true,
    };

    fn merge(&mut self, other: Self) {
        self.display |= other.display;
        self.bool_value |= other.bool_value;
        self.actual_type |= other.actual_type;
    }
}

#[derive(Debug, Clone)]
pub(super) struct TraceFact {
    pub(super) display: Option<String>,
    pub(super) bool_value: Option<bool>,
    pub(super) actual_type: Option<String>,
    pub(super) is_null: bool,
    pub(super) path: Option<CfdPath>,
}

#[derive(Debug, Clone)]
pub(super) struct ComparisonFailure {
    pub(super) lhs_expression: String,
    pub(super) lhs: TraceFact,
    pub(super) rhs_expression: String,
    pub(super) rhs: TraceFact,
    pub(super) op: CftSchemaCmpOp,
    pub(super) path: Option<CfdPath>,
}

#[derive(Debug, Default)]
pub(super) struct EvaluationTrace {
    requests: BTreeMap<ExprKey, CaptureRequest>,
    facts: BTreeMap<ExprKey, TraceFact>,
    comparison_failure: Option<ComparisonFailure>,
    unique_failures: BTreeMap<ExprKey, String>,
}

impl EvaluationTrace {
    pub(super) fn for_explanation(expr: &CftSchemaCheckExpr) -> Self {
        let mut trace = Self::default();
        match &expr.kind {
            CftSchemaCheckExprKind::Call { name, args }
                if matches!(name.as_str(), "contains" | "isUnique" | "matches") =>
            {
                if let Some(collection) = args.first() {
                    trace.request(collection, CaptureRequest::DISPLAY);
                }
            }
            CftSchemaCheckExprKind::MethodCall { receiver, name, .. }
                if matches!(name.as_str(), "contains" | "isUnique" | "matches") =>
            {
                trace.request(receiver, CaptureRequest::DISPLAY);
            }
            CftSchemaCheckExprKind::BinOp {
                op: CftSchemaBinOp::And | CftSchemaBinOp::Or,
                lhs,
                rhs,
            } => {
                trace.request(lhs, CaptureRequest::BOOL);
                trace.request(rhs, CaptureRequest::BOOL);
            }
            CftSchemaCheckExprKind::Is {
                expr: inner,
                predicate,
            } => match predicate {
                CftSchemaTypePredicate::Null => {
                    trace.request(inner, CaptureRequest::DISPLAY);
                }
                CftSchemaTypePredicate::Type(_) => {
                    trace.request(inner, CaptureRequest::ACTUAL_TYPE);
                }
            },
            CftSchemaCheckExprKind::CmpChain { first, rest } => {
                trace.request(first, CaptureRequest::DISPLAY);
                for (_, operand) in rest {
                    trace.request(operand, CaptureRequest::DISPLAY);
                }
            }
            _ => {}
        }
        trace
    }

    fn request(&mut self, expr: &CftSchemaCheckExpr, request: CaptureRequest) {
        self.requests
            .entry(expr_key(expr))
            .or_default()
            .merge(request);
    }

    pub(super) fn record(
        &mut self,
        expr: &CftSchemaCheckExpr,
        value: &LocatedCheckValue,
        model: &CfdDataModel,
    ) {
        let key = expr_key(expr);
        let Some(request) = self.requests.get(&key).copied() else {
            return;
        };
        self.facts.insert(key, trace_fact(request, value, model));
    }

    pub(super) fn fact(&self, expr: &CftSchemaCheckExpr) -> Option<&TraceFact> {
        self.facts.get(&expr_key(expr))
    }

    pub(super) fn note_comparison_failure(
        &mut self,
        lhs: &CftSchemaCheckExpr,
        op: CftSchemaCmpOp,
        rhs: &CftSchemaCheckExpr,
        path: Option<CfdPath>,
    ) {
        let lhs_expression = super::diagnostics::render_expr(lhs);
        let rhs_expression = super::diagnostics::render_expr(rhs);
        let Some(lhs) = self.fact(lhs).cloned() else {
            return;
        };
        let Some(rhs) = self.fact(rhs).cloned() else {
            return;
        };
        self.comparison_failure = Some(ComparisonFailure {
            lhs_expression,
            lhs,
            rhs_expression,
            rhs,
            op,
            path,
        });
    }

    pub(super) const fn comparison_failure(&self) -> Option<&ComparisonFailure> {
        self.comparison_failure.as_ref()
    }

    pub(super) fn note_unique_failure(
        &mut self,
        collection: &CftSchemaCheckExpr,
        detail: String,
    ) {
        self.unique_failures.insert(expr_key(collection), detail);
    }

    pub(super) fn unique_failure(&self, collection: &CftSchemaCheckExpr) -> Option<&str> {
        self.unique_failures
            .get(&expr_key(collection))
            .map(String::as_str)
    }
}

fn trace_fact(
    request: CaptureRequest,
    value: &LocatedCheckValue,
    model: &CfdDataModel,
) -> TraceFact {
    TraceFact {
        display: request
            .display
            .then(|| format_value_for_message(&value.value)),
        bool_value: if request.bool_value {
            match &value.value {
                CheckValue::Bool(value) => Some(*value),
                _ => None,
            }
        } else {
            None
        },
        actual_type: request
            .actual_type
            .then(|| value.value.actual_type(model).map(str::to_string))
            .flatten(),
        is_null: matches!(value.value, CheckValue::Null),
        path: value.path.clone(),
    }
}

fn expr_key(expr: &CftSchemaCheckExpr) -> ExprKey {
    std::ptr::from_ref(expr).addr()
}
