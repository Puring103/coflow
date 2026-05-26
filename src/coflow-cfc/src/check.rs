mod diagnostics;
mod runtime;
mod scope;

use self::diagnostics::{all_failed, cond_failed, describe_expr, eval_error};
use self::runtime::{
    compare_values, eval_value_equals_ref, is_string_value, numeric_bin, shift_amount, EvalValue,
    NumberKind,
};
use self::scope::{enum_type_value, module_namespace_value, ref_layer, CheckScope};
use crate::ast::{
    BinOp, CheckBlock, CheckExpr, CheckExprKind, CmpOp, CondStmt, Item, QuantifierKind, TypeName,
    TypePredicate, TypeRef, UnaryOp,
};
use crate::container::{CfcContainer, CfcResult, ModuleId};
use crate::error::{AllFailedItem, CheckError};
use crate::span::Span;
use crate::value::{CfcNominalType, CfcValue, CfcValueRef};
use std::collections::{BTreeMap, HashMap, HashSet};

pub(crate) fn run(container: &CfcContainer, result: &CfcResult) -> Vec<CheckError> {
    CheckRunner::new(container, result).run()
}

struct CheckRunner<'a> {
    container: &'a CfcContainer,
    result: &'a CfcResult,
    errors: Vec<CheckError>,
    checked_objects: HashSet<usize>,
    visited_walk: HashSet<usize>,
}

impl<'a> CheckRunner<'a> {
    fn new(container: &'a CfcContainer, result: &'a CfcResult) -> Self {
        Self {
            container,
            result,
            errors: Vec::new(),
            checked_objects: HashSet::new(),
            visited_walk: HashSet::new(),
        }
    }

    fn run(mut self) -> Vec<CheckError> {
        let enum_values = self.enum_values();
        self.run_type_checks(&enum_values);
        self.run_top_level_checks(&enum_values);
        self.errors
    }

    fn enum_values(&self) -> HashMap<(ModuleId, String, String), CfcValueRef> {
        let mut out = HashMap::new();
        for (module_id, module) in &self.container.modules {
            for item in &module.ast.items {
                let Item::Enum(def) = item else {
                    continue;
                };
                let mut next = 0;
                for variant in &def.variants {
                    let value = variant.value.unwrap_or(next);
                    next = value + 1;
                    out.insert(
                        (module_id.clone(), def.name.clone(), variant.name.clone()),
                        CfcValueRef::new(CfcValue::Enum {
                            enum_type: CfcNominalType {
                                module: module_id.clone(),
                                name: def.name.clone(),
                            },
                            variant: variant.name.clone(),
                            value,
                        }),
                    );
                }
            }
        }
        out
    }

    fn run_type_checks(&mut self, enum_values: &HashMap<(ModuleId, String, String), CfcValueRef>) {
        for (_, module) in self.result.modules() {
            for (_, value) in module.values() {
                self.walk_value(&value, enum_values);
            }
        }
    }

    fn walk_value(
        &mut self,
        value: &CfcValueRef,
        enum_values: &HashMap<(ModuleId, String, String), CfcValueRef>,
    ) {
        if !self.visited_walk.insert(value.ptr_key()) {
            return;
        }
        let children = {
            let borrowed = value.borrow();
            match &*borrowed {
                CfcValue::Object { type_name, fields } => {
                    if let Some(type_name) = type_name {
                        self.check_type_instance(type_name, fields, value, enum_values);
                    }
                    fields.values().cloned().collect::<Vec<_>>()
                }
                CfcValue::Union { value, .. } => vec![value.clone()],
                CfcValue::Array(items) => items.clone(),
                CfcValue::Dict(entries) => entries
                    .iter()
                    .flat_map(|(key, value)| [key.clone(), value.clone()])
                    .collect(),
                _ => Vec::new(),
            }
        };
        for child in children {
            self.walk_value(&child, enum_values);
        }
    }

