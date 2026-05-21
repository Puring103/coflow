use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::ast::{BinaryOp, UnaryOp};
use crate::hir::{ClosureData, GlobalId, HirExpr, HirGlobal, HirModule, Ty, Value, VariantId};
use crate::span::Span;

use super::{Diagnostic, SemaErrorKind};

pub fn evaluate_configs(module: &mut HirModule, diagnostics: &mut Vec<Diagnostic>) {
    let config_ids: Vec<GlobalId> = module
        .globals
        .iter()
        .filter_map(|global| match global {
            HirGlobal::Config { id, .. } => Some(*id),
            _ => None,
        })
        .collect();

    let mut deps = HashMap::<GlobalId, Vec<GlobalId>>::new();
    for id in &config_ids {
        if let Some(HirGlobal::Config { ty, value, .. }) = module.global(*id) {
            let mut found = Vec::new();
            collect_config_deps(module, value, &mut found, diagnostics);
            if let Some(Ty::Class(class_id)) = ty {
                collect_class_config_deps(module, *class_id, &mut found, diagnostics);
            }
            deps.insert(*id, found);
        }
    }

    let mut order = Vec::new();
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    for id in &config_ids {
        visit_config(
            *id,
            &deps,
            &mut visiting,
            &mut visited,
            &mut order,
            diagnostics,
        );
    }
    module.config_eval_order = order.clone();

    let mut values = HashMap::<GlobalId, Value>::new();
    for id in order {
        let Some(HirGlobal::Config {
            ty, value, span, ..
        }) = module.global(id).cloned()
        else {
            continue;
        };
        match eval_expr(module, &values, &value, None) {
            Ok(mut value) => {
                if let Some(ty) = ty {
                    if !value_matches_ty(module, &value, &ty) {
                        diagnostics.push(Diagnostic::Sema(SemaErrorKind::ConfigTypeMismatch, span));
                        continue;
                    }
                    if let Ty::Class(class_id) = ty {
                        value = fill_class_defaults(
                            module,
                            &values,
                            class_id,
                            value,
                            diagnostics,
                            span,
                        );
                        run_class_checks(module, &values, class_id, &value, diagnostics, span);
                    }
                }
                values.insert(id, value);
            }
            Err(kind) => diagnostics.push(Diagnostic::Sema(kind, span)),
        }
    }

    module.config_values = values.into_iter().collect();
    module.config_values.sort_by_key(|(id, _)| id.0);
}

fn collect_config_deps(
    module: &HirModule,
    expr: &HirExpr,
    out: &mut Vec<GlobalId>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match expr {
        HirExpr::Global { id, span } => match module.global(*id) {
            Some(HirGlobal::Config { .. }) => out.push(*id),
            Some(HirGlobal::Var { .. }) => {
                diagnostics.push(Diagnostic::Sema(SemaErrorKind::ConfigDependsOnVar, *span));
            }
            _ => {}
        },
        HirExpr::Closure { .. } | HirExpr::Error(_) => {}
        HirExpr::Const { .. }
        | HirExpr::Local { .. }
        | HirExpr::Upvalue { .. }
        | HirExpr::Variant { .. }
        | HirExpr::SelfField { .. } => {}
        HirExpr::Unary { expr, .. } | HirExpr::TypeGuard { expr, .. } => {
            collect_config_deps(module, expr, out, diagnostics);
        }
        HirExpr::Binary { lhs, rhs, .. }
        | HirExpr::NullCoalesce {
            left: lhs,
            right: rhs,
            ..
        } => {
            collect_config_deps(module, lhs, out, diagnostics);
            collect_config_deps(module, rhs, out, diagnostics);
        }
        HirExpr::AndChain { exprs, .. }
        | HirExpr::Array {
            elements: exprs, ..
        } => {
            for expr in exprs {
                collect_config_deps(module, expr, out, diagnostics);
            }
        }
        HirExpr::Call { callee, args, .. } => {
            collect_config_deps(module, callee, out, diagnostics);
            for arg in args {
                collect_config_deps(module, &arg.value, out, diagnostics);
            }
        }
        HirExpr::Field { obj, .. } | HirExpr::OptField { obj, .. } => {
            collect_config_deps(module, obj, out, diagnostics);
        }
        HirExpr::Index { obj, index, .. } | HirExpr::OptIndex { obj, index, .. } => {
            collect_config_deps(module, obj, out, diagnostics);
            collect_config_deps(module, index, out, diagnostics);
        }
        HirExpr::Object {
            fields, spreads, ..
        } => {
            for (_, value) in fields {
                collect_config_deps(module, value, out, diagnostics);
            }
            for spread in spreads {
                collect_config_deps(module, spread, out, diagnostics);
            }
        }
        HirExpr::Dict { entries, .. } => {
            for (key, value) in entries {
                collect_config_deps(module, key, out, diagnostics);
                collect_config_deps(module, value, out, diagnostics);
            }
        }
        HirExpr::Range { start, end, .. } => {
            collect_config_deps(module, start, out, diagnostics);
            collect_config_deps(module, end, out, diagnostics);
        }
        HirExpr::If {
            cond,
            then_expr,
            else_expr,
            ..
        } => {
            collect_config_deps(module, cond, out, diagnostics);
            collect_config_deps(module, then_expr, out, diagnostics);
            collect_config_deps(module, else_expr, out, diagnostics);
        }
    }
}

