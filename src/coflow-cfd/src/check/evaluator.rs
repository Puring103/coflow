use super::value::{comparable_key, dict_key_from_check_value, values_equal, CheckValue};
use crate::diagnostic::{CfdDiagnostic, CfdErrorCode, CfdPath};
use crate::model::{CfdDataModel, CfdEnumValue, CfdRecordId};
use crate::schema_view::SchemaView;
use coflow_cft::{
    CftSchemaBinOp, CftSchemaCheckBlock, CftSchemaCheckExpr, CftSchemaCheckExprKind,
    CftSchemaCheckStmt, CftSchemaCmpOp, CftSchemaQuantifierKind, CftSchemaTypePredicate,
    CftSchemaUnaryOp, Span,
};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct CheckEvaluator<'a> {
    schema: &'a SchemaView,
    model: &'a CfdDataModel,
    root_record: Option<CfdRecordId>,
    root_path: CfdPath,
    current: CheckValue,
    scopes: Vec<BTreeMap<String, CheckValue>>,
    pub(super) diagnostics: Vec<CfdDiagnostic>,
}

impl<'a> CheckEvaluator<'a> {
    pub(super) fn new(
        schema: &'a SchemaView,
        model: &'a CfdDataModel,
        root_record: Option<CfdRecordId>,
        root_path: CfdPath,
        current: CheckValue,
    ) -> Self {
        Self {
            schema,
            model,
            root_record,
            root_path,
            current,
            scopes: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    pub(super) fn eval_check_block(&mut self, check: &CftSchemaCheckBlock) {
        self.eval_stmts(&check.stmts);
    }

    fn eval_stmts(&mut self, stmts: &[CftSchemaCheckStmt]) {
        for stmt in stmts {
            self.eval_stmt(stmt);
        }
    }

    fn eval_stmt(&mut self, stmt: &CftSchemaCheckStmt) {
        match stmt {
            CftSchemaCheckStmt::Expr(expr) => match self.eval_expr(expr) {
                Ok(CheckValue::Bool(true)) => {}
                Ok(CheckValue::Bool(false)) => self.diag(
                    CfdErrorCode::CheckFailed,
                    expr.span,
                    "check condition evaluated to false",
                ),
                Ok(_) => self.diag(
                    CfdErrorCode::CheckEvalTypeError,
                    expr.span,
                    "check expression did not evaluate to bool",
                ),
                Err(()) => {}
            },
            CftSchemaCheckStmt::When {
                condition, body, ..
            } => match self.eval_expr(condition) {
                Ok(CheckValue::Bool(true)) => self.eval_stmts(body),
                Ok(CheckValue::Bool(false)) => {}
                Ok(_) => self.diag(
                    CfdErrorCode::CheckEvalTypeError,
                    condition.span,
                    "when condition did not evaluate to bool",
                ),
                Err(()) => {}
            },
            CftSchemaCheckStmt::Quantifier {
                kind,
                binding,
                collection,
                body,
                span,
            } => {
                let Ok(collection) = self.eval_expr(collection) else {
                    return;
                };
                let Some(items) = self.quantifier_items(collection, *span) else {
                    return;
                };
                self.eval_quantifier(*kind, binding, &items, body, *span);
            }
        }
    }

    fn eval_quantifier(
        &mut self,
        kind: CftSchemaQuantifierKind,
        binding: &str,
        items: &[CheckValue],
        body: &[CftSchemaCheckStmt],
        span: Span,
    ) {
        let mut matched = 0_usize;
        for item in items {
            let diagnostic_start = self.diagnostics.len();
            let mut scope = BTreeMap::new();
            scope.insert(binding.to_string(), item.clone());
            self.scopes.push(scope);
            self.eval_stmts(body);
            let passed = self.diagnostics.len() == diagnostic_start;
            let _ = self.scopes.pop();

            match kind {
                CftSchemaQuantifierKind::All => {}
                CftSchemaQuantifierKind::Any | CftSchemaQuantifierKind::None => {
                    self.diagnostics.truncate(diagnostic_start);
                }
            }

            if passed {
                matched += 1;
            }
        }

        match kind {
            CftSchemaQuantifierKind::All => {}
            CftSchemaQuantifierKind::Any if matched == 0 => self.diag(
                CfdErrorCode::CheckFailed,
                span,
                "any quantifier did not match any element",
            ),
            CftSchemaQuantifierKind::Any => {}
            CftSchemaQuantifierKind::None if matched > 0 => self.diag(
                CfdErrorCode::CheckFailed,
                span,
                "none quantifier matched at least one element",
            ),
            CftSchemaQuantifierKind::None => {}
        }
    }

    fn quantifier_items(&mut self, collection: CheckValue, span: Span) -> Option<Vec<CheckValue>> {
        match collection {
            CheckValue::Array(items) => Some(items),
            CheckValue::Dict(entries) => Some(
                entries
                    .into_iter()
                    .map(|entry| CheckValue::Entry(Box::new(entry)))
                    .collect(),
            ),
            _ => {
                self.diag(
                    CfdErrorCode::CheckEvalTypeError,
                    span,
                    "quantifier target is not a collection",
                );
                None
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    fn eval_expr(&mut self, expr: &CftSchemaCheckExpr) -> Result<CheckValue, ()> {
        match &expr.kind {
            CftSchemaCheckExprKind::Int(value) => Ok(CheckValue::Int(*value)),
            CftSchemaCheckExprKind::Float(value) => Ok(CheckValue::Float(*value)),
            CftSchemaCheckExprKind::Bool(value) => Ok(CheckValue::Bool(*value)),
            CftSchemaCheckExprKind::Null => Ok(CheckValue::Null),
            CftSchemaCheckExprKind::String(value) => Ok(CheckValue::String(value.clone())),
            CftSchemaCheckExprKind::Name(name) => self.eval_name(name, expr.span),
            CftSchemaCheckExprKind::Field { expr: inner, name } => {
                if let CftSchemaCheckExprKind::Name(enum_name) = &inner.kind {
                    if let Some(enum_value) = self.schema.enum_variant_value(enum_name, name) {
                        return Ok(CheckValue::Enum(CfdEnumValue {
                            enum_name: enum_name.clone(),
                            variant: name.clone(),
                            value: enum_value,
                        }));
                    }
                }
                let target = self.eval_expr(inner)?;
                self.eval_field(target, name, expr.span)
            }
            CftSchemaCheckExprKind::Index { expr: inner, index } => {
                let target = self.eval_expr(inner)?;
                let index = self.eval_expr(index)?;
                self.eval_index(target, index, expr.span)
            }
            CftSchemaCheckExprKind::Is {
                expr: inner,
                predicate,
            } => {
                let value = self.eval_expr(inner)?;
                Ok(CheckValue::Bool(self.eval_is(&value, predicate)))
            }
            CftSchemaCheckExprKind::Call { name, args } => self.eval_call(name, args, expr.span),
            CftSchemaCheckExprKind::BinOp { op, lhs, rhs } => {
                self.eval_bin_op(*op, lhs, rhs, expr.span)
            }
            CftSchemaCheckExprKind::Unary { op, expr: inner } => {
                let value = self.eval_expr(inner)?;
                self.eval_unary(*op, value, expr.span)
            }
            CftSchemaCheckExprKind::CmpChain { first, rest } => {
                let mut lhs = self.eval_expr(first)?;
                for (op, rhs_expr) in rest {
                    let rhs = self.eval_expr(rhs_expr)?;
                    if !self.compare(*op, &lhs, &rhs, rhs_expr.span)? {
                        return Ok(CheckValue::Bool(false));
                    }
                    lhs = rhs;
                }
                Ok(CheckValue::Bool(true))
            }
        }
    }

    fn eval_name(&mut self, name: &str, span: Span) -> Result<CheckValue, ()> {
        for scope in self.scopes.iter().rev() {
            if let Some(value) = scope.get(name) {
                return Ok(value.clone());
            }
        }
        if let Some(value) = self.current.field(self.model, name) {
            return Ok(value);
        }
        if let Some(value) = self.schema.consts.get(name) {
            return Ok(CheckValue::from_const(value));
        }
        if self.schema.enums.contains_key(name) {
            return Ok(CheckValue::EnumNamespace(name.to_string()));
        }
        self.diag(
            CfdErrorCode::CheckEvalTypeError,
            span,
            format!("unknown check value `{name}`"),
        );
        Err(())
    }

    fn eval_field(&mut self, target: CheckValue, name: &str, span: Span) -> Result<CheckValue, ()> {
        if matches!(target, CheckValue::Null) {
            self.diag(
                CfdErrorCode::CheckNullAccess,
                span,
                "field access on null value",
            );
            return Err(());
        }
        match target {
            CheckValue::Record(record) => record.field(self.model, name).ok_or_else(|| {
                self.diag(
                    CfdErrorCode::CheckEvalTypeError,
                    span,
                    format!("record has no field `{name}`"),
                );
            }),
            CheckValue::Entry(entry) => match name {
                "key" => Ok(*entry.key),
                "value" => Ok(entry.value),
                _ => {
                    self.diag(
                        CfdErrorCode::CheckEvalTypeError,
                        span,
                        format!("dict entry has no field `{name}`"),
                    );
                    Err(())
                }
            },
            _ => {
                self.diag(
                    CfdErrorCode::CheckEvalTypeError,
                    span,
                    "field access target is not an object",
                );
                Err(())
            }
        }
    }

    fn eval_index(
        &mut self,
        target: CheckValue,
        index: CheckValue,
        span: Span,
    ) -> Result<CheckValue, ()> {
        if matches!(target, CheckValue::Null) {
            self.diag(
                CfdErrorCode::CheckNullAccess,
                span,
                "index access on null value",
            );
            return Err(());
        }
        match target {
            CheckValue::Array(items) => {
                let CheckValue::Int(index) = index else {
                    self.diag(
                        CfdErrorCode::CheckEvalTypeError,
                        span,
                        "array index is not int",
                    );
                    return Err(());
                };
                let Ok(index) = usize::try_from(index) else {
                    self.diag(
                        CfdErrorCode::CheckIndexOutOfBounds,
                        span,
                        "array index is negative",
                    );
                    return Err(());
                };
                items.get(index).cloned().ok_or_else(|| {
                    self.diag(
                        CfdErrorCode::CheckIndexOutOfBounds,
                        span,
                        "array index is out of bounds",
                    );
                })
            }
            CheckValue::Dict(entries) => {
                let Some(key) = dict_key_from_check_value(&index) else {
                    self.diag(
                        CfdErrorCode::CheckEvalTypeError,
                        span,
                        "dict index is not a valid key",
                    );
                    return Err(());
                };
                entries
                    .into_iter()
                    .find(|entry| entry.key_key().is_some_and(|entry_key| entry_key == key))
                    .map(|entry| entry.value)
                    .ok_or_else(|| {
                        self.diag(
                            CfdErrorCode::CheckMissingDictKey,
                            span,
                            "dict key is missing",
                        );
                    })
            }
            _ => {
                self.diag(
                    CfdErrorCode::CheckEvalTypeError,
                    span,
                    "index target is not a collection",
                );
                Err(())
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
        span: Span,
    ) -> Result<CheckValue, ()> {
        if self.schema.enums.contains_key(name) {
            let Some(arg) = args.first() else {
                self.diag(
                    CfdErrorCode::CheckEvalTypeError,
                    span,
                    "missing enum constructor arg",
                );
                return Err(());
            };
            let CheckValue::Int(value) = self.eval_expr(arg)? else {
                self.diag(
                    CfdErrorCode::CheckEvalTypeError,
                    arg.span,
                    "enum constructor arg is not int",
                );
                return Err(());
            };
            return Ok(CheckValue::Enum(
                self.schema
                    .enum_value_from_int(name, value)
                    .unwrap_or(CfdEnumValue {
                        enum_name: name.to_string(),
                        variant: value.to_string(),
                        value,
                    }),
            ));
        }

        match name {
            "len" => {
                let Some(arg) = args.first() else {
                    self.diag(
                        CfdErrorCode::CheckEvalTypeError,
                        span,
                        "len expects one argument",
                    );
                    return Err(());
                };
                match self.eval_expr(arg)? {
                    CheckValue::Array(items) => Ok(CheckValue::Int(items.len() as i64)),
                    CheckValue::Dict(entries) => Ok(CheckValue::Int(entries.len() as i64)),
                    _ => {
                        self.diag(
                            CfdErrorCode::CheckEvalTypeError,
                            arg.span,
                            "len expects array or dict",
                        );
                        Err(())
                    }
                }
            }
            "contains" => {
                let [collection, value] = args else {
                    self.diag(
                        CfdErrorCode::CheckEvalTypeError,
                        span,
                        "contains expects two arguments",
                    );
                    return Err(());
                };
                let collection = self.eval_expr(collection)?;
                let value = self.eval_expr(value)?;
                Ok(CheckValue::Bool(self.contains_value(&collection, &value)))
            }
            "unique" => {
                let Some(arg) = args.first() else {
                    self.diag(
                        CfdErrorCode::CheckEvalTypeError,
                        span,
                        "unique expects one argument",
                    );
                    return Err(());
                };
                let CheckValue::Array(items) = self.eval_expr(arg)? else {
                    self.diag(
                        CfdErrorCode::CheckEvalTypeError,
                        arg.span,
                        "unique expects array",
                    );
                    return Err(());
                };
                let mut seen = BTreeSet::new();
                for item in items {
                    let Some(key) = comparable_key(&item) else {
                        self.diag(
                            CfdErrorCode::CheckEvalTypeError,
                            arg.span,
                            "unique element is not comparable",
                        );
                        return Err(());
                    };
                    if !seen.insert(key) {
                        return Ok(CheckValue::Bool(false));
                    }
                }
                Ok(CheckValue::Bool(true))
            }
            "min" | "max" => self.eval_min_max(name, args, span),
            "sum" => self.eval_sum(args, span),
            "keys" => {
                let Some(arg) = args.first() else {
                    self.diag(
                        CfdErrorCode::CheckEvalTypeError,
                        span,
                        "keys expects one argument",
                    );
                    return Err(());
                };
                let CheckValue::Dict(entries) = self.eval_expr(arg)? else {
                    self.diag(
                        CfdErrorCode::CheckEvalTypeError,
                        arg.span,
                        "keys expects dict",
                    );
                    return Err(());
                };
                Ok(CheckValue::Array(
                    entries.into_iter().map(|entry| *entry.key).collect(),
                ))
            }
            "values" => {
                let Some(arg) = args.first() else {
                    self.diag(
                        CfdErrorCode::CheckEvalTypeError,
                        span,
                        "values expects one argument",
                    );
                    return Err(());
                };
                let CheckValue::Dict(entries) = self.eval_expr(arg)? else {
                    self.diag(
                        CfdErrorCode::CheckEvalTypeError,
                        arg.span,
                        "values expects dict",
                    );
                    return Err(());
                };
                Ok(CheckValue::Array(
                    entries.into_iter().map(|entry| entry.value).collect(),
                ))
            }
            "matches" => {
                let [value, pattern_expr] = args else {
                    self.diag(
                        CfdErrorCode::CheckEvalTypeError,
                        span,
                        "matches expects two arguments",
                    );
                    return Err(());
                };
                let CheckValue::String(value) = self.eval_expr(value)? else {
                    self.diag(
                        CfdErrorCode::CheckEvalTypeError,
                        value.span,
                        "matches value is not string",
                    );
                    return Err(());
                };
                let CftSchemaCheckExprKind::String(pattern) = &pattern_expr.kind else {
                    self.diag(
                        CfdErrorCode::CheckEvalTypeError,
                        pattern_expr.span,
                        "matches pattern is not literal",
                    );
                    return Err(());
                };
                let regex = Regex::new(pattern).map_err(|_| {
                    self.diag(
                        CfdErrorCode::CheckInvalidRegex,
                        pattern_expr.span,
                        "regex pattern cannot be compiled",
                    );
                })?;
                Ok(CheckValue::Bool(regex.is_match(&value)))
            }
            _ => {
                self.diag(
                    CfdErrorCode::CheckEvalTypeError,
                    span,
                    format!("unknown function `{name}`"),
                );
                Err(())
            }
        }
    }

    fn eval_min_max(
        &mut self,
        name: &str,
        args: &[CftSchemaCheckExpr],
        span: Span,
    ) -> Result<CheckValue, ()> {
        let Some(arg) = args.first() else {
            self.diag(
                CfdErrorCode::CheckEvalTypeError,
                span,
                "min/max expects one argument",
            );
            return Err(());
        };
        let CheckValue::Array(items) = self.eval_expr(arg)? else {
            self.diag(
                CfdErrorCode::CheckEvalTypeError,
                arg.span,
                "min/max expects array",
            );
            return Err(());
        };
        let Some(mut out) = items.first().cloned() else {
            self.diag(
                CfdErrorCode::CheckEmptyMinMax,
                span,
                "min/max called on empty array",
            );
            return Err(());
        };
        for item in items.iter().skip(1) {
            let ord = self.compare_order(&out, item, span)?;
            if (name == "min" && ord.is_gt()) || (name == "max" && ord.is_lt()) {
                out = item.clone();
            }
        }
        Ok(out)
    }

    fn eval_sum(&mut self, args: &[CftSchemaCheckExpr], span: Span) -> Result<CheckValue, ()> {
        let Some(arg) = args.first() else {
            self.diag(
                CfdErrorCode::CheckEvalTypeError,
                span,
                "sum expects one argument",
            );
            return Err(());
        };
        let CheckValue::Array(items) = self.eval_expr(arg)? else {
            self.diag(
                CfdErrorCode::CheckEvalTypeError,
                arg.span,
                "sum expects array",
            );
            return Err(());
        };
        let mut int_sum = 0_i64;
        let mut float_sum = 0.0_f64;
        let mut saw_float = false;
        for item in items {
            match item {
                CheckValue::Int(value) if !saw_float => int_sum = int_sum.saturating_add(value),
                CheckValue::Int(value) => float_sum += value as f64,
                CheckValue::Float(value) => {
                    if !saw_float {
                        saw_float = true;
                        float_sum = int_sum as f64;
                    }
                    float_sum += value;
                }
                _ => {
                    self.diag(
                        CfdErrorCode::CheckEvalTypeError,
                        arg.span,
                        "sum item is not numeric",
                    );
                    return Err(());
                }
            }
        }
        if saw_float {
            Ok(CheckValue::Float(float_sum))
        } else {
            Ok(CheckValue::Int(int_sum))
        }
    }

    fn contains_value(&mut self, collection: &CheckValue, value: &CheckValue) -> bool {
        match collection {
            CheckValue::Array(items) => items.iter().any(|item| values_equal(item, value)),
            CheckValue::Dict(entries) => dict_key_from_check_value(value).is_some_and(|key| {
                entries
                    .iter()
                    .any(|entry| entry.key_key() == Some(key.clone()))
            }),
            _ => false,
        }
    }

    fn eval_unary(
        &mut self,
        op: CftSchemaUnaryOp,
        value: CheckValue,
        span: Span,
    ) -> Result<CheckValue, ()> {
        match (op, value) {
            (CftSchemaUnaryOp::Not, CheckValue::Bool(value)) => Ok(CheckValue::Bool(!value)),
            (CftSchemaUnaryOp::Neg, CheckValue::Int(value)) => Ok(CheckValue::Int(-value)),
            (CftSchemaUnaryOp::Neg, CheckValue::Float(value)) => Ok(CheckValue::Float(-value)),
            (CftSchemaUnaryOp::BitNot, CheckValue::Int(value)) => Ok(CheckValue::Int(!value)),
            (CftSchemaUnaryOp::BitNot, CheckValue::Enum(value)) => Ok(CheckValue::Enum(
                self.enum_with_value(&value.enum_name, !value.value),
            )),
            _ => {
                self.diag(
                    CfdErrorCode::CheckEvalTypeError,
                    span,
                    "unsupported unary operation",
                );
                Err(())
            }
        }
    }

    fn eval_bin_op(
        &mut self,
        op: CftSchemaBinOp,
        lhs: &CftSchemaCheckExpr,
        rhs: &CftSchemaCheckExpr,
        span: Span,
    ) -> Result<CheckValue, ()> {
        match op {
            CftSchemaBinOp::Or => {
                let lhs = self.eval_expr(lhs)?;
                let CheckValue::Bool(lhs) = lhs else {
                    self.diag(CfdErrorCode::CheckEvalTypeError, span, "lhs is not bool");
                    return Err(());
                };
                if lhs {
                    return Ok(CheckValue::Bool(true));
                }
                let rhs = self.eval_expr(rhs)?;
                let CheckValue::Bool(rhs) = rhs else {
                    self.diag(CfdErrorCode::CheckEvalTypeError, span, "rhs is not bool");
                    return Err(());
                };
                Ok(CheckValue::Bool(rhs))
            }
            CftSchemaBinOp::And => {
                let lhs = self.eval_expr(lhs)?;
                let CheckValue::Bool(lhs) = lhs else {
                    self.diag(CfdErrorCode::CheckEvalTypeError, span, "lhs is not bool");
                    return Err(());
                };
                if !lhs {
                    return Ok(CheckValue::Bool(false));
                }
                let rhs = self.eval_expr(rhs)?;
                let CheckValue::Bool(rhs) = rhs else {
                    self.diag(CfdErrorCode::CheckEvalTypeError, span, "rhs is not bool");
                    return Err(());
                };
                Ok(CheckValue::Bool(rhs))
            }
            _ => {
                let lhs = self.eval_expr(lhs)?;
                let rhs = self.eval_expr(rhs)?;
                self.eval_eager_bin_op(op, lhs, rhs, span)
            }
        }
    }

    fn eval_eager_bin_op(
        &mut self,
        op: CftSchemaBinOp,
        lhs: CheckValue,
        rhs: CheckValue,
        span: Span,
    ) -> Result<CheckValue, ()> {
        match (op, lhs, rhs) {
            (CftSchemaBinOp::Add, CheckValue::Int(lhs), CheckValue::Int(rhs)) => {
                self.checked_int(lhs.checked_add(rhs), span, "integer addition overflow")
            }
            (CftSchemaBinOp::Sub, CheckValue::Int(lhs), CheckValue::Int(rhs)) => {
                self.checked_int(lhs.checked_sub(rhs), span, "integer subtraction overflow")
            }
            (CftSchemaBinOp::Mul, CheckValue::Int(lhs), CheckValue::Int(rhs)) => self.checked_int(
                lhs.checked_mul(rhs),
                span,
                "integer multiplication overflow",
            ),
            (CftSchemaBinOp::Div, CheckValue::Int(lhs), CheckValue::Int(rhs)) => {
                self.checked_int(lhs.checked_div(rhs), span, "integer division failed")
            }
            (CftSchemaBinOp::IntDiv, CheckValue::Int(lhs), CheckValue::Int(rhs)) => {
                self.checked_int(lhs.checked_div(rhs), span, "integer division failed")
            }
            (CftSchemaBinOp::Mod, CheckValue::Int(lhs), CheckValue::Int(rhs)) => {
                self.checked_int(lhs.checked_rem(rhs), span, "integer modulo failed")
            }
            (CftSchemaBinOp::Pow, CheckValue::Int(lhs), CheckValue::Int(rhs)) => {
                match rhs.try_into().ok().and_then(|rhs| lhs.checked_pow(rhs)) {
                    Some(value) => Ok(CheckValue::Int(value)),
                    None => {
                        self.diag(
                            CfdErrorCode::CheckEvalTypeError,
                            span,
                            "integer power failed",
                        );
                        Err(())
                    }
                }
            }
            (CftSchemaBinOp::Shl, CheckValue::Int(lhs), CheckValue::Int(rhs)) => self
                .checked_shift(
                    i64::checked_shl,
                    lhs,
                    rhs,
                    span,
                    "integer shift left failed",
                ),
            (CftSchemaBinOp::Shr, CheckValue::Int(lhs), CheckValue::Int(rhs)) => self
                .checked_shift(
                    i64::checked_shr,
                    lhs,
                    rhs,
                    span,
                    "integer shift right failed",
                ),
            (CftSchemaBinOp::Add, CheckValue::Float(lhs), CheckValue::Float(rhs)) => {
                Ok(CheckValue::Float(lhs + rhs))
            }
            (CftSchemaBinOp::Sub, CheckValue::Float(lhs), CheckValue::Float(rhs)) => {
                Ok(CheckValue::Float(lhs - rhs))
            }
            (CftSchemaBinOp::Mul, CheckValue::Float(lhs), CheckValue::Float(rhs)) => {
                Ok(CheckValue::Float(lhs * rhs))
            }
            (CftSchemaBinOp::Div, CheckValue::Float(lhs), CheckValue::Float(rhs)) => {
                Ok(CheckValue::Float(lhs / rhs))
            }
            (CftSchemaBinOp::Pow, CheckValue::Float(lhs), CheckValue::Float(rhs)) => {
                Ok(CheckValue::Float(lhs.powf(rhs)))
            }
            (CftSchemaBinOp::BitOr, CheckValue::Int(lhs), CheckValue::Int(rhs)) => {
                Ok(CheckValue::Int(lhs | rhs))
            }
            (CftSchemaBinOp::BitXor, CheckValue::Int(lhs), CheckValue::Int(rhs)) => {
                Ok(CheckValue::Int(lhs ^ rhs))
            }
            (CftSchemaBinOp::BitAnd, CheckValue::Int(lhs), CheckValue::Int(rhs)) => {
                Ok(CheckValue::Int(lhs & rhs))
            }
            (
                op @ (CftSchemaBinOp::BitOr | CftSchemaBinOp::BitXor | CftSchemaBinOp::BitAnd),
                CheckValue::Enum(lhs),
                CheckValue::Enum(rhs),
            ) if lhs.enum_name == rhs.enum_name => {
                let value = match op {
                    CftSchemaBinOp::BitOr => lhs.value | rhs.value,
                    CftSchemaBinOp::BitXor => lhs.value ^ rhs.value,
                    CftSchemaBinOp::BitAnd => lhs.value & rhs.value,
                    _ => unreachable!(),
                };
                Ok(CheckValue::Enum(
                    self.enum_with_value(&lhs.enum_name, value),
                ))
            }
            _ => {
                self.diag(
                    CfdErrorCode::CheckEvalTypeError,
                    span,
                    "unsupported binary operation",
                );
                Err(())
            }
        }
    }

    fn checked_int(
        &mut self,
        value: Option<i64>,
        span: Span,
        message: impl Into<String>,
    ) -> Result<CheckValue, ()> {
        value.map(CheckValue::Int).ok_or_else(|| {
            self.diag(CfdErrorCode::CheckEvalTypeError, span, message);
        })
    }

    fn checked_shift(
        &mut self,
        op: fn(i64, u32) -> Option<i64>,
        lhs: i64,
        rhs: i64,
        span: Span,
        message: impl Into<String>,
    ) -> Result<CheckValue, ()> {
        let Some(rhs) = rhs.try_into().ok() else {
            self.diag(CfdErrorCode::CheckEvalTypeError, span, message);
            return Err(());
        };
        self.checked_int(op(lhs, rhs), span, message)
    }

    fn compare(
        &mut self,
        op: CftSchemaCmpOp,
        lhs: &CheckValue,
        rhs: &CheckValue,
        span: Span,
    ) -> Result<bool, ()> {
        Ok(match op {
            CftSchemaCmpOp::Eq => values_equal(lhs, rhs),
            CftSchemaCmpOp::Ne => !values_equal(lhs, rhs),
            CftSchemaCmpOp::Lt => self.compare_order(lhs, rhs, span)?.is_lt(),
            CftSchemaCmpOp::Le => !self.compare_order(lhs, rhs, span)?.is_gt(),
            CftSchemaCmpOp::Gt => self.compare_order(lhs, rhs, span)?.is_gt(),
            CftSchemaCmpOp::Ge => !self.compare_order(lhs, rhs, span)?.is_lt(),
        })
    }

    fn compare_order(
        &mut self,
        lhs: &CheckValue,
        rhs: &CheckValue,
        span: Span,
    ) -> Result<std::cmp::Ordering, ()> {
        match (lhs, rhs) {
            (CheckValue::Int(lhs), CheckValue::Int(rhs)) => Ok(lhs.cmp(rhs)),
            (CheckValue::Float(lhs), CheckValue::Float(rhs)) => {
                lhs.partial_cmp(rhs).ok_or_else(|| {
                    self.diag(
                        CfdErrorCode::CheckEvalTypeError,
                        span,
                        "float comparison failed",
                    );
                })
            }
            (CheckValue::Enum(lhs), CheckValue::Enum(rhs)) if lhs.enum_name == rhs.enum_name => {
                Ok(lhs.value.cmp(&rhs.value))
            }
            _ => {
                self.diag(
                    CfdErrorCode::CheckEvalTypeError,
                    span,
                    "values are not ordered comparable",
                );
                Err(())
            }
        }
    }

    fn enum_with_value(&self, enum_name: &str, value: i64) -> CfdEnumValue {
        self.schema
            .enum_value_from_int(enum_name, value)
            .unwrap_or(CfdEnumValue {
                enum_name: enum_name.to_string(),
                variant: value.to_string(),
                value,
            })
    }

    fn diag(&mut self, code: CfdErrorCode, _span: Span, message: impl Into<String>) {
        self.diagnostics.push(
            CfdDiagnostic::error(code, message)
                .with_primary(self.root_record, self.root_path.clone()),
        );
    }
}