    fn check_type_instance(
        &mut self,
        type_name: &CfcNominalType,
        fields: &BTreeMap<String, CfcValueRef>,
        value: &CfcValueRef,
        enum_values: &HashMap<(ModuleId, String, String), CfcValueRef>,
    ) {
        if !self.checked_objects.insert(value.ptr_key()) {
            return;
        }
        let Some(block) = self.type_check_block(type_name).cloned() else {
            return;
        };
        let mut scope = CheckScope::new(enum_values, self.base_scope(&type_name.module, false));
        scope.push(ref_layer(
            fields
                .iter()
                .map(|(name, value)| (name.clone(), value.clone())),
        ));
        self.eval_block(
            &block,
            &type_name.module,
            &mut scope,
            type_name.name.as_str(),
        );
    }

    fn type_check_block(&self, type_name: &CfcNominalType) -> Option<&CheckBlock> {
        let module = self.container.modules.get(&type_name.module)?;
        module.ast.items.iter().find_map(|item| {
            let Item::Type(def) = item else {
                return None;
            };
            (def.name == type_name.name)
                .then_some(def.check.as_ref())
                .flatten()
        })
    }

    fn run_top_level_checks(
        &mut self,
        enum_values: &HashMap<(ModuleId, String, String), CfcValueRef>,
    ) {
        for (module_id, module_result) in self.result.modules() {
            let Some(module) = self.container.modules.get(module_id) else {
                continue;
            };
            let locals = module_result
                .values()
                .map(|(name, value)| (name.to_string(), EvalValue::Ref(value)))
                .collect::<BTreeMap<_, _>>();
            for item in &module.ast.items {
                let Item::Check(block) = item else {
                    continue;
                };
                let mut scope = CheckScope::new(enum_values, self.base_scope(module_id, true));
                scope.push(locals.clone());
                self.eval_block(block, module_id, &mut scope, module_id.as_str());
            }
        }
    }

    fn base_scope(
        &self,
        module: &ModuleId,
        allow_imported_data: bool,
    ) -> BTreeMap<String, EvalValue> {
        let mut out = BTreeMap::new();
        let Some(module_data) = self.container.modules.get(module) else {
            return out;
        };
        for item in &module_data.ast.items {
            if let Item::Enum(def) = item {
                out.insert(def.name.clone(), enum_type_value(module, &def.name));
            }
        }
        for import in &module_data.imports {
            if let Some(dep) = module_data.bindings.get(&import.id) {
                out.insert(
                    import.alias.clone(),
                    module_namespace_value(dep, allow_imported_data),
                );
            }
        }
        out
    }

    fn eval_block(
        &mut self,
        block: &CheckBlock,
        module: &ModuleId,
        scope: &mut CheckScope<'_>,
        context: &str,
    ) {
        for stmt in &block.stmts {
            if !self.eval_stmt(stmt, module, scope, context) {
                break;
            }
        }
    }