fn collect_class_config_deps(
    module: &HirModule,
    class_id: crate::hir::ClassId,
    out: &mut Vec<GlobalId>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(class) = module.class(class_id) else {
        return;
    };
    for field in &class.fields {
        if let Some(default) = &field.default {
            collect_config_deps(module, default, out, diagnostics);
        }
    }
    for arm in &class.checks {
        collect_config_deps(module, &arm.cond, out, diagnostics);
        collect_config_deps(module, &arm.message, out, diagnostics);
    }
}

fn visit_config(
    id: GlobalId,
    deps: &HashMap<GlobalId, Vec<GlobalId>>,
    visiting: &mut HashSet<GlobalId>,
    visited: &mut HashSet<GlobalId>,
    order: &mut Vec<GlobalId>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if visited.contains(&id) {
        return;
    }
    if !visiting.insert(id) {
        diagnostics.push(Diagnostic::Sema(
            SemaErrorKind::ConfigCircularDependency,
            Span { start: 0, end: 0 },
        ));
        return;
    }
    for dep in deps.get(&id).into_iter().flatten() {
        visit_config(*dep, deps, visiting, visited, order, diagnostics);
    }
    visiting.remove(&id);
    visited.insert(id);
    order.push(id);
}

fn eval_expr(
    module: &HirModule,
    configs: &HashMap<GlobalId, Value>,
    expr: &HirExpr,
    self_value: Option<&Value>,
) -> Result<Value, SemaErrorKind> {
    match expr {
        HirExpr::Const { value, .. } => Ok(value.clone()),
        HirExpr::Global { id, .. } => match module.global(*id) {
            Some(HirGlobal::Config { .. }) => configs
                .get(id)
                .cloned()
                .ok_or(SemaErrorKind::ConfigCircularDependency),
            Some(HirGlobal::Function { fn_id, .. }) => {
                Ok(Value::Closure(Rc::new(ClosureData { fn_id: *fn_id })))
            }
            Some(HirGlobal::Enum { enum_id, .. }) => Ok(Value::EnumVariant(*enum_id, VariantId(0))),
            Some(HirGlobal::Var { .. }) => Err(SemaErrorKind::ConfigDependsOnVar),
            _ => Err(SemaErrorKind::ConfigNonConstant),
        },
        HirExpr::Variant {
            enum_id, variant, ..
        } => Ok(Value::EnumVariant(*enum_id, *variant)),
        HirExpr::Closure { fn_id, .. } => {
            Ok(Value::Closure(Rc::new(ClosureData { fn_id: *fn_id })))
        }
        HirExpr::SelfField { name, .. } => match self_value {
            Some(Value::Object { fields, .. }) => fields
                .iter()
                .find_map(|(field, value)| (field == name).then_some(value.clone()))
                .ok_or(SemaErrorKind::ConfigNonConstant),
            _ => Err(SemaErrorKind::ConfigNonConstant),
        },
        HirExpr::Unary { op, expr, .. } => {
            let value = eval_expr(module, configs, expr, self_value)?;
            eval_unary(*op, value)
        }
        HirExpr::Binary { op, lhs, rhs, .. } => {
            let lhs = eval_expr(module, configs, lhs, self_value)?;
            if *op == BinaryOp::And {
                if let Value::Bool(false) = lhs {
                    return Ok(Value::Bool(false));
                }
            }
            if *op == BinaryOp::Or {
                if let Value::Bool(true) = lhs {
                    return Ok(Value::Bool(true));
                }
            }
            let rhs = eval_expr(module, configs, rhs, self_value)?;
            eval_binary(*op, lhs, rhs)
        }
        HirExpr::AndChain { exprs, .. } => {
            if self_value.is_none() {
                return Err(SemaErrorKind::ConfigNonConstant);
            }
            for expr in exprs {
                match eval_expr(module, configs, expr, self_value)? {
                    Value::Bool(true) => {}
                    Value::Bool(false) => return Ok(Value::Bool(false)),
                    _ => return Err(SemaErrorKind::ConfigTypeMismatch),
                }
            }
            Ok(Value::Bool(true))
        }
        HirExpr::NullCoalesce { left, right, .. } => {
            let left = eval_expr(module, configs, left, self_value)?;
            if matches!(left, Value::Null) {
                eval_expr(module, configs, right, self_value)
            } else {
                Ok(left)
            }
        }
        HirExpr::Call { .. } => Err(SemaErrorKind::ConfigNonConstant),
        HirExpr::Field { obj, field, .. } | HirExpr::OptField { obj, field, .. } => {
            let obj = eval_expr(module, configs, obj, self_value)?;
            match obj {
                Value::Object { fields, .. } => Ok(fields
                    .iter()
                    .find_map(|(name, value)| (name == field).then_some(value.clone()))
                    .unwrap_or(Value::Null)),
                Value::Null => Ok(Value::Null),
                _ => Err(SemaErrorKind::ConfigNonConstant),
            }
        }
        HirExpr::Index { obj, index, .. } | HirExpr::OptIndex { obj, index, .. } => {
            let obj = eval_expr(module, configs, obj, self_value)?;
            let index = eval_expr(module, configs, index, self_value)?;
            eval_index(obj, index)
        }
        HirExpr::Array { elements, .. } => {
            let mut values = Vec::new();
            for element in elements {
                values.push(eval_expr(module, configs, element, self_value)?);
            }
            Ok(Value::Array(Rc::new(values)))
        }
        HirExpr::Object {
            class,
            fields,
            spreads,
            ..
        } => {
            let mut output = Vec::<(String, Value)>::new();
            for spread in spreads {
                match eval_expr(module, configs, spread, self_value)? {
                    Value::Object { fields, .. } => {
                        for (name, value) in fields.iter() {
                            set_field(&mut output, name.clone(), value.clone());
                        }
                    }
                    _ => return Err(SemaErrorKind::ConfigNonConstant),
                }
            }
            for (name, expr) in fields {
                let value = eval_expr(module, configs, expr, self_value)?;
                set_field(&mut output, name.clone(), value);
            }
            Ok(Value::Object {
                class: *class,
                fields: Rc::new(output),
            })
        }
        HirExpr::Dict { entries, .. } => {
            let mut values = Vec::new();
            for (key, value) in entries {
                values.push((
                    eval_expr(module, configs, key, self_value)?,
                    eval_expr(module, configs, value, self_value)?,
                ));
            }
            Ok(Value::Dict(Rc::new(values)))
        }
        HirExpr::Range {
            start,
            end,
            inclusive,
            ..
        } => {
            let start = eval_expr(module, configs, start, self_value)?;
            let end = eval_expr(module, configs, end, self_value)?;
            match (start, end) {
                (Value::Int(start), Value::Int(end)) => Ok(Value::Range {
                    start,
                    end,
                    inclusive: *inclusive,
                }),
                _ => Err(SemaErrorKind::ConfigTypeMismatch),
            }
        }
        HirExpr::If {
            cond,
            then_expr,
            else_expr,
            ..
        } => match eval_expr(module, configs, cond, self_value)? {
            Value::Bool(true) => eval_expr(module, configs, then_expr, self_value),
            Value::Bool(false) => eval_expr(module, configs, else_expr, self_value),
            _ => Err(SemaErrorKind::ConfigTypeMismatch),
        },
        HirExpr::TypeGuard { expr, ty, .. } => {
            let value = eval_expr(module, configs, expr, self_value)?;
            if value_matches_ty(module, &value, ty) {
                Ok(value)
            } else {
                Err(SemaErrorKind::ConfigTypeMismatch)
            }
        }
        HirExpr::Local { .. } | HirExpr::Upvalue { .. } | HirExpr::Error(_) => {
            Err(SemaErrorKind::ConfigNonConstant)
        }
    }
}

