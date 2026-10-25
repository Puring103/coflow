use std::collections::BTreeMap;

use coflow_language::cft::{
    CftSchemaBinOp, CftSchemaCheckExpr, CftSchemaCheckExprKind, CftSchemaCmpOp,
    CftSchemaTypePredicate,
};
use coflow_model::CfdDataModel;

use super::format_value_for_message;
use crate::eval::{EvalValue, LocatedEvalValue, ScalarValue, TemporaryValue, ValueLocation};

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
pub(crate) struct TraceFact<'model> {
    display: Option<TraceDisplay<'model>>,
    pub(crate) bool_value: Option<bool>,
    pub(crate) actual_type: Option<&'model str>,
    pub(crate) location: Option<ValueLocation>,
}

impl TraceFact<'_> {
    pub(crate) fn display(&self) -> Option<String> {
        self.display.as_ref().map(TraceDisplay::render)
    }
}

// 成功断言只保留渲染所需的最小证据，不格式化标量，也不复制集合或记录成员列表。
#[derive(Debug, Clone)]
enum TraceDisplay<'model> {
    Model(&'model coflow_model::CfdValue),
    DictKey(&'model coflow_model::CfdDictKey),
    Temporary(TemporaryValue),
    Text(&'static str),
    Name(coflow_language::cft::EnumName),
    Array(usize, bool),
    Dict(usize, bool),
    Entry(Box<TraceDisplay<'model>>),
}

impl<'model> TraceDisplay<'model> {
    fn capture(value: &EvalValue<'model>) -> Self {
        use coflow_language::cft::CftConstValue;
        match value {
            EvalValue::Model(value, _) => Self::Model(value),
            EvalValue::DictKey(key) => Self::DictKey(key),
            EvalValue::Temporary(value) => Self::Temporary(value.clone()),
            EvalValue::Constant(value) => match value {
                CftConstValue::Array(items) => Self::Array(items.len(), true),
                CftConstValue::Dictionary(entries) => Self::Dict(entries.len(), true),
                CftConstValue::OptionNone => Self::Text("None"),
                CftConstValue::OptionSome(_) => Self::Text("Some(...)"),
                CftConstValue::Int(value) => Self::Temporary(TemporaryValue::Int(*value)),
                CftConstValue::Long(value) => Self::Temporary(TemporaryValue::Long(*value)),
                CftConstValue::Float(value) => Self::Temporary(TemporaryValue::Float(*value)),
                CftConstValue::Double(value) => Self::Temporary(TemporaryValue::Double(*value)),
                CftConstValue::Bool(value) => Self::Temporary(TemporaryValue::Bool(*value)),
                CftConstValue::String(value) => {
                    Self::Temporary(TemporaryValue::String(value.clone()))
                }
                _ => Self::Text("<constant>"),
            },
            EvalValue::EnumNamespace(name) => Self::Name(name.clone()),
            EvalValue::Record(_) => Self::Text("<record>"),
            EvalValue::Entry(entry) => Self::Entry(Box::new(Self::capture(&entry.key))),
            EvalValue::Array { items, .. } => Self::Array(items.len(), false),
            EvalValue::Dict { entries, .. } => Self::Dict(entries.len(), false),
            EvalValue::Dimension { base, .. } if value.scalar().is_some() => {
                Self::capture(&base.value)
            }
            EvalValue::Dimension { .. } => Self::Text("<dimension>"),
        }
    }

    fn render(&self) -> String {
        match self {
            Self::Model(value) => format_value_for_message(&EvalValue::Model(value, None)),
            Self::DictKey(key) => format_value_for_message(&EvalValue::DictKey(key)),
            Self::Temporary(value) => {
                format_value_for_message(&EvalValue::Temporary(value.clone()))
            }
            Self::Text(value) => (*value).to_owned(),
            Self::Name(name) => name.to_string(),
            Self::Array(len, true) => format!("<array constant len={len}>"),
            Self::Array(len, false) => format!("<array len={len}>"),
            Self::Dict(len, true) => format!("<dict constant len={len}>"),
            Self::Dict(len, false) => format!("<dict len={len}>"),
            Self::Entry(key) => format!("<entry key={}>", key.render()),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ComparisonFailure<'model> {
    pub(crate) lhs_expression: String,
    pub(crate) lhs: TraceFact<'model>,
    pub(crate) rhs_expression: String,
    pub(crate) rhs: TraceFact<'model>,
    pub(crate) op: CftSchemaCmpOp,
    pub(crate) location: Option<ValueLocation>,
}

#[derive(Debug, Default)]
pub(crate) struct EvaluationTrace<'model> {
    requests: BTreeMap<ExprKey, CaptureRequest>,
    facts: BTreeMap<ExprKey, TraceFact<'model>>,
    comparison_failure: Option<ComparisonFailure<'model>>,
    unique_failures: BTreeMap<ExprKey, String>,
}

impl<'model> EvaluationTrace<'model> {
    pub(crate) fn for_explanation(expr: &CftSchemaCheckExpr) -> Self {
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
                CftSchemaTypePredicate::Some { .. } => {
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

    pub(crate) fn record(
        &mut self,
        expr: &CftSchemaCheckExpr,
        value: &LocatedEvalValue<'model>,
        model: &'model CfdDataModel,
    ) {
        let key = expr_key(expr);
        let Some(request) = self.requests.get(&key).copied() else {
            return;
        };
        self.facts.insert(key, trace_fact(request, value, model));
    }

    pub(crate) fn fact(&self, expr: &CftSchemaCheckExpr) -> Option<&TraceFact<'model>> {
        self.facts.get(&expr_key(expr))
    }

    pub(crate) fn note_comparison_failure(
        &mut self,
        lhs: &CftSchemaCheckExpr,
        op: CftSchemaCmpOp,
        rhs: &CftSchemaCheckExpr,
        location: Option<ValueLocation>,
    ) {
        let lhs_expression = super::render_expr(lhs);
        let rhs_expression = super::render_expr(rhs);
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
            location,
        });
    }

    pub(crate) const fn comparison_failure(&self) -> Option<&ComparisonFailure<'model>> {
        self.comparison_failure.as_ref()
    }

    pub(crate) fn note_unique_failure(&mut self, collection: &CftSchemaCheckExpr, detail: String) {
        self.unique_failures.insert(expr_key(collection), detail);
    }

    pub(crate) fn unique_failure(&self, collection: &CftSchemaCheckExpr) -> Option<&str> {
        self.unique_failures
            .get(&expr_key(collection))
            .map(String::as_str)
    }
}

fn trace_fact<'model>(
    request: CaptureRequest,
    value: &LocatedEvalValue<'model>,
    model: &'model CfdDataModel,
) -> TraceFact<'model> {
    TraceFact {
        display: request.display.then(|| TraceDisplay::capture(&value.value)),
        bool_value: if request.bool_value {
            match value.value.scalar() {
                Some(ScalarValue::Bool(value)) => Some(value),
                _ => None,
            }
        } else {
            None
        },
        actual_type: request
            .actual_type
            .then(|| value.value.actual_type(model))
            .flatten(),
        location: value.location.clone(),
    }
}

fn expr_key(expr: &CftSchemaCheckExpr) -> ExprKey {
    std::ptr::from_ref(expr).addr()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deferred_evidence_preserves_scalar_and_collection_rendering() {
        let model_text = coflow_model::CfdValue::String("quoted \"text\"\nvalue".to_owned());
        let values = [
            EvalValue::Model(&model_text, None),
            EvalValue::int(-42),
            EvalValue::double(1.25),
            EvalValue::bool(false),
            EvalValue::string("temporary"),
            EvalValue::Constant(coflow_language::cft::CftConstValue::Array(vec![
                coflow_language::cft::CftConstValue::Int(1),
            ])),
            EvalValue::Constant(coflow_language::cft::CftConstValue::OptionNone),
        ];
        for value in values {
            assert_eq!(
                TraceDisplay::capture(&value).render(),
                format_value_for_message(&value)
            );
        }
        assert!(matches!(
            TraceDisplay::capture(&EvalValue::Model(&model_text, None)),
            TraceDisplay::Model(value) if std::ptr::eq(value, &raw const model_text)
        ));
    }
}
