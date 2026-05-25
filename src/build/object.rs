use super::{BuildCtx, ObjectEvalState, ObjectFieldPlan, TypeInfo};
use crate::ast::{Expr, ExprKind, ObjectField, TypeName, TypeRef};
use crate::container::ModuleId;
use crate::error::{BuildError, BuildErrorKind};
use crate::value::{CfcNominalType, CfcValue, CfcValueRef};
use std::collections::{BTreeMap, HashMap, HashSet};

impl BuildCtx<'_> {
    pub(super) fn eval_object(
        &mut self,
        module: &ModuleId,
        fields: &[ObjectField],
        type_info: &TypeInfo,
        parent_locals: Option<&BTreeMap<String, CfcValueRef>>,
    ) -> Option<CfcValueRef> {
        let mut field_map = HashMap::new();
        let mut has_error = false;
        for field in fields {
            if field_map.insert(field.name.as_str(), field).is_some() {
                has_error = true;
                self.errors.push(BuildError::new(
                    BuildErrorKind::DuplicateField,
                    format!("duplicate field `{}`", field.name),
                    Some(field.span),
                ));
            }
        }
        let mut plans = HashMap::new();
        for field in &type_info.def.fields {
            if let Some(expr_field) = field_map.remove(field.name.as_str()) {
                plans.insert(
                    field.name.clone(),
                    ObjectFieldPlan {
                        ty: field.ty.clone(),
                        expr: expr_field.value.clone(),
                    },
                );
            } else if let Some(default) = &field.default {
                plans.insert(
                    field.name.clone(),
                    ObjectFieldPlan {
                        ty: field.ty.clone(),
                        expr: default.clone(),
                    },
                );
            } else {
                has_error = true;
                self.errors.push(BuildError::new(
                    BuildErrorKind::MissingRequiredField,
                    format!("missing required field `{}`", field.name),
                    Some(field.span),
                ));
            }
        }
        let has_extra_fields = !field_map.is_empty();
        for (_, extra) in field_map {
            has_error = true;
            self.errors.push(BuildError::new(
                BuildErrorKind::ExtraField,
                format!("unknown field `{}`", extra.name),
                Some(extra.span),
            ));
        }

        let mut state = ObjectEvalState {
            plans,
            values: BTreeMap::new(),
            visiting: HashSet::new(),
            parent_locals: parent_locals.cloned().unwrap_or_default(),
        };
        for field in &type_info.def.fields {
            if !state.plans.contains_key(&field.name) {
                continue;
            }
            if self
                .eval_object_field(module, &type_info.module, &mut state, &field.name)
                .is_none()
            {
                has_error = true;
            }
        }

        if !has_error && state.values.len() == type_info.def.fields.len() && !has_extra_fields {
            Some(CfcValueRef::new(CfcValue::Object {
                type_name: Some(CfcNominalType {
                    module: type_info.module.clone(),
                    name: type_info.def.name.clone(),
                }),
                fields: state.values,
            }))
        } else {
            None
        }
    }

    pub(super) fn eval_object_field(
        &mut self,
        module: &ModuleId,
        default_module: &ModuleId,
        state: &mut ObjectEvalState,
        name: &str,
    ) -> Option<CfcValueRef> {
        if let Some(value) = state.values.get(name) {
            return Some(value.clone());
        }
        if !state.visiting.insert(name.to_string()) {
            self.errors.push(BuildError::new(
                BuildErrorKind::Cycle,
                format!("cyclic field reference involving `{name}`"),
                None,
            ));
            return None;
        }
        let Some(plan) = state.plans.get(name).cloned() else {
            state.visiting.remove(name);
            return state.parent_locals.get(name).cloned();
        };
        let value = self.eval_expr_in_object_state(
            module,
            default_module,
            &plan.expr,
            Some(&plan.ty),
            state,
        );
        state.visiting.remove(name);
        if let Some(value) = value {
            state.values.insert(name.to_string(), value.clone());
            Some(value)
        } else {
            None
        }
    }

    #[allow(clippy::too_many_lines)]
    fn eval_expr_in_object_state(
        &mut self,
        module: &ModuleId,
        default_module: &ModuleId,
        expr: &Expr,
        expected: Option<&TypeRef>,
        state: &mut ObjectEvalState,
    ) -> Option<CfcValueRef> {
        if matches!(expected, Some(TypeRef::Any)) {
            return self.eval_any_in_object_state(module, default_module, expr, state);
        }
        match expected {
            Some(TypeRef::Int) => match expr.kind {
                ExprKind::Int(value) => Some(CfcValueRef::new(CfcValue::Int(value))),
                ExprKind::Name(ref name) => self
                    .resolve_name_in_object_state(module, default_module, name, expr.span, state)
                    .and_then(|value| self.ensure_basic_value(expr, value, "int")),
                ExprKind::Qualified(ref parts) => self
                    .eval_qualified_in_object_state(module, default_module, expr, parts, state)
                    .and_then(|value| self.ensure_basic_value(expr, value, "int")),
                ExprKind::Path { .. } => self
                    .eval_path_in_object_state(module, default_module, expr, state)
                    .and_then(|value| self.ensure_basic_value(expr, value, "int")),
                _ => self.type_error(expr, "int"),
            },
            Some(TypeRef::Float) => match expr.kind {
                ExprKind::Float(value) => Some(CfcValueRef::new(CfcValue::Float(value))),
                ExprKind::Name(ref name) => self
                    .resolve_name_in_object_state(module, default_module, name, expr.span, state)
                    .and_then(|value| self.ensure_basic_value(expr, value, "float")),
                ExprKind::Qualified(ref parts) => self
                    .eval_qualified_in_object_state(module, default_module, expr, parts, state)
                    .and_then(|value| self.ensure_basic_value(expr, value, "float")),
                ExprKind::Path { .. } => self
                    .eval_path_in_object_state(module, default_module, expr, state)
                    .and_then(|value| self.ensure_basic_value(expr, value, "float")),
                _ => self.type_error(expr, "float"),
            },
            Some(TypeRef::Bool) => match expr.kind {
                ExprKind::Bool(value) => Some(CfcValueRef::new(CfcValue::Bool(value))),
                ExprKind::Name(ref name) => self
                    .resolve_name_in_object_state(module, default_module, name, expr.span, state)
                    .and_then(|value| self.ensure_basic_value(expr, value, "bool")),
                ExprKind::Qualified(ref parts) => self
                    .eval_qualified_in_object_state(module, default_module, expr, parts, state)
                    .and_then(|value| self.ensure_basic_value(expr, value, "bool")),
                ExprKind::Path { .. } => self
                    .eval_path_in_object_state(module, default_module, expr, state)
                    .and_then(|value| self.ensure_basic_value(expr, value, "bool")),
                _ => self.type_error(expr, "bool"),
            },
            Some(TypeRef::String) => match &expr.kind {
                ExprKind::String(value) => Some(CfcValueRef::new(CfcValue::String(value.clone()))),
                ExprKind::Name(name) => self
                    .resolve_name_in_object_state(module, default_module, name, expr.span, state)
                    .and_then(|value| self.ensure_basic_value(expr, value, "string")),
                ExprKind::Qualified(parts) => self
                    .eval_qualified_in_object_state(module, default_module, expr, parts, state)
                    .and_then(|value| self.ensure_basic_value(expr, value, "string")),
                ExprKind::Path { .. } => self
                    .eval_path_in_object_state(module, default_module, expr, state)
                    .and_then(|value| self.ensure_basic_value(expr, value, "string")),
                _ => self.type_error(expr, "string"),
            },
            Some(TypeRef::Array(inner)) => match &expr.kind {
                ExprKind::Array(items) => {
                    let mut out = Vec::new();
                    for item in items {
                        out.push(self.eval_expr_in_object_state(
                            module,
                            default_module,
                            item,
                            Some(inner),
                            state,
                        )?);
                    }
                    Some(CfcValueRef::new(CfcValue::Array(out)))
                }
                ExprKind::Name(name) => self
                    .resolve_name_in_object_state(module, default_module, name, expr.span, state)
                    .and_then(|value| self.ensure_array_value(expr, &value)),
                ExprKind::Qualified(parts) => self
                    .eval_qualified_in_object_state(module, default_module, expr, parts, state)
                    .and_then(|value| self.ensure_array_value(expr, &value)),
                ExprKind::Path { .. } => self
                    .eval_path_in_object_state(module, default_module, expr, state)
                    .and_then(|value| self.ensure_array_value(expr, &value)),
                _ => self.type_error(expr, "array"),
            },
            Some(TypeRef::Dict(key_ty, value_ty)) => match &expr.kind {
                ExprKind::Dict(entries) => {
                    let mut out = Vec::new();
                    for (key, value) in entries {
                        out.push((
                            self.eval_expr_in_object_state(
                                module,
                                default_module,
                                key,
                                Some(key_ty),
                                state,
                            )?,
                            self.eval_expr_in_object_state(
                                module,
                                default_module,
                                value,
                                Some(value_ty),
                                state,
                            )?,
                        ));
                    }
                    Some(CfcValueRef::new(CfcValue::Dict(out)))
                }
                ExprKind::Name(name) => self
                    .resolve_name_in_object_state(module, default_module, name, expr.span, state)
                    .and_then(|value| self.ensure_dict_value(expr, &value)),
                ExprKind::Qualified(parts) => self
                    .eval_qualified_in_object_state(module, default_module, expr, parts, state)
                    .and_then(|value| self.ensure_dict_value(expr, &value)),
                ExprKind::Path { .. } => self
                    .eval_path_in_object_state(module, default_module, expr, state)
                    .and_then(|value| self.ensure_dict_value(expr, &value)),
                _ => self.type_error(expr, "dict"),
            },
            Some(TypeRef::Named(name)) => {
                self.eval_named_in_object_state(module, default_module, expr, name, state)
            }
            Some(TypeRef::Any) => {
                self.eval_any_in_object_state(module, default_module, expr, state)
            }
            None => self.eval_untyped_in_object_state(module, default_module, expr, state),
        }
    }

    fn eval_named_in_object_state(
        &mut self,
        module: &ModuleId,
        default_module: &ModuleId,
        expr: &Expr,
        name: &TypeName,
        state: &mut ObjectEvalState,
    ) -> Option<CfcValueRef> {
        let (target_module, target_name) =
            self.resolve_type_name(default_module, name, expr.span)?;
        if self
            .symbols
            .enums
            .contains_key(&(target_module.clone(), target_name.clone()))
        {
            return self.eval_enum_value(module, expr, &target_module, &target_name);
        }
        let Some(type_info) = self
            .symbols
            .types
            .get(&(target_module.clone(), target_name.clone()))
            .cloned()
        else {
            self.errors.push(BuildError::new(
                BuildErrorKind::UnknownType,
                format!("unknown type `{target_name}`"),
                Some(expr.span),
            ));
            return None;
        };
        match &expr.kind {
            ExprKind::Object(fields) => {
                self.eval_object_with_parent(module, fields, &type_info, state)
            }
            ExprKind::Name(name) => {
                let value = self.resolve_name_in_object_state(
                    module,
                    default_module,
                    name,
                    expr.span,
                    state,
                )?;
                self.ensure_named_value(expr, &value, &type_info)
            }
            ExprKind::Qualified(parts) if parts.len() == 2 => {
                let value = self.eval_qualified_in_object_state(
                    module,
                    default_module,
                    expr,
                    parts,
                    state,
                )?;
                self.ensure_named_value(expr, &value, &type_info)
            }
            ExprKind::Path { .. } => {
                let value = self.eval_path_in_object_state(module, default_module, expr, state)?;
                self.ensure_named_value(expr, &value, &type_info)
            }
            _ => self.type_error(expr, &target_name),
        }
    }

    fn eval_object_with_parent(
        &mut self,
        module: &ModuleId,
        fields: &[ObjectField],
        type_info: &TypeInfo,
        parent: &ObjectEvalState,
    ) -> Option<CfcValueRef> {
        let mut parent_locals = parent.parent_locals.clone();
        parent_locals.extend(parent.values.clone());
        self.eval_object(module, fields, type_info, Some(&parent_locals))
    }
}