fn fill_class_defaults(
    module: &HirModule,
    configs: &HashMap<GlobalId, Value>,
    class_id: crate::hir::ClassId,
    value: Value,
    diagnostics: &mut Vec<Diagnostic>,
    span: Span,
) -> Value {
    let Value::Object { fields, .. } = value else {
        return value;
    };
    let Some(class) = module.class(class_id) else {
        return Value::Object {
            class: Some(class_id),
            fields,
        };
    };

    let mut output = fields.as_ref().clone();
    for field in &class.fields {
        if output.iter().any(|(name, _)| name == &field.name) {
            continue;
        }
        if let Some(default) = &field.default {
            match eval_expr(module, configs, default, None) {
                Ok(value) => output.push((field.name.clone(), value)),
                Err(kind) => diagnostics.push(Diagnostic::Sema(kind, span)),
            }
        }
    }
    Value::Object {
        class: Some(class_id),
        fields: Rc::new(output),
    }
}

fn run_class_checks(
    module: &HirModule,
    configs: &HashMap<GlobalId, Value>,
    class_id: crate::hir::ClassId,
    value: &Value,
    diagnostics: &mut Vec<Diagnostic>,
    span: Span,
) {
    let Some(class) = module.class(class_id) else {
        return;
    };
    for arm in &class.checks {
        match eval_expr(module, configs, &arm.cond, Some(value)) {
            Ok(Value::Bool(true)) => {}
            Ok(Value::Bool(false)) => {
                diagnostics.push(Diagnostic::Sema(SemaErrorKind::ConfigCheckFailed, arm.span));
            }
            Ok(_) => diagnostics.push(Diagnostic::Sema(
                SemaErrorKind::ConfigTypeMismatch,
                arm.span,
            )),
            Err(kind) => diagnostics.push(Diagnostic::Sema(kind, span)),
        }
    }
}