    fn eval_stmt(
        &mut self,
        stmt: &CondStmt,
        module: &ModuleId,
        scope: &mut CheckScope<'_>,
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
            } => self.eval_quantifier(
                *kind, binding, collection, body, *span, module, scope, context,
            ),
        }
    }

    fn eval_condition(
        &mut self,
        expr: &CheckExpr,
        module: &ModuleId,
        scope: &mut CheckScope<'_>,
        context: &str,
    ) -> bool {
        match self.eval_expr(expr, module, scope, context) {
            Ok(value) => match value {
                EvalValue::Bool(true) => true,
                EvalValue::Bool(false) => {
                    self.errors
                        .push(cond_failed(describe_expr(expr), module, context, expr.span));
                    true
                }
                other => {
                    self.errors.push(eval_error(
                        format!("condition must be bool, found {}", other.type_name()),
                        module,
                        context,
                        expr.span,
                    ));
                    false
                }
            },
            Err(message) => {
                self.errors
                    .push(eval_error(message, module, context, expr.span));
                false
            }
        }
    }

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    fn eval_quantifier(
        &mut self,
        kind: QuantifierKind,
        binding: &str,
        collection: &CheckExpr,
        body: &[CondStmt],
        span: Span,
        module: &ModuleId,
        scope: &mut CheckScope<'_>,
        context: &str,
    ) -> bool {
        let Ok(value) = self.eval_expr(collection, module, scope, context) else {
            self.errors.push(eval_error(
                format!(
                    "failed to evaluate {} collection `{}`",
                    quantifier_name(kind),
                    describe_expr(collection)
                ),
                module,
                context,
                collection.span,
            ));
            return false;
        };
        let entries = match self.collection_entries(value, collection, module, context, kind) {
            Some(entries) => entries,
            None => return false,
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
            let mut stopped = false;
            for stmt in body {
                if !self.eval_stmt(stmt, module, scope, &item_context) {
                    stopped = true;
                    break;
                }
            }
            scope.pop();
            let errors = self.errors.split_off(before);
            if errors.is_empty() {
                passed.push(format!("{binding}{label}"));
                if kind == QuantifierKind::Any {
                    return true;
                }
            } else {
                failed.push(AllFailedItem {
                    key: format!("{binding}{label}"),
                    errors,
                });
                if stopped {
                    self.errors.push(all_failed(
                        format!(
                            "{} {binding} in {}",
                            quantifier_name(kind),
                            describe_expr(collection)
                        ),
                        module,
                        context,
                        total,
                        failed,
                        span,
                    ));
                    return false;
                }
            }
        }

        match kind {
            QuantifierKind::All => {
                if !failed.is_empty() {
                    self.errors.push(all_failed(
                        format!("all {binding} in {}", describe_expr(collection)),
                        module,
                        context,
                        total,
                        failed,
                        span,
                    ));
                }
                true
            }
            QuantifierKind::Any => {
                self.errors.push(cond_failed(
                    format!("any {binding} in {}", describe_expr(collection)),
                    module,
                    context,
                    span,
                ));
                true
            }
            QuantifierKind::None => {
                if !passed.is_empty() {
                    let failed = passed
                        .into_iter()
                        .map(|key| AllFailedItem {
                            key,
                            errors: Vec::new(),
                        })
                        .collect();
                    self.errors.push(all_failed(
                        format!("none {binding} in {}", describe_expr(collection)),
                        module,
                        context,
                        total,
                        failed,
                        span,
                    ));
                }
                true
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
    ) -> Option<Vec<(String, CfcValueRef)>> {
        match value {
            EvalValue::Ref(value) => match &*value.borrow() {
                CfcValue::Array(items) => Some(
                    items
                        .iter()
                        .enumerate()
                        .map(|(index, value)| (format!("[{index}]"), value.clone()))
                        .collect::<Vec<_>>(),
                ),
                CfcValue::Dict(items) => Some(
                    items
                        .iter()
                        .enumerate()
                        .map(|(index, (key, value))| {
                            (
                                format!("[{index}]"),
                                CfcValueRef::new(CfcValue::Object {
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

    fn eval_expr(
        &self,
        expr: &CheckExpr,
        module: &ModuleId,
        scope: &CheckScope<'_>,
        context: &str,
    ) -> Result<EvalValue, String> {
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
                self.eval_field(expr, name, module, scope, context)
            }
            CheckExprKind::Index { expr, index } => {
                self.eval_index(expr, index, module, scope, context)
            }
            CheckExprKind::Is { expr, predicate } => {
                self.eval_is(expr, predicate, module, scope, context)
            }
            CheckExprKind::Call { name, args } => {
                self.eval_call(name, args, module, scope, context)
            }
            CheckExprKind::BinOp { op, lhs, rhs } => {
                self.eval_bin(*op, lhs, rhs, module, scope, context)
            }
            CheckExprKind::Unary { op, expr } => self.eval_unary(*op, expr, module, scope, context),
            CheckExprKind::CmpChain { first, rest } => {
                self.eval_cmp_chain(first, rest, module, scope, context)
            }
        }
    }

    fn eval_is(
        &self,
        expr: &CheckExpr,
        predicate: &TypePredicate,
        module: &ModuleId,
        scope: &CheckScope<'_>,
        context: &str,
    ) -> Result<EvalValue, String> {
        let value = self.eval_expr(expr, module, scope, context)?;
        if matches!(predicate, TypePredicate::Null) {
            let is_null = match value {
                EvalValue::Null => true,
                EvalValue::Ref(value) => matches!(&*value.borrow(), CfcValue::Null),
                _ => false,
            };
            return Ok(EvalValue::Bool(is_null));
        }
        let Ok(value) = value.into_ref() else {
            return Ok(EvalValue::Bool(false));
        };
        let value = unwrap_union_ref(value);
        let borrowed = value.borrow();
        let CfcValue::Object {
            type_name: Some(actual),
            ..
        } = &*borrowed
        else {
            return Ok(EvalValue::Bool(false));
        };
        let TypePredicate::Type(ty) = predicate else {
            unreachable!("null predicate handled before nominal type check");
        };
        let Some((target_module, target_name)) = self.resolve_check_type_name(module, ty) else {
            return Ok(EvalValue::Bool(false));
        };
        if actual.module == target_module && actual.name == target_name {
            return Ok(EvalValue::Bool(true));
        }
        Ok(EvalValue::Bool(self.union_contains_actual(
            &target_module,
            &target_name,
            actual,
        )))
    }

    fn resolve_check_type_name(
        &self,
        module: &ModuleId,
        ty: &TypeName,
    ) -> Option<(ModuleId, String)> {
        match ty {
            TypeName::Local(name) => Some((module.clone(), name.clone())),
            TypeName::Imported { alias, name } => {
                let module_data = self.container.modules.get(module)?;
                let import = module_data
                    .imports
                    .iter()
                    .find(|import| import.alias == *alias)?;
                let dep = module_data.bindings.get(&import.id)?;
                Some((dep.clone(), name.clone()))
            }
        }
    }

    fn union_contains_actual(
        &self,
        union_module: &ModuleId,
        union_name: &str,
        actual: &CfcNominalType,
    ) -> bool {
        let Some(module) = self.container.modules.get(union_module) else {
            return false;
        };
        let Some(alias) = module.ast.items.iter().find_map(|item| {
            let Item::Type(def) = item else {
                return None;
            };
            (def.name == union_name)
                .then_some(def.alias.as_ref())
                .flatten()
        }) else {
            return false;
        };
        let TypeRef::Union(branches) = alias else {
            return false;
        };
        branches.iter().any(|branch| {
            let TypeRef::Named(name) = branch else {
                return false;
            };
            self.resolve_check_type_name(union_module, name)
                .is_some_and(|(branch_module, branch_name)| {
                    actual.module == branch_module && actual.name == branch_name
                })
        })
    }

    fn eval_call(
        &self,
        name: &str,
        args: &[CheckExpr],
        module: &ModuleId,
        scope: &CheckScope<'_>,
        context: &str,
    ) -> Result<EvalValue, String> {
        match name {
            "len" => {
                self.expect_arity(name, args, 1)?;
                self.builtin_len(&args[0], module, scope, context)
            }
            "contains" => {
                self.expect_arity(name, args, 2)?;
                self.builtin_contains(&args[0], &args[1], module, scope, context)
            }
            "unique" => {
                self.expect_arity(name, args, 1)?;
                self.builtin_unique(&args[0], module, scope, context)
            }
            "min" => {
                self.expect_arity(name, args, 1)?;
                self.builtin_min_max("min", &args[0], module, scope, context)
            }
            "max" => {
                self.expect_arity(name, args, 1)?;
                self.builtin_min_max("max", &args[0], module, scope, context)
            }
            "sum" => {
                self.expect_arity(name, args, 1)?;
                self.builtin_sum(&args[0], module, scope, context)
            }
            "keys" => {
                self.expect_arity(name, args, 1)?;
                self.builtin_keys_values("keys", &args[0], module, scope, context)
            }
            "values" => {
                self.expect_arity(name, args, 1)?;
                self.builtin_keys_values("values", &args[0], module, scope, context)
            }
            _ => Err(format!("unknown builtin function `{name}`")),
        }
    }

    fn expect_arity(&self, name: &str, args: &[CheckExpr], expected: usize) -> Result<(), String> {
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

    fn builtin_len(
        &self,
        arg: &CheckExpr,
        module: &ModuleId,
        scope: &CheckScope<'_>,
        context: &str,
    ) -> Result<EvalValue, String> {
        let value = self.eval_expr(arg, module, scope, context)?.into_ref()?;
        let len = match &*value.borrow() {
            CfcValue::Array(items) => items.len(),
            CfcValue::Dict(entries) => entries.len(),
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

    fn builtin_contains(
        &self,
        collection: &CheckExpr,
        needle: &CheckExpr,
        module: &ModuleId,
        scope: &CheckScope<'_>,
        context: &str,
    ) -> Result<EvalValue, String> {
        let collection = self
            .eval_expr(collection, module, scope, context)?
            .into_ref()?;
        let needle = self.eval_expr(needle, module, scope, context)?;
        let contains = match &*collection.borrow() {
            CfcValue::Array(items) => items
                .iter()
                .any(|item| eval_value_equals_ref(&needle, item)),
            CfcValue::Dict(entries) => entries
                .iter()
                .any(|(key, _)| eval_value_equals_ref(&needle, key)),
            other => {
                return Err(format!(
                    "contains() expects array or dict, found {}",
                    other.type_name()
                ));
            }
        };
        Ok(EvalValue::Bool(contains))
    }

    fn builtin_unique(
        &self,
        arg: &CheckExpr,
        module: &ModuleId,
        scope: &CheckScope<'_>,
        context: &str,
    ) -> Result<EvalValue, String> {
        let value = self.eval_expr(arg, module, scope, context)?.into_ref()?;
        let CfcValue::Array(items) = &*value.borrow() else {
            return Err("unique() expects array".to_string());
        };
        let mut seen = HashSet::new();
        for item in items {
            let key = unique_key(item)?;
            if !seen.insert(key) {
                return Ok(EvalValue::Bool(false));
            }
        }
        Ok(EvalValue::Bool(true))
    }

    fn builtin_min_max(
        &self,
        name: &str,
        arg: &CheckExpr,
        module: &ModuleId,
        scope: &CheckScope<'_>,
        context: &str,
    ) -> Result<EvalValue, String> {
        let value = self.eval_expr(arg, module, scope, context)?.into_ref()?;
        let CfcValue::Array(items) = &*value.borrow() else {
            return Err(format!("{name}() expects array"));
        };
        let Some(mut best) = items.first().cloned() else {
            return Err(format!("{name}() requires a non-empty array"));
        };
        ensure_orderable_for_builtin(name, &best)?;
        for item in &items[1..] {
            ensure_orderable_for_builtin(name, item)?;
            let op = if name == "min" { CmpOp::Lt } else { CmpOp::Gt };
            if compare_values(
                op,
                &EvalValue::Ref(item.clone()),
                &EvalValue::Ref(best.clone()),
            )? {
                best = item.clone();
            }
        }
        Ok(EvalValue::Ref(best))
    }

    fn builtin_sum(
        &self,
        arg: &CheckExpr,
        module: &ModuleId,
        scope: &CheckScope<'_>,
        context: &str,
    ) -> Result<EvalValue, String> {
        let value = self.eval_expr(arg, module, scope, context)?.into_ref()?;
        let CfcValue::Array(items) = &*value.borrow() else {
            return Err("sum() expects array".to_string());
        };
        let mut int_total = 0i64;
        let mut float_total = 0.0f64;
        let mut has_float = false;
        for item in items {
            match &*item.borrow() {
                CfcValue::Int(value) if has_float => {
                    #[allow(clippy::cast_precision_loss)]
                    {
                        float_total += *value as f64;
                    }
                }
                CfcValue::Int(value) => {
                    int_total = int_total
                        .checked_add(*value)
                        .ok_or_else(|| "integer sum overflow".to_string())?;
                }
                CfcValue::Float(value) if has_float => {
                    float_total += value;
                }
                CfcValue::Float(value) => {
                    #[allow(clippy::cast_precision_loss)]
                    {
                        float_total = int_total as f64 + value;
                    }
                    has_float = true;
                }
                other => {
                    return Err(format!(
                        "sum() expects numeric array, found {}",
                        other.type_name()
                    ));
                }
            }
        }
        if has_float {
            Ok(EvalValue::Float(float_total))
        } else {
            Ok(EvalValue::Int(int_total))
        }
    }

    fn builtin_keys_values(
        &self,
        name: &str,
        arg: &CheckExpr,
        module: &ModuleId,
        scope: &CheckScope<'_>,
        context: &str,
    ) -> Result<EvalValue, String> {
        let value = self.eval_expr(arg, module, scope, context)?.into_ref()?;
        let CfcValue::Dict(entries) = &*value.borrow() else {
            return Err(format!("{name}() expects dict"));
        };
        let items = if name == "keys" {
            entries.iter().map(|(key, _)| key.clone()).collect()
        } else {
            entries.iter().map(|(_, value)| value.clone()).collect()
        };
        Ok(EvalValue::Ref(CfcValueRef::new(CfcValue::Array(items))))
    }

    fn eval_field(
        &self,
        expr: &CheckExpr,
        name: &str,
        module: &ModuleId,
        scope: &CheckScope<'_>,
        context: &str,
    ) -> Result<EvalValue, String> {
        let base = self.eval_expr(expr, module, scope, context)?;
        match base {
            EvalValue::EnumType {
                module,
                name: enum_name,
            } => scope
                .enum_values
                .get(&(module, enum_name, name.to_string()))
                .cloned()
                .map(EvalValue::Ref)
                .ok_or_else(|| format!("missing field `{name}`")),
            EvalValue::ModuleNamespace { module, allow_data } => {
                if scope
                    .enum_values
                    .keys()
                    .any(|(enum_module, enum_name, _)| enum_module == &module && enum_name == name)
                {
                    return Ok(EvalValue::EnumType {
                        module,
                        name: name.to_string(),
                    });
                }
                if allow_data {
                    if let Some(value) = self
                        .result
                        .module(&module)
                        .and_then(|module| module.get(name))
                    {
                        return Ok(EvalValue::Ref(value));
                    }
                }
                Err(format!("missing field `{name}`"))
            }
            other => {
                let value = unwrap_union_ref(other.into_ref()?);
                let borrowed = value.borrow();
                let CfcValue::Object { fields, .. } = &*borrowed else {
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
        }
    }

    fn eval_index(
        &self,
        expr: &CheckExpr,
        index: &CheckExpr,
        module: &ModuleId,
        scope: &CheckScope<'_>,
        context: &str,
    ) -> Result<EvalValue, String> {
        let base = self.eval_expr(expr, module, scope, context)?.into_ref()?;
        let base = unwrap_union_ref(base);
        let index = self.eval_expr(index, module, scope, context)?;
        let borrowed = base.borrow();
        match &*borrowed {
            CfcValue::Array(items) => {
                let index = index.into_i64()?;
                let index = usize::try_from(index)
                    .map_err(|_| format!("array index `{index}` is out of bounds"))?;
                items
                    .get(index)
                    .cloned()
                    .map(EvalValue::Ref)
                    .ok_or_else(|| format!("array index `{index}` is out of bounds"))
            }
            CfcValue::Dict(entries) => entries
                .iter()
                .find(|(key, _)| eval_value_equals_ref(&index, key))
                .map(|(_, value)| EvalValue::Ref(value.clone()))
                .ok_or_else(|| "dict key not found".to_string()),
            other => Err(format!("cannot index {}", other.type_name())),
        }
    }

    fn eval_unary(
        &self,
        op: UnaryOp,
        expr: &CheckExpr,
        module: &ModuleId,
        scope: &CheckScope<'_>,
        context: &str,
    ) -> Result<EvalValue, String> {
        let value = self.eval_expr(expr, module, scope, context)?;
        match op {
            UnaryOp::Not => Ok(EvalValue::Bool(!value.into_bool()?)),
            UnaryOp::BitNot => Ok(EvalValue::Int(!value.into_i64()?)),
            UnaryOp::Neg => match value {
                EvalValue::Int(value) => Ok(EvalValue::Int(-value)),
                EvalValue::Float(value) => Ok(EvalValue::Float(-value)),
                EvalValue::Ref(value) => match &*value.borrow() {
                    CfcValue::Int(value) => Ok(EvalValue::Int(-value)),
                    CfcValue::Float(value) => Ok(EvalValue::Float(-value)),
                    other => Err(format!(
                        "expected numeric value, found {}",
                        other.type_name()
                    )),
                },
                other => Err(format!(
                    "expected numeric value, found {}",
                    other.type_name()
                )),
            },
        }
    }

    #[allow(clippy::too_many_lines)]
    fn eval_bin(
        &self,
        op: BinOp,
        lhs: &CheckExpr,
        rhs: &CheckExpr,
        module: &ModuleId,
        scope: &CheckScope<'_>,
        context: &str,
    ) -> Result<EvalValue, String> {
        match op {
            BinOp::Or => {
                let lhs = self.eval_expr(lhs, module, scope, context)?.into_bool()?;
                if lhs {
                    return Ok(EvalValue::Bool(true));
                }
                Ok(EvalValue::Bool(
                    self.eval_expr(rhs, module, scope, context)?.into_bool()?,
                ))
            }
            BinOp::And => {
                let lhs = self.eval_expr(lhs, module, scope, context)?.into_bool()?;
                if !lhs {
                    return Ok(EvalValue::Bool(false));
                }
                Ok(EvalValue::Bool(
                    self.eval_expr(rhs, module, scope, context)?.into_bool()?,
                ))
            }
            BinOp::BitOr => Ok(EvalValue::Int(
                self.eval_expr(lhs, module, scope, context)?.into_i64()?
                    | self.eval_expr(rhs, module, scope, context)?.into_i64()?,
            )),
            BinOp::BitXor => Ok(EvalValue::Int(
                self.eval_expr(lhs, module, scope, context)?.into_i64()?
                    ^ self.eval_expr(rhs, module, scope, context)?.into_i64()?,
            )),
            BinOp::BitAnd => Ok(EvalValue::Int(
                self.eval_expr(lhs, module, scope, context)?.into_i64()?
                    & self.eval_expr(rhs, module, scope, context)?.into_i64()?,
            )),
            BinOp::Add => {
                let lhs = self.eval_expr(lhs, module, scope, context)?;
                let rhs = self.eval_expr(rhs, module, scope, context)?;
                if is_string_value(&lhs) || is_string_value(&rhs) {
                    return Ok(EvalValue::String(format!(
                        "{}{}",
                        lhs.into_string()?,
                        rhs.into_string()?
                    )));
                }
                numeric_bin(lhs, rhs, i64::checked_add, |a, b| a + b, "addition")
            }
            BinOp::Sub => numeric_bin(
                self.eval_expr(lhs, module, scope, context)?,
                self.eval_expr(rhs, module, scope, context)?,
                i64::checked_sub,
                |a, b| a - b,
                "subtraction",
            ),
            BinOp::Mul => numeric_bin(
                self.eval_expr(lhs, module, scope, context)?,
                self.eval_expr(rhs, module, scope, context)?,
                i64::checked_mul,
                |a, b| a * b,
                "multiplication",
            ),
            BinOp::Div => {
                let lhs = self.eval_expr(lhs, module, scope, context)?.into_f64()?;
                let rhs = self.eval_expr(rhs, module, scope, context)?.into_f64()?;
                if rhs == 0.0 {
                    return Err("division by zero".to_string());
                }
                Ok(EvalValue::Float(lhs / rhs))
            }
            BinOp::IntDiv => {
                let lhs = self.eval_expr(lhs, module, scope, context)?.into_i64()?;
                let rhs = self.eval_expr(rhs, module, scope, context)?.into_i64()?;
                if rhs == 0 {
                    return Err("division by zero".to_string());
                }
                lhs.checked_div(rhs)
                    .map(EvalValue::Int)
                    .ok_or_else(|| "integer division overflow".to_string())
            }
            BinOp::Mod => {
                let lhs = self.eval_expr(lhs, module, scope, context)?.into_i64()?;
                let rhs = self.eval_expr(rhs, module, scope, context)?.into_i64()?;
                if rhs == 0 {
                    return Err("modulo by zero".to_string());
                }
                lhs.checked_rem(rhs)
                    .map(EvalValue::Int)
                    .ok_or_else(|| "integer remainder overflow".to_string())
            }
            BinOp::Pow => {
                let lhs = self.eval_expr(lhs, module, scope, context)?;
                let rhs = self.eval_expr(rhs, module, scope, context)?;
                match (lhs.number_kind()?, rhs.number_kind()?) {
                    (NumberKind::Int, NumberKind::Int) => {
                        let exp = rhs.into_i64()?;
                        let exp = u32::try_from(exp)
                            .map_err(|_| "integer exponent must be nonnegative".to_string())?;
                        lhs.into_i64()?
                            .checked_pow(exp)
                            .map(EvalValue::Int)
                            .ok_or_else(|| "integer exponentiation overflow".to_string())
                    }
                    _ => Ok(EvalValue::Float(lhs.into_f64()?.powf(rhs.into_f64()?))),
                }
            }
            BinOp::Shl => {
                let lhs = self.eval_expr(lhs, module, scope, context)?.into_i64()?;
                let rhs = shift_amount(self.eval_expr(rhs, module, scope, context)?.into_i64()?)?;
                lhs.checked_shl(rhs)
                    .map(EvalValue::Int)
                    .ok_or_else(|| "left shift overflow".to_string())
            }
            BinOp::Shr => {
                let lhs = self.eval_expr(lhs, module, scope, context)?.into_i64()?;
                let rhs = shift_amount(self.eval_expr(rhs, module, scope, context)?.into_i64()?)?;
                lhs.checked_shr(rhs)
                    .map(EvalValue::Int)
                    .ok_or_else(|| "right shift overflow".to_string())
            }
        }
    }

    fn eval_cmp_chain(
        &self,
        first: &CheckExpr,
        rest: &[(CmpOp, CheckExpr)],
        module: &ModuleId,
        scope: &CheckScope<'_>,
        context: &str,
    ) -> Result<EvalValue, String> {
        let mut lhs = self.eval_expr(first, module, scope, context)?;
        for (op, rhs_expr) in rest {
            let rhs = self.eval_expr(rhs_expr, module, scope, context)?;
            if !compare_values(*op, &lhs, &rhs)? {
                return Ok(EvalValue::Bool(false));
            }
            lhs = rhs;
        }
        Ok(EvalValue::Bool(true))
    }
}

fn quantifier_name(kind: QuantifierKind) -> &'static str {
    match kind {
        QuantifierKind::All => "all",
        QuantifierKind::Any => "any",
        QuantifierKind::None => "none",
    }
}

fn unwrap_union_ref(mut value: CfcValueRef) -> CfcValueRef {
    while let Some(inner) = {
        let borrowed = value.borrow();
        match &*borrowed {
            CfcValue::Union { value, .. } => Some(value.clone()),
            _ => None,
        }
    } {
        value = inner;
    }
    value
}

fn unique_key(value: &CfcValueRef) -> Result<String, String> {
    match &*value.borrow() {
        CfcValue::Int(value) => Ok(format!("int:{value}")),
        CfcValue::Bool(value) => Ok(format!("bool:{value}")),
        CfcValue::String(value) => Ok(format!("string:{value}")),
        CfcValue::Enum {
            enum_type, variant, ..
        } => Ok(format!(
            "enum:{}:{}:{variant}",
            enum_type.module.as_str(),
            enum_type.name
        )),
        CfcValue::Float(_) => Err("unique() does not support float arrays".to_string()),
        other => Err(format!(
            "unique() expects scalar array, found {}",
            other.type_name()
        )),
    }
}

fn ensure_orderable_for_builtin(name: &str, value: &CfcValueRef) -> Result<(), String> {
    match &*value.borrow() {
        CfcValue::Int(_) | CfcValue::Float(_) | CfcValue::Enum { .. } => Ok(()),
        other => Err(format!(
            "{name}() expects int, float, or enum array, found {}",
            other.type_name()
        )),
    }
}
