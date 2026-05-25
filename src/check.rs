use crate::ast::{BinOp, CheckBlock, CheckExpr, CheckExprKind, CmpOp, CondStmt, Item, UnaryOp};
use crate::container::{CfcContainer, CfcResult, ModuleId};
use crate::error::{AllFailedItem, CheckError, CheckErrorKind};
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

struct CheckScope<'a> {
    layers: Vec<BTreeMap<String, CfcValueRef>>,
    enum_values: &'a HashMap<(ModuleId, String, String), CfcValueRef>,
}

#[derive(Debug, Clone)]
enum EvalValue {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    Enum {
        enum_type: CfcNominalType,
        variant: String,
        value: i64,
    },
    Ref(CfcValueRef),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NumberKind {
    Int,
    Float,
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
                        self.check_type_instance(type_name, fields, value.ptr_key(), enum_values);
                    }
                    fields.values().cloned().collect::<Vec<_>>()
                }
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
        ptr_key: usize,
        enum_values: &HashMap<(ModuleId, String, String), CfcValueRef>,
    ) {
        if !self.checked_objects.insert(ptr_key) {
            return;
        }
        let Some(block) = self.type_check_block(type_name).cloned() else {
            return;
        };
        let mut scope = CheckScope::new(enum_values, self.base_scope(&type_name.module, false));
        scope.push(fields.clone());
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
                .map(|(name, value)| (name.to_string(), value))
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
    ) -> BTreeMap<String, CfcValueRef> {
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
            CondStmt::All {
                binding,
                collection,
                body,
                span,
            } => self.eval_all(binding, collection, body, *span, module, scope, context),
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
                        .push(cond_failed(describe_expr(expr), context, expr.span));
                    true
                }
                other => {
                    self.errors.push(eval_error(
                        format!("condition must be bool, found {}", other.type_name()),
                        context,
                        expr.span,
                    ));
                    false
                }
            },
            Err(message) => {
                self.errors.push(eval_error(message, context, expr.span));
                false
            }
        }
    }

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    fn eval_all(
        &mut self,
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
                    "failed to evaluate all collection `{}`",
                    describe_expr(collection)
                ),
                context,
                collection.span,
            ));
            return false;
        };
        let entries = match value {
            EvalValue::Ref(value) => match &*value.borrow() {
                CfcValue::Array(items) => items
                    .iter()
                    .enumerate()
                    .map(|(index, value)| (format!("[{index}]"), value.clone()))
                    .collect::<Vec<_>>(),
                CfcValue::Dict(items) => items
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
                other => {
                    self.errors.push(eval_error(
                        format!(
                            "all collection must be array or dict, found {}",
                            other.type_name()
                        ),
                        context,
                        collection.span,
                    ));
                    return false;
                }
            },
            other => {
                self.errors.push(eval_error(
                    format!(
                        "all collection must be array or dict, found {}",
                        other.type_name()
                    ),
                    context,
                    collection.span,
                ));
                return false;
            }
        };

        let total = entries.len();
        let mut failed = Vec::new();
        for (label, item) in entries {
            scope.push(BTreeMap::from([(binding.to_string(), item)]));
            let item_context = format!("{context} {binding}{label}");
            let before = self.errors.len();
            for stmt in body {
                if !self.eval_stmt(stmt, module, scope, &item_context) {
                    let errors = self.errors.split_off(before);
                    failed.push(AllFailedItem {
                        key: format!("{binding}{label}"),
                        errors,
                    });
                    scope.pop();
                    self.errors.push(all_failed(
                        format!("all {binding} in {}", describe_expr(collection)),
                        context,
                        total,
                        failed,
                        span,
                    ));
                    return true;
                }
            }
            scope.pop();
            let errors = self.errors.split_off(before);
            if !errors.is_empty() {
                failed.push(AllFailedItem {
                    key: format!("{binding}{label}"),
                    errors,
                });
            }
        }
        if !failed.is_empty() {
            self.errors.push(all_failed(
                format!("all {binding} in {}", describe_expr(collection)),
                context,
                total,
                failed,
                span,
            ));
        }
        true
    }

    fn eval_expr(
        &self,
        expr: &CheckExpr,
        module: &ModuleId,
        scope: &CheckScope<'_>,
        context: &str,
    ) -> Result<EvalValue, String> {
        match &expr.kind {
            CheckExprKind::Int(value) => Ok(EvalValue::Int(*value)),
            CheckExprKind::Float(value) => Ok(EvalValue::Float(*value)),
            CheckExprKind::Bool(value) => Ok(EvalValue::Bool(*value)),
            CheckExprKind::Str(value) => Ok(EvalValue::String(value.clone())),
            CheckExprKind::Name(name) => scope
                .lookup(name)
                .map(EvalValue::Ref)
                .ok_or_else(|| format!("unknown name `{name}`")),
            CheckExprKind::Field { expr, name } => {
                self.eval_field(expr, name, module, scope, context)
            }
            CheckExprKind::Index { expr, index } => {
                self.eval_index(expr, index, module, scope, context)
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

    fn eval_field(
        &self,
        expr: &CheckExpr,
        name: &str,
        module: &ModuleId,
        scope: &CheckScope<'_>,
        context: &str,
    ) -> Result<EvalValue, String> {
        let base = self.eval_expr(expr, module, scope, context)?;
        if let EvalValue::Ref(value) = &base {
            if let Some(namespace_value) = self.namespace_field(value, name, scope) {
                return Ok(EvalValue::Ref(namespace_value));
            }
        }
        let value = base.into_ref()?;
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

    fn namespace_field(
        &self,
        value: &CfcValueRef,
        field: &str,
        scope: &CheckScope<'_>,
    ) -> Option<CfcValueRef> {
        let borrowed = value.borrow();
        let CfcValue::Object { fields, .. } = &*borrowed else {
            return None;
        };
        let module_value = fields.get("__module")?;
        let module = {
            let module_borrowed = module_value.borrow();
            let CfcValue::String(module) = &*module_borrowed else {
                return None;
            };
            ModuleId::from(module.clone())
        };

        if let Some(name_value) = fields.get("__name") {
            let name_borrowed = name_value.borrow();
            let CfcValue::String(enum_name) = &*name_borrowed else {
                return None;
            };
            return scope
                .enum_values
                .get(&(module, enum_name.clone(), field.to_string()))
                .cloned();
        }

        if scope
            .enum_values
            .keys()
            .any(|(enum_module, enum_name, _)| enum_module == &module && enum_name == field)
        {
            return Some(enum_type_value(&module, field));
        }

        let allow_data = fields
            .get("__allow_data")
            .is_some_and(|value| matches!(&*value.borrow(), CfcValue::Bool(true)));
        if allow_data {
            return self
                .result
                .module(&module)
                .and_then(|module| module.get(field));
        }
        None
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
                if matches!(lhs, EvalValue::String(_)) || matches!(rhs, EvalValue::String(_)) {
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

impl<'a> CheckScope<'a> {
    fn new(
        enum_values: &'a HashMap<(ModuleId, String, String), CfcValueRef>,
        base: BTreeMap<String, CfcValueRef>,
    ) -> Self {
        let mut scope = Self {
            layers: Vec::new(),
            enum_values,
        };
        scope.push(base);
        scope
    }

    fn push(&mut self, layer: BTreeMap<String, CfcValueRef>) {
        self.layers.push(layer);
    }

    fn pop(&mut self) {
        self.layers.pop();
    }

    fn lookup(&self, name: &str) -> Option<CfcValueRef> {
        self.layers
            .iter()
            .rev()
            .find_map(|layer| layer.get(name).cloned())
    }
}

impl EvalValue {
    fn type_name(&self) -> &'static str {
        match self {
            EvalValue::Int(_) => "int",
            EvalValue::Float(_) => "float",
            EvalValue::Bool(_) => "bool",
            EvalValue::String(_) => "string",
            EvalValue::Enum { .. } => "enum",
            EvalValue::Ref(value) => value.borrow().type_name(),
        }
    }

    fn into_ref(self) -> Result<CfcValueRef, String> {
        match self {
            EvalValue::Ref(value) => Ok(value),
            other => Err(format!("expected reference, found {}", other.type_name())),
        }
    }

    fn into_bool(self) -> Result<bool, String> {
        match self {
            EvalValue::Bool(value) => Ok(value),
            EvalValue::Ref(value) => match &*value.borrow() {
                CfcValue::Bool(value) => Ok(*value),
                other => Err(format!("expected bool, found {}", other.type_name())),
            },
            other => Err(format!("expected bool, found {}", other.type_name())),
        }
    }

    fn into_i64(self) -> Result<i64, String> {
        match self {
            EvalValue::Int(value) => Ok(value),
            EvalValue::Ref(value) => match &*value.borrow() {
                CfcValue::Int(value) => Ok(*value),
                other => Err(format!("expected int, found {}", other.type_name())),
            },
            other => Err(format!("expected int, found {}", other.type_name())),
        }
    }

    #[allow(clippy::cast_precision_loss)]
    fn into_f64(self) -> Result<f64, String> {
        match self {
            EvalValue::Int(value) => Ok(value as f64),
            EvalValue::Float(value) => Ok(value),
            EvalValue::Ref(value) => match &*value.borrow() {
                CfcValue::Int(value) => Ok(*value as f64),
                CfcValue::Float(value) => Ok(*value),
                other => Err(format!(
                    "expected numeric value, found {}",
                    other.type_name()
                )),
            },
            other => Err(format!(
                "expected numeric value, found {}",
                other.type_name()
            )),
        }
    }

    fn into_string(self) -> Result<String, String> {
        match self {
            EvalValue::String(value) => Ok(value),
            EvalValue::Ref(value) => match &*value.borrow() {
                CfcValue::String(value) => Ok(value.clone()),
                other => Err(format!("expected string, found {}", other.type_name())),
            },
            other => Err(format!("expected string, found {}", other.type_name())),
        }
    }

    fn number_kind(&self) -> Result<NumberKind, String> {
        match self {
            EvalValue::Int(_) => Ok(NumberKind::Int),
            EvalValue::Float(_) => Ok(NumberKind::Float),
            EvalValue::Ref(value) => match &*value.borrow() {
                CfcValue::Int(_) => Ok(NumberKind::Int),
                CfcValue::Float(_) => Ok(NumberKind::Float),
                other => Err(format!(
                    "expected numeric value, found {}",
                    other.type_name()
                )),
            },
            other => Err(format!(
                "expected numeric value, found {}",
                other.type_name()
            )),
        }
    }
}

fn enum_type_value(module: &ModuleId, name: &str) -> CfcValueRef {
    CfcValueRef::new(CfcValue::Object {
        type_name: None,
        fields: BTreeMap::from([
            (
                "__module".to_string(),
                CfcValueRef::new(CfcValue::String(module.to_string())),
            ),
            (
                "__name".to_string(),
                CfcValueRef::new(CfcValue::String(name.to_string())),
            ),
        ]),
    })
}

fn module_namespace_value(module: &ModuleId, allow_data: bool) -> CfcValueRef {
    CfcValueRef::new(CfcValue::Object {
        type_name: None,
        fields: BTreeMap::from([
            (
                "__module".to_string(),
                CfcValueRef::new(CfcValue::String(module.to_string())),
            ),
            (
                "__allow_data".to_string(),
                CfcValueRef::new(CfcValue::Bool(allow_data)),
            ),
        ]),
    })
}

fn numeric_bin(
    lhs: EvalValue,
    rhs: EvalValue,
    int_op: impl FnOnce(i64, i64) -> Option<i64>,
    float_op: impl FnOnce(f64, f64) -> f64,
    operation: &str,
) -> Result<EvalValue, String> {
    match (lhs.number_kind()?, rhs.number_kind()?) {
        (NumberKind::Int, NumberKind::Int) => int_op(lhs.into_i64()?, rhs.into_i64()?)
            .map(EvalValue::Int)
            .ok_or_else(|| format!("integer {operation} overflow")),
        _ => Ok(EvalValue::Float(float_op(lhs.into_f64()?, rhs.into_f64()?))),
    }
}

fn shift_amount(value: i64) -> Result<u32, String> {
    let amount =
        u32::try_from(value).map_err(|_| "shift amount must be nonnegative".to_string())?;
    if amount >= i64::BITS {
        return Err(format!("shift amount `{amount}` is out of range"));
    }
    Ok(amount)
}

fn compare_values(op: CmpOp, lhs: &EvalValue, rhs: &EvalValue) -> Result<bool, String> {
    match op {
        CmpOp::Eq => equal_values(lhs, rhs),
        CmpOp::Ne => Ok(!equal_values(lhs, rhs)?),
        CmpOp::Lt | CmpOp::Le | CmpOp::Gt | CmpOp::Ge => compare_ordered(op, lhs, rhs),
    }
}

#[allow(clippy::float_cmp)]
fn equal_values(lhs: &EvalValue, rhs: &EvalValue) -> Result<bool, String> {
    match (materialize(lhs)?, materialize(rhs)?) {
        (EvalValue::Int(a), EvalValue::Int(b)) => Ok(a == b),
        (EvalValue::Float(a), EvalValue::Float(b)) => Ok(a == b),
        (EvalValue::Bool(a), EvalValue::Bool(b)) => Ok(a == b),
        (EvalValue::String(a), EvalValue::String(b)) => Ok(a == b),
        (
            EvalValue::Enum {
                enum_type: a_ty,
                variant: a_variant,
                ..
            },
            EvalValue::Enum {
                enum_type: b_ty,
                variant: b_variant,
                ..
            },
        ) => Ok(a_ty == b_ty && a_variant == b_variant),
        (a, b) => Err(format!(
            "cannot compare {} and {} for equality",
            a.type_name(),
            b.type_name()
        )),
    }
}

#[allow(clippy::cast_precision_loss)]
fn compare_ordered(op: CmpOp, lhs: &EvalValue, rhs: &EvalValue) -> Result<bool, String> {
    match (materialize(lhs)?, materialize(rhs)?) {
        (EvalValue::Int(a), EvalValue::Int(b)) => Ok(apply_cmp(op, &a, &b)),
        (EvalValue::Int(a), EvalValue::Float(b)) => Ok(apply_cmp(op, &(a as f64), &b)),
        (EvalValue::Float(a), EvalValue::Int(b)) => Ok(apply_cmp(op, &a, &(b as f64))),
        (EvalValue::Float(a), EvalValue::Float(b)) => Ok(apply_cmp(op, &a, &b)),
        (
            EvalValue::Enum {
                enum_type: a_ty,
                value: a_value,
                ..
            },
            EvalValue::Enum {
                enum_type: b_ty,
                value: b_value,
                ..
            },
        ) if a_ty == b_ty => Ok(apply_cmp(op, &a_value, &b_value)),
        (
            EvalValue::Enum {
                enum_type: a_ty, ..
            },
            EvalValue::Enum {
                enum_type: b_ty, ..
            },
        ) => Err(format!(
            "cannot compare enum `{}` with enum `{}`",
            a_ty.name, b_ty.name
        )),
        (a, b) => Err(format!(
            "cannot order compare {} and {}",
            a.type_name(),
            b.type_name()
        )),
    }
}

fn apply_cmp<T: PartialOrd>(op: CmpOp, lhs: &T, rhs: &T) -> bool {
    match op {
        CmpOp::Eq => lhs == rhs,
        CmpOp::Ne => lhs != rhs,
        CmpOp::Lt => lhs < rhs,
        CmpOp::Le => lhs <= rhs,
        CmpOp::Gt => lhs > rhs,
        CmpOp::Ge => lhs >= rhs,
    }
}

fn materialize(value: &EvalValue) -> Result<EvalValue, String> {
    match value {
        EvalValue::Ref(value) => match &*value.borrow() {
            CfcValue::Int(value) => Ok(EvalValue::Int(*value)),
            CfcValue::Float(value) => Ok(EvalValue::Float(*value)),
            CfcValue::Bool(value) => Ok(EvalValue::Bool(*value)),
            CfcValue::String(value) => Ok(EvalValue::String(value.clone())),
            CfcValue::Enum {
                enum_type,
                variant,
                value,
            } => Ok(EvalValue::Enum {
                enum_type: enum_type.clone(),
                variant: variant.clone(),
                value: *value,
            }),
            other => Err(format!(
                "expected scalar value, found {}",
                other.type_name()
            )),
        },
        other => Ok(other.clone()),
    }
}

fn eval_value_equals_ref(value: &EvalValue, other: &CfcValueRef) -> bool {
    equal_values(value, &EvalValue::Ref(other.clone())).unwrap_or(false)
}

fn describe_expr(expr: &CheckExpr) -> String {
    match &expr.kind {
        CheckExprKind::Int(value) => value.to_string(),
        CheckExprKind::Float(value) => value.to_string(),
        CheckExprKind::Bool(value) => value.to_string(),
        CheckExprKind::Str(value) => format!("{value:?}"),
        CheckExprKind::Name(name) => name.clone(),
        CheckExprKind::Field { expr, name } => format!("{}.{}", describe_expr(expr), name),
        CheckExprKind::Index { expr, index } => {
            format!("{}[{}]", describe_expr(expr), describe_expr(index))
        }
        CheckExprKind::BinOp { op, lhs, rhs } => {
            format!(
                "{} {} {}",
                describe_expr(lhs),
                bin_op_name(*op),
                describe_expr(rhs)
            )
        }
        CheckExprKind::Unary { op, expr } => {
            format!("{}{}", unary_op_name(*op), describe_expr(expr))
        }
        CheckExprKind::CmpChain { first, rest } => {
            let mut out = describe_expr(first);
            for (op, expr) in rest {
                out.push(' ');
                out.push_str(cmp_op_name(*op));
                out.push(' ');
                out.push_str(&describe_expr(expr));
            }
            out
        }
    }
}

fn bin_op_name(op: BinOp) -> &'static str {
    match op {
        BinOp::Or => "||",
        BinOp::And => "&&",
        BinOp::BitOr => "|",
        BinOp::BitXor => "^",
        BinOp::BitAnd => "&",
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::IntDiv => "//",
        BinOp::Mod => "%",
        BinOp::Pow => "**",
        BinOp::Shl => "<<",
        BinOp::Shr => ">>",
    }
}

fn unary_op_name(op: UnaryOp) -> &'static str {
    match op {
        UnaryOp::Not => "!",
        UnaryOp::BitNot => "~",
        UnaryOp::Neg => "-",
    }
}

fn cmp_op_name(op: CmpOp) -> &'static str {
    match op {
        CmpOp::Eq => "==",
        CmpOp::Ne => "!=",
        CmpOp::Lt => "<",
        CmpOp::Le => "<=",
        CmpOp::Gt => ">",
        CmpOp::Ge => ">=",
    }
}

fn cond_failed(source: String, context: &str, span: Span) -> CheckError {
    let message = format!("check failed [{context}]: {source}");
    CheckError {
        message,
        span: Some(span),
        kind: CheckErrorKind::CondFailed {
            evaluated: source.clone(),
            source,
            context: context.to_string(),
        },
    }
}

fn all_failed(
    source: String,
    context: &str,
    total: usize,
    failed: Vec<AllFailedItem>,
    span: Span,
) -> CheckError {
    let message = format!(
        "check failed [{context}]: {source} ({}/{total} failed)",
        failed.len()
    );
    CheckError {
        message,
        span: Some(span),
        kind: CheckErrorKind::AllFailed {
            source,
            context: context.to_string(),
            total,
            failed,
        },
    }
}

fn eval_error(message: String, context: &str, span: Span) -> CheckError {
    CheckError {
        message: format!("check eval error [{context}]: {message}"),
        span: Some(span),
        kind: CheckErrorKind::EvalError {
            message,
            context: context.to_string(),
        },
    }
}