fn set_field(fields: &mut Vec<(String, Value)>, name: String, value: Value) {
    if let Some((_, existing)) = fields.iter_mut().find(|(field, _)| *field == name) {
        *existing = value;
    } else {
        fields.push((name, value));
    }
}

fn eval_unary(op: UnaryOp, value: Value) -> Result<Value, SemaErrorKind> {
    match (op, value) {
        (UnaryOp::Not, Value::Bool(value)) => Ok(Value::Bool(!value)),
        (UnaryOp::Neg, Value::Int(value)) => Ok(Value::Int(-value)),
        (UnaryOp::Neg, Value::Float(value)) => Ok(Value::Float(-value)),
        (UnaryOp::BitNot, Value::Int(value)) => Ok(Value::Int(!value)),
        _ => Err(SemaErrorKind::ConfigTypeMismatch),
    }
}

fn eval_binary(op: BinaryOp, lhs: Value, rhs: Value) -> Result<Value, SemaErrorKind> {
    match op {
        BinaryOp::Add => match (lhs, rhs) {
            (Value::Int(lhs), Value::Int(rhs)) => Ok(Value::Int(lhs + rhs)),
            (Value::Float(lhs), Value::Float(rhs)) => Ok(Value::Float(lhs + rhs)),
            (Value::Int(lhs), Value::Float(rhs)) => Ok(Value::Float(lhs as f64 + rhs)),
            (Value::Float(lhs), Value::Int(rhs)) => Ok(Value::Float(lhs + rhs as f64)),
            (Value::String(lhs), Value::String(rhs)) => {
                Ok(Value::String(Rc::from(format!("{lhs}{rhs}").as_str())))
            }
            _ => Err(SemaErrorKind::ConfigTypeMismatch),
        },
        BinaryOp::Sub => numeric(lhs, rhs, |a, b| a - b, |a, b| a - b),
        BinaryOp::Mul => numeric(lhs, rhs, |a, b| a * b, |a, b| a * b),
        BinaryOp::Div => numeric_float(lhs, rhs, |a, b| a / b),
        BinaryOp::IntDiv => match (lhs, rhs) {
            (Value::Int(lhs), Value::Int(rhs)) => Ok(Value::Int(lhs / rhs)),
            _ => Err(SemaErrorKind::ConfigTypeMismatch),
        },
        BinaryOp::Rem => match (lhs, rhs) {
            (Value::Int(lhs), Value::Int(rhs)) => Ok(Value::Int(lhs % rhs)),
            _ => Err(SemaErrorKind::ConfigTypeMismatch),
        },
        BinaryOp::Pow => numeric_float(lhs, rhs, |a, b| a.powf(b)),
        BinaryOp::Eq => Ok(Value::Bool(lhs == rhs)),
        BinaryOp::NotEq => Ok(Value::Bool(lhs != rhs)),
        BinaryOp::Lt => compare(lhs, rhs, |ord| ord < 0),
        BinaryOp::LtEq => compare(lhs, rhs, |ord| ord <= 0),
        BinaryOp::Gt => compare(lhs, rhs, |ord| ord > 0),
        BinaryOp::GtEq => compare(lhs, rhs, |ord| ord >= 0),
        BinaryOp::And => match (lhs, rhs) {
            (Value::Bool(lhs), Value::Bool(rhs)) => Ok(Value::Bool(lhs && rhs)),
            _ => Err(SemaErrorKind::ConfigTypeMismatch),
        },
        BinaryOp::Or => match (lhs, rhs) {
            (Value::Bool(lhs), Value::Bool(rhs)) => Ok(Value::Bool(lhs || rhs)),
            _ => Err(SemaErrorKind::ConfigTypeMismatch),
        },
        BinaryOp::BitAnd => int_binary(lhs, rhs, |a, b| a & b),
        BinaryOp::BitOr => int_binary(lhs, rhs, |a, b| a | b),
        BinaryOp::BitXor => int_binary(lhs, rhs, |a, b| a ^ b),
        BinaryOp::Shl => int_binary(lhs, rhs, |a, b| a << b),
        BinaryOp::Shr => int_binary(lhs, rhs, |a, b| a >> b),
        BinaryOp::In | BinaryOp::NotIn | BinaryOp::NullCoalesce => {
            Err(SemaErrorKind::ConfigNonConstant)
        }
    }
}

