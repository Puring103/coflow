use super::builtins::Builtin;
use super::value::{
    comparable_key, dict_key_from_check_value, format_check_key_for_path, values_equal, CheckValue,
    LocatedCheckValue,
};
use crate::schema_view::SchemaView;
use coflow_cft::{
    CftSchemaBinOp, CftSchemaCheckBlock, CftSchemaCheckExpr, CftSchemaCheckExprKind,
    CftSchemaCheckStmt, CftSchemaCmpOp, CftSchemaQuantifierKind, CftSchemaTypePredicate,
    CftSchemaTypeRef, CftSchemaUnaryOp,
};
use coflow_data_model::{
    CfdDataModel, CfdDiagnostic, CfdEnumValue, CfdErrorCode, CfdPath, CfdRecordId,
};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct CheckEvaluator<'a> {
    schema: &'a SchemaView,
    model: &'a CfdDataModel,
    root_record: Option<CfdRecordId>,
    root_path: CfdPath,
    current: CheckValue,
    scopes: Vec<BTreeMap<String, LocatedCheckValue>>,
    pub(super) diagnostics: Vec<CfdDiagnostic>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EvalFlow {
    Continue,
    HardStop,
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

    pub(super) fn eval_check_block(&mut self, check: &CftSchemaCheckBlock) -> EvalFlow {
        self.eval_stmts(&check.stmts)
    }

    fn eval_stmts(&mut self, stmts: &[CftSchemaCheckStmt]) -> EvalFlow {
        for stmt in stmts {
            if self.eval_stmt(stmt) == EvalFlow::HardStop {
                return EvalFlow::HardStop;
            }
        }
        EvalFlow::Continue
    }

    fn eval_stmt(&mut self, stmt: &CftSchemaCheckStmt) -> EvalFlow {
        match stmt {
            CftSchemaCheckStmt::Expr(expr) => match self.eval_expr(expr) {
                Ok(value) if matches!(value.value, CheckValue::Bool(true)) => EvalFlow::Continue,
                Ok(value) if matches!(value.value, CheckValue::Bool(false)) => {
                    self.diag_at(
                        CfdErrorCode::CheckFailed,
                        value.path,
                        "check condition evaluated to false",
                    );
                    EvalFlow::Continue
                }
                Ok(value) => {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        value.path,
                        "check expression did not evaluate to bool",
                    );
                    EvalFlow::HardStop
                }
                Err(()) => EvalFlow::HardStop,
            },
            CftSchemaCheckStmt::When {
                condition, body, ..
            } => match self.eval_expr(condition) {
                Ok(value) if matches!(value.value, CheckValue::Bool(true)) => self.eval_stmts(body),
                Ok(value) if matches!(value.value, CheckValue::Bool(false)) => EvalFlow::Continue,
                Ok(value) => {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        value.path,
                        "when condition did not evaluate to bool",
                    );
                    EvalFlow::HardStop
                }
                Err(()) => EvalFlow::HardStop,
            },
            CftSchemaCheckStmt::Quantifier {
                kind,
                binding,
                collection,
                body,
                ..
            } => {
                let Ok(collection) = self.eval_expr(collection) else {
                    return EvalFlow::HardStop;
                };
                let Some(items) = self.quantifier_items(collection) else {
                    return EvalFlow::HardStop;
                };
                self.eval_quantifier(*kind, binding, &items, body)
            }
        }
    }

    fn eval_quantifier(
        &mut self,
        kind: CftSchemaQuantifierKind,
        binding: &str,
        items: &[LocatedCheckValue],
        body: &[CftSchemaCheckStmt],
    ) -> EvalFlow {
        let mut matched = 0_usize;
        let mut any_failures = Vec::new();
        let mut none_match_paths = Vec::new();
        for item in items {
            let diagnostic_start = self.diagnostics.len();
            let mut scope = BTreeMap::new();
            scope.insert(binding.to_string(), item.clone());
            self.scopes.push(scope);
            let flow = self.eval_stmts(body);
            let passed = flow == EvalFlow::Continue && self.diagnostics.len() == diagnostic_start;
            let _ = self.scopes.pop();

            if flow == EvalFlow::HardStop {
                return EvalFlow::HardStop;
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
            CftSchemaQuantifierKind::All => {}
            CftSchemaQuantifierKind::Any if matched == 0 => {
                if any_failures.is_empty() {
                    self.diag(
                        CfdErrorCode::CheckFailed,
                        "any quantifier did not match any element",
                    );
                } else {
                    self.diagnostics.extend(any_failures);
                }
            }
            CftSchemaQuantifierKind::Any => {}
            CftSchemaQuantifierKind::None if matched > 0 => {
                for path in none_match_paths {
                    self.diag_at(
                        CfdErrorCode::CheckFailed,
                        path,
                        "none quantifier matched this element",
                    );
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
            _ => {
                self.diag(
                    CfdErrorCode::CheckEvalTypeError,
                    "quantifier target is not a collection",
                );
                None
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    fn eval_expr(&mut self, expr: &CftSchemaCheckExpr) -> Result<LocatedCheckValue, ()> {
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
                self.eval_index(target, index)
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

    fn eval_name(&mut self, name: &str) -> Result<LocatedCheckValue, ()> {
        for scope in self.scopes.iter().rev() {
            if let Some(value) = scope.get(name) {
                return Ok(value.clone());
            }
        }
        if let Some(value) = self.current_field(name) {
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
            format!("unknown check value `{name}`"),
        );
        Err(())
    }

    fn current_field(&self, name: &str) -> Option<LocatedCheckValue> {
        let CheckValue::Record(record) = &self.current else {
            return None;
        };
        if name == "id" {
            return self.virtual_id(record, record.path());
        }
        let field_type = self.field_type_for_record(record, name);
        record.field(self.model, field_type, name)
    }

    fn field_type_for_record(
        &self,
        record: &super::value::CheckRecordRef,
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
    ) -> Result<LocatedCheckValue, ()> {
        if matches!(target.value, CheckValue::Null) {
            self.diag_at(
                CfdErrorCode::CheckNullAccess,
                target.path,
                "field access on null value",
            );
            return Err(());
        }
        match target.value {
            CheckValue::Record(record) => {
                if name == "id" {
                    return self.virtual_id(&record, target.path).ok_or_else(|| {
                        self.diag_at(
                            CfdErrorCode::CheckEvalTypeError,
                            None,
                            "record has no virtual id",
                        );
                    });
                }
                let field_type = self.field_type_for_record(&record, name);
                record.field(self.model, field_type, name).ok_or_else(|| {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        target.path,
                        format!("record has no field `{name}`"),
                    );
                })
            }
            CheckValue::Entry(entry) => match name {
                "key" => Ok(LocatedCheckValue::new(*entry.key, target.path)),
                "value" => Ok(LocatedCheckValue::new(entry.value, target.path)),
                _ => {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        target.path,
                        format!("dict entry has no field `{name}`"),
                    );
                    Err(())
                }
            },
            _ => {
                self.diag_at(
                    CfdErrorCode::CheckEvalTypeError,
                    target.path,
                    "field access target is not an object",
                );
                Err(())
            }
        }
    }

    fn virtual_id(
        &self,
        record: &super::value::CheckRecordRef,
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
    ) -> Result<LocatedCheckValue, ()> {
        if matches!(target.value, CheckValue::Null) {
            self.diag_at(
                CfdErrorCode::CheckNullAccess,
                target.path,
                "index access on null value",
            );
            return Err(());
        }
        match target.value {
            CheckValue::Array { items, .. } => {
                let CheckValue::Int(index) = index.value else {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        index.path,
                        "array index is not int",
                    );
                    return Err(());
                };
                let Ok(index) = usize::try_from(index) else {
                    self.diag_at(
                        CfdErrorCode::CheckIndexOutOfBounds,
                        target.path,
                        "array index is negative",
                    );
                    return Err(());
                };
                items
                    .get(index)
                    .cloned()
                    .map(|value| {
                        LocatedCheckValue::new(
                            value,
                            target.path.clone().map(|path| path.index(index)),
                        )
                    })
                    .ok_or_else(|| {
                        self.diag_at(
                            CfdErrorCode::CheckIndexOutOfBounds,
                            target.path,
                            "array index is out of bounds",
                        );
                    })
            }
            CheckValue::Dict { entries, .. } => {
                let Some(key) = dict_key_from_check_value(&index.value) else {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        index.path,
                        "dict index is not a valid key",
                    );
                    return Err(());
                };
                entries
                    .into_iter()
                    .find(|entry| entry.key_key().is_some_and(|entry_key| entry_key == key))
                    .map(|entry| LocatedCheckValue::new(entry.value, target.path.clone()))
                    .ok_or_else(|| {
                        self.diag_at(
                            CfdErrorCode::CheckMissingDictKey,
                            target.path,
                            "dict key is missing",
                        );
                    })
            }
            _ => {
                self.diag_at(
                    CfdErrorCode::CheckEvalTypeError,
                    target.path,
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
    ) -> Result<LocatedCheckValue, ()> {
        if self.schema.enums.contains_key(name) {
            let arg = self.exactly_one_arg(args, "enum constructor expects one argument")?;
            let arg_value = self.eval_expr(arg)?;
            let CheckValue::Int(value) = arg_value.value else {
                self.diag_at(
                    CfdErrorCode::CheckEvalTypeError,
                    arg_value.path,
                    "enum constructor arg is not int",
                );
                return Err(());
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
                format!("unknown function `{name}`"),
            );
            return Err(());
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
                    _ => {
                        self.diag_at(
                            CfdErrorCode::CheckEvalTypeError,
                            arg_value.path,
                            "len expects array or dict",
                        );
                        Err(())
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
                let CheckValue::Array { items, .. } = arg_value.value else {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        arg_value.path,
                        "unique expects array",
                    );
                    return Err(());
                };
                let mut seen = BTreeSet::new();
                for item in items {
                    let Some(key) = comparable_key(&item) else {
                        self.diag_at(
                            CfdErrorCode::CheckEvalTypeError,
                            arg_value.path.clone(),
                            "unique element is not comparable",
                        );
                        return Err(());
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
                let CheckValue::Dict {
                    entries, key_type, ..
                } = arg_value.value
                else {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        arg_value.path,
                        "keys expects dict",
                    );
                    return Err(());
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
                let CheckValue::Dict {
                    entries,
                    value_type,
                    ..
                } = arg_value.value
                else {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        arg_value.path,
                        "values expects dict",
                    );
                    return Err(());
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
                let CheckValue::String(text) = value.value else {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        value.path,
                        "matches value is not string",
                    );
                    return Err(());
                };
                let CftSchemaCheckExprKind::String(pattern) = &args[1].kind else {
                    self.diag(
                        CfdErrorCode::CheckEvalTypeError,
                        "matches pattern is not literal",
                    );
                    return Err(());
                };
                let regex = Regex::new(pattern).map_err(|_| {
                    self.diag(
                        CfdErrorCode::CheckEvalTypeError,
                        "regex pattern cannot be compiled",
                    );
                })?;
                Ok(LocatedCheckValue::new(
                    CheckValue::Bool(regex.is_match(&text)),
                    value.path,
                ))
            }
        }
    }

    fn require_builtin_arity(
        &mut self,
        builtin: Builtin,
        args: &[CftSchemaCheckExpr],
    ) -> Result<(), ()> {
        if args.len() == builtin.arity() {
            return Ok(());
        }
        self.diag(
            CfdErrorCode::CheckEvalTypeError,
            format!(
                "{} expects {} argument{}",
                builtin.name(),
                builtin.arity(),
                if builtin.arity() == 1 { "" } else { "s" }
            ),
        );
        Err(())
    }

    fn exactly_one_arg<'b>(
        &mut self,
        args: &'b [CftSchemaCheckExpr],
        message: &str,
    ) -> Result<&'b CftSchemaCheckExpr, ()> {
        let [arg] = args else {
            self.diag(CfdErrorCode::CheckEvalTypeError, message);
            return Err(());
        };
        Ok(arg)
    }

    fn eval_min_max(
        &mut self,
        builtin: Builtin,
        args: &[CftSchemaCheckExpr],
    ) -> Result<LocatedCheckValue, ()> {
        let arg_value = self.eval_expr(&args[0])?;
        let CheckValue::Array { items, .. } = arg_value.value else {
            self.diag_at(
                CfdErrorCode::CheckEvalTypeError,
                arg_value.path,
                "min/max expects array",
            );
            return Err(());
        };
        if items.is_empty() {
            self.diag_at(
                CfdErrorCode::CheckEmptyMinMax,
                arg_value.path,
                "min/max called on empty array",
            );
            return Err(());
        }
        let mut non_null_items = items
            .iter()
            .filter(|item| !matches!(item, CheckValue::Null));
        let Some(mut out) = non_null_items.next().cloned() else {
            self.diag_at(
                CfdErrorCode::CheckEmptyMinMax,
                arg_value.path,
                "min/max called with no non-null values",
            );
            return Err(());
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

    fn eval_sum(&mut self, args: &[CftSchemaCheckExpr]) -> Result<LocatedCheckValue, ()> {
        let arg_value = self.eval_expr(&args[0])?;
        let CheckValue::Array {
            items,
            element_type,
        } = arg_value.value
        else {
            self.diag_at(
                CfdErrorCode::CheckEvalTypeError,
                arg_value.path,
                "sum expects array",
            );
            return Err(());
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
                            "integer sum overflowed",
                        );
                        return Err(());
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
                _ => {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        arg_value.path.clone(),
                        "sum item is not numeric",
                    );
                    return Err(());
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
    ) -> Result<bool, ()> {
        match &collection.value {
            CheckValue::Array { items, .. } => {
                Ok(items.iter().any(|item| values_equal(item, value)))
            }
            CheckValue::Dict { entries, .. } => {
                let Some(key) = dict_key_from_check_value(value) else {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        collection.path.clone(),
                        "contains dict key is not a valid key",
                    );
                    return Err(());
                };
                Ok(entries
                    .iter()
                    .any(|entry| entry.key_key() == Some(key.clone())))
            }
            _ => {
                self.diag_at(
                    CfdErrorCode::CheckEvalTypeError,
                    collection.path.clone(),
                    "contains expects array or dict",
                );
                Err(())
            }
        }
    }

    fn eval_unary(
        &mut self,
        op: CftSchemaUnaryOp,
        value: LocatedCheckValue,
    ) -> Result<LocatedCheckValue, ()> {
        let path = value.path;
        match (op, value.value) {
            (CftSchemaUnaryOp::Not, CheckValue::Bool(value)) => {
                Ok(LocatedCheckValue::new(CheckValue::Bool(!value), path))
            }
            (CftSchemaUnaryOp::Neg, CheckValue::Int(value)) => {
                self.checked_int(value.checked_neg(), path, "integer negation overflowed")
            }
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
            _ => {
                self.diag_at(
                    CfdErrorCode::CheckEvalTypeError,
                    path,
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
    ) -> Result<LocatedCheckValue, ()> {
        match op {
            CftSchemaBinOp::Or => {
                let lhs = self.eval_expr(lhs)?;
                let lhs_path = lhs.path.clone();
                let CheckValue::Bool(lhs) = lhs.value else {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        lhs_path,
                        "lhs is not bool",
                    );
                    return Err(());
                };
                if lhs {
                    return Ok(LocatedCheckValue::new(CheckValue::Bool(true), lhs_path));
                }
                let rhs = self.eval_expr(rhs)?;
                let rhs_path = rhs.path.clone();
                let CheckValue::Bool(rhs) = rhs.value else {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        rhs_path,
                        "rhs is not bool",
                    );
                    return Err(());
                };
                Ok(LocatedCheckValue::new(CheckValue::Bool(rhs), rhs_path))
            }
            CftSchemaBinOp::And => {
                let lhs = self.eval_expr(lhs)?;
                let lhs_path = lhs.path.clone();
                let CheckValue::Bool(lhs) = lhs.value else {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        lhs_path,
                        "lhs is not bool",
                    );
                    return Err(());
                };
                if !lhs {
                    return Ok(LocatedCheckValue::new(CheckValue::Bool(false), lhs_path));
                }
                let rhs = self.eval_expr(rhs)?;
                let rhs_path = rhs.path.clone();
                let CheckValue::Bool(rhs) = rhs.value else {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        rhs_path,
                        "rhs is not bool",
                    );
                    return Err(());
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
    ) -> Result<LocatedCheckValue, ()> {
        if matches!(lhs, CheckValue::Null) || matches!(rhs, CheckValue::Null) {
            self.diag_at(
                CfdErrorCode::CheckNullAccess,
                path,
                "binary operation on null value",
            );
            return Err(());
        }
        match (op, lhs, rhs) {
            (CftSchemaBinOp::Add, CheckValue::Int(lhs), CheckValue::Int(rhs)) => {
                self.checked_int(lhs.checked_add(rhs), path, "integer addition overflow")
            }
            (CftSchemaBinOp::Sub, CheckValue::Int(lhs), CheckValue::Int(rhs)) => {
                self.checked_int(lhs.checked_sub(rhs), path, "integer subtraction overflow")
            }
            (CftSchemaBinOp::Mul, CheckValue::Int(lhs), CheckValue::Int(rhs)) => self.checked_int(
                lhs.checked_mul(rhs),
                path,
                "integer multiplication overflow",
            ),
            (CftSchemaBinOp::Div, CheckValue::Int(lhs), CheckValue::Int(rhs)) => {
                self.checked_int(lhs.checked_div(rhs), path, "integer division failed")
            }
            (CftSchemaBinOp::IntDiv, CheckValue::Int(lhs), CheckValue::Int(rhs)) => {
                self.checked_int(lhs.checked_div(rhs), path, "integer division failed")
            }
            (CftSchemaBinOp::Mod, CheckValue::Int(lhs), CheckValue::Int(rhs)) => {
                self.checked_int(lhs.checked_rem(rhs), path, "integer modulo failed")
            }
            (CftSchemaBinOp::Pow, CheckValue::Int(lhs), CheckValue::Int(rhs)) => {
                match rhs.try_into().ok().and_then(|rhs| lhs.checked_pow(rhs)) {
                    Some(value) => Ok(LocatedCheckValue::new(CheckValue::Int(value), path)),
                    None => {
                        self.diag_at(
                            CfdErrorCode::CheckEvalTypeError,
                            path,
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
                    path,
                    "integer shift left failed",
                ),
            (CftSchemaBinOp::Shr, CheckValue::Int(lhs), CheckValue::Int(rhs)) => self
                .checked_shift(
                    i64::checked_shr,
                    lhs,
                    rhs,
                    path,
                    "integer shift right failed",
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
            _ => {
                self.diag_at(
                    CfdErrorCode::CheckEvalTypeError,
                    path,
                    "unsupported binary operation",
                );
                Err(())
            }
        }
    }

    fn checked_int(
        &mut self,
        value: Option<i64>,
        path: Option<CfdPath>,
        message: impl Into<String>,
    ) -> Result<LocatedCheckValue, ()> {
        value
            .map(|value| LocatedCheckValue::new(CheckValue::Int(value), path.clone()))
            .ok_or_else(|| {
                self.diag_at(CfdErrorCode::CheckEvalTypeError, path, message);
            })
    }

    fn checked_shift(
        &mut self,
        op: fn(i64, u32) -> Option<i64>,
        lhs: i64,
        rhs: i64,
        path: Option<CfdPath>,
        message: impl Into<String>,
    ) -> Result<LocatedCheckValue, ()> {
        let Some(rhs) = rhs.try_into().ok() else {
            self.diag_at(CfdErrorCode::CheckEvalTypeError, path, message);
            return Err(());
        };
        self.checked_int(op(lhs, rhs), path, message)
    }

    fn compare(
        &mut self,
        op: CftSchemaCmpOp,
        lhs: &CheckValue,
        rhs: &CheckValue,
        path: Option<CfdPath>,
    ) -> Result<bool, ()> {
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
    ) -> Result<std::cmp::Ordering, ()> {
        if matches!(lhs, CheckValue::Null) || matches!(rhs, CheckValue::Null) {
            self.diag_at(
                CfdErrorCode::CheckNullAccess,
                path,
                "ordered comparison on null value",
            );
            return Err(());
        }
        match (lhs, rhs) {
            (CheckValue::Int(lhs), CheckValue::Int(rhs)) => Ok(lhs.cmp(rhs)),
            (CheckValue::Float(lhs), CheckValue::Float(rhs)) => {
                lhs.partial_cmp(rhs).ok_or_else(|| {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        path,
                        "float comparison failed",
                    );
                })
            }
            (CheckValue::Enum(lhs), CheckValue::Enum(rhs)) if lhs.enum_name == rhs.enum_name => {
                Ok(lhs.value.cmp(&rhs.value))
            }
            _ => {
                self.diag_at(
                    CfdErrorCode::CheckEvalTypeError,
                    path,
                    "values are not ordered comparable",
                );
                Err(())
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
        self.diagnostics
            .push(CfdDiagnostic::error(code, message).with_primary(self.root_record, path));
    }
}

fn type_ref_is_float(ty: Option<&CftSchemaTypeRef>) -> bool {
    match ty {
        Some(CftSchemaTypeRef::Float) => true,
        Some(CftSchemaTypeRef::Nullable(inner)) => type_ref_is_float(Some(inner)),
        _ => false,
    }
}