fn eval_index(obj: Value, index: Value) -> Result<Value, SemaErrorKind> {
    match (obj, index) {
        (Value::Array(values), Value::Int(index)) => {
            Ok(values.get(index as usize).cloned().unwrap_or(Value::Null))
        }
        (Value::Dict(entries), key) => Ok(entries
            .iter()
            .find_map(|(entry_key, value)| (*entry_key == key).then_some(value.clone()))
            .unwrap_or(Value::Null)),
        (Value::Null, _) => Ok(Value::Null),
        _ => Err(SemaErrorKind::ConfigTypeMismatch),
    }
}

fn numeric(
    lhs: Value,
    rhs: Value,
    int_op: fn(i64, i64) -> i64,
    float_op: fn(f64, f64) -> f64,
) -> Result<Value, SemaErrorKind> {
    match (lhs, rhs) {
        (Value::Int(lhs), Value::Int(rhs)) => Ok(Value::Int(int_op(lhs, rhs))),
        (Value::Float(lhs), Value::Float(rhs)) => Ok(Value::Float(float_op(lhs, rhs))),
        (Value::Int(lhs), Value::Float(rhs)) => Ok(Value::Float(float_op(lhs as f64, rhs))),
        (Value::Float(lhs), Value::Int(rhs)) => Ok(Value::Float(float_op(lhs, rhs as f64))),
        _ => Err(SemaErrorKind::ConfigTypeMismatch),
    }
}

fn numeric_float(lhs: Value, rhs: Value, op: fn(f64, f64) -> f64) -> Result<Value, SemaErrorKind> {
    match (lhs, rhs) {
        (Value::Int(lhs), Value::Int(rhs)) => Ok(Value::Float(op(lhs as f64, rhs as f64))),
        (Value::Float(lhs), Value::Float(rhs)) => Ok(Value::Float(op(lhs, rhs))),
        (Value::Int(lhs), Value::Float(rhs)) => Ok(Value::Float(op(lhs as f64, rhs))),
        (Value::Float(lhs), Value::Int(rhs)) => Ok(Value::Float(op(lhs, rhs as f64))),
        _ => Err(SemaErrorKind::ConfigTypeMismatch),
    }
}

fn int_binary(lhs: Value, rhs: Value, op: fn(i64, i64) -> i64) -> Result<Value, SemaErrorKind> {
    match (lhs, rhs) {
        (Value::Int(lhs), Value::Int(rhs)) => Ok(Value::Int(op(lhs, rhs))),
        _ => Err(SemaErrorKind::ConfigTypeMismatch),
    }
}

fn compare(lhs: Value, rhs: Value, pred: fn(i8) -> bool) -> Result<Value, SemaErrorKind> {
    let ord = match (lhs, rhs) {
        (Value::Int(lhs), Value::Int(rhs)) => lhs.cmp(&rhs) as i8,
        (Value::Float(lhs), Value::Float(rhs)) => lhs
            .partial_cmp(&rhs)
            .map(|ord| ord as i8)
            .ok_or(SemaErrorKind::ConfigTypeMismatch)?,
        (Value::String(lhs), Value::String(rhs)) => lhs.as_ref().cmp(rhs.as_ref()) as i8,
        _ => return Err(SemaErrorKind::ConfigTypeMismatch),
    };
    Ok(Value::Bool(pred(ord)))
}

fn value_matches_ty(module: &HirModule, value: &Value, ty: &Ty) -> bool {
    match ty {
        Ty::Any => true,
        Ty::Int => matches!(value, Value::Int(_)),
        Ty::Float => matches!(value, Value::Float(_)),
        Ty::Bool => matches!(value, Value::Bool(_)),
        Ty::String => matches!(value, Value::String(_)),
        Ty::Null => matches!(value, Value::Null),
        Ty::Array(element_ty) => match value {
            Value::Array(values) => values
                .iter()
                .all(|value| value_matches_ty(module, value, element_ty)),
            _ => false,
        },
        Ty::Dict(key_ty, value_ty) => match value {
            Value::Dict(entries) => entries.iter().all(|(key, value)| {
                value_matches_ty(module, key, key_ty) && value_matches_ty(module, value, value_ty)
            }),
            _ => false,
        },
        Ty::Class(class_id) => match value {
            Value::Object { fields, .. } => module.class(*class_id).is_some_and(|class| {
                class.fields.iter().all(|field| {
                    fields
                        .iter()
                        .find_map(|(name, value)| (name == &field.name).then_some(value))
                        .map_or(field.default.is_some(), |value| {
                            value_matches_ty(module, value, &field.ty)
                        })
                }) && fields
                    .iter()
                    .all(|(name, _)| class.fields.iter().any(|field| field.name == *name))
            }),
            _ => false,
        },
        Ty::Enum(enum_id) => {
            matches!(value, Value::EnumVariant(value_enum, _) if value_enum == enum_id)
        }
        Ty::Function => matches!(value, Value::Closure(_)),
        Ty::FunctionSig { .. } => matches!(value, Value::Closure(_)),
        Ty::Iterator => matches!(value, Value::Range { .. }),
        Ty::Error => true,
    }
}
