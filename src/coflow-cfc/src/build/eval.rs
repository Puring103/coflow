use super::{BuildCtx, ObjectEvalState, TypeInfo};
use crate::ast::{DataDef, Expr, ExprKind, TypeName, TypeRef};
use crate::build::support::{build_error, format_nominal, value_signature};
use crate::container::{CfcModuleResult, ModuleId};
use crate::error::{BuildError, BuildErrorKind};
use crate::value::{CfcNominalType, CfcValue, CfcValueRef};
use std::collections::BTreeMap;

impl BuildCtx<'_> {
    pub(super) fn build_values(&mut self) {
        let keys: Vec<_> = self.symbols.data.keys().cloned().collect();
        for (module, name) in keys {
            if self.eval_data(&module, &name).is_none() {
                self.errors.push(build_error(format!(
                    "failed to build data node `{module}.{name}`"
                )));
            }
        }

        for module_id in &self.module_ids {
            let values = self
                .graph
                .memo
                .iter()
                .filter(|((module, _), _)| module == module_id)
                .filter(|(key, _)| !self.graph.failed.contains(key))
                .map(|((_, name), value)| (name.clone(), value.clone()))
                .collect();
            self.graph
                .results
                .insert(module_id.clone(), CfcModuleResult::new(values));
        }
    }

    pub(super) fn eval_data(&mut self, module: &ModuleId, name: &str) -> Option<CfcValueRef> {
        let key = (module.clone(), name.to_string());
        if let Some(value) = self.graph.memo.get(&key) {
            return (!self.graph.failed.contains(&key)).then(|| value.clone());
        }
        let Some(def) = self.symbols.data.get(&key).cloned() else {
            self.errors
                .push(build_error(format!("unknown data node `{module}.{name}`")));
            return None;
        };

        let placeholder = if data_has_identity(&def) {
            let value = CfcValueRef::pending(identity_placeholder(&def));
            self.graph.memo.insert(key.clone(), value.clone());
            Some(value)
        } else {
            None
        };

        if !self.graph.visiting.insert(key.clone()) {
            return self.graph.memo.get(&key).cloned();
        }
        let value = self.eval_expr(module, &def.value, def.ty.as_ref());
        self.graph.visiting.remove(&key);
        if let Some(value) = value {
            if let Some(placeholder) = placeholder {
                placeholder.replace(value.borrow().clone());
                Some(placeholder)
            } else {
                self.graph.memo.insert(key, value.clone());
                Some(value)
            }
        } else {
            if placeholder.is_some() {
                self.graph.memo.remove(&key);
            }
            self.graph.failed.insert(key);
            None
        }
    }

    fn eval_expr(
        &mut self,
        module: &ModuleId,
        expr: &Expr,
        expected: Option<&TypeRef>,
    ) -> Option<CfcValueRef> {
        self.eval_expr_with_locals(module, expr, expected, None)
    }

    fn eval_expr_with_locals(
        &mut self,
        module: &ModuleId,
        expr: &Expr,
        expected: Option<&TypeRef>,
        locals: Option<&BTreeMap<String, CfcValueRef>>,
    ) -> Option<CfcValueRef> {
        if matches!(expected, Some(TypeRef::Any)) {
            return self.eval_any_with_locals(module, expr, locals);
        }
        match expected {
            Some(TypeRef::Int) => match expr.kind {
                ExprKind::Int(value) => Some(CfcValueRef::new(CfcValue::Int(value))),
                ExprKind::Name(ref name) => self
                    .resolve_name_value(module, name, expr.span, locals)
                    .and_then(|value| self.ensure_basic_value(expr, value, "int")),
                ExprKind::Qualified(ref parts) => self
                    .eval_qualified_as_path_or_data(module, expr, parts, locals)
                    .and_then(|value| self.ensure_basic_value(expr, value, "int")),
                ExprKind::Path { .. } => self
                    .eval_path_expr(module, expr, locals)
                    .and_then(|value| self.ensure_basic_value(expr, value, "int")),
                _ => self.type_error(expr, "int"),
            },
            Some(TypeRef::Float) => match expr.kind {
                ExprKind::Float(value) => Some(CfcValueRef::new(CfcValue::Float(value))),
                ExprKind::Name(ref name) => self
                    .resolve_name_value(module, name, expr.span, locals)
                    .and_then(|value| self.ensure_basic_value(expr, value, "float")),
                ExprKind::Qualified(ref parts) => self
                    .eval_qualified_as_path_or_data(module, expr, parts, locals)
                    .and_then(|value| self.ensure_basic_value(expr, value, "float")),
                ExprKind::Path { .. } => self
                    .eval_path_expr(module, expr, locals)
                    .and_then(|value| self.ensure_basic_value(expr, value, "float")),
                _ => self.type_error(expr, "float"),
            },
            Some(TypeRef::Bool) => match expr.kind {
                ExprKind::Bool(value) => Some(CfcValueRef::new(CfcValue::Bool(value))),
                ExprKind::Name(ref name) => self
                    .resolve_name_value(module, name, expr.span, locals)
                    .and_then(|value| self.ensure_basic_value(expr, value, "bool")),
                ExprKind::Qualified(ref parts) => self
                    .eval_qualified_as_path_or_data(module, expr, parts, locals)
                    .and_then(|value| self.ensure_basic_value(expr, value, "bool")),
                ExprKind::Path { .. } => self
                    .eval_path_expr(module, expr, locals)
                    .and_then(|value| self.ensure_basic_value(expr, value, "bool")),
                _ => self.type_error(expr, "bool"),
            },
            Some(TypeRef::String) => match &expr.kind {
                ExprKind::String(value) => Some(CfcValueRef::new(CfcValue::String(value.clone()))),
                ExprKind::Name(name) => self
                    .resolve_name_value(module, name, expr.span, locals)
                    .and_then(|value| self.ensure_basic_value(expr, value, "string")),
                ExprKind::Qualified(parts) => self
                    .eval_qualified_as_path_or_data(module, expr, parts, locals)
                    .and_then(|value| self.ensure_basic_value(expr, value, "string")),
                ExprKind::Path { .. } => self
                    .eval_path_expr(module, expr, locals)
                    .and_then(|value| self.ensure_basic_value(expr, value, "string")),
                _ => self.type_error(expr, "string"),
            },
            Some(TypeRef::StringLiteral(expected)) => match &expr.kind {
                ExprKind::String(value) if value == expected => {
                    Some(CfcValueRef::new(CfcValue::String(value.clone())))
                }
                ExprKind::Name(name) => self
                    .resolve_name_value(module, name, expr.span, locals)
                    .and_then(|value| self.ensure_string_literal_value(expr, &value, expected)),
                ExprKind::Qualified(parts) => self
                    .eval_qualified_as_path_or_data(module, expr, parts, locals)
                    .and_then(|value| self.ensure_string_literal_value(expr, &value, expected)),
                ExprKind::Path { .. } => self
                    .eval_path_expr(module, expr, locals)
                    .and_then(|value| self.ensure_string_literal_value(expr, &value, expected)),
                _ => self.type_error(expr, &format!("{expected:?}")),
            },
            Some(TypeRef::Array(inner)) => self.eval_array(module, expr, inner, locals),
            Some(TypeRef::Dict(key, value)) => self.eval_dict(module, expr, key, value, locals),
            Some(TypeRef::Union(items)) => self.eval_union_expected(module, expr, items, locals),
            Some(TypeRef::Named(name)) => self.eval_named_expected(module, expr, name, locals),
            Some(TypeRef::Any) => self.eval_any_with_locals(module, expr, locals),
            None => self.eval_untyped_with_locals(module, expr, locals),
        }
    }

    fn eval_named_expected(
        &mut self,
        module: &ModuleId,
        expr: &Expr,
        name: &TypeName,
        locals: Option<&BTreeMap<String, CfcValueRef>>,
    ) -> Option<CfcValueRef> {
        let (target_module, target_name) = self.resolve_type_name(module, name, expr.span)?;
        if self
            .symbols
            .enums
            .contains_key(&(target_module.clone(), target_name.clone()))
        {
            return self.eval_enum_value(module, expr, &target_module, &target_name);
        }
        if self
            .symbols
            .unions
            .contains_key(&(target_module.clone(), target_name.clone()))
        {
            return self.eval_named_union_expected(
                module,
                expr,
                &target_module,
                &target_name,
                locals,
            );
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
            ExprKind::Object(fields) => self.eval_object(module, fields, &type_info, locals),
            ExprKind::Name(name) => {
                let value = Self::resolve_local(name, locals)
                    .or_else(|| self.eval_data_name(module, name, expr.span))?;
                self.ensure_named_value(expr, &value, &type_info)
            }
            ExprKind::Qualified(parts) if parts.len() == 2 => {
                let value = self.eval_qualified_as_path_or_data(module, expr, parts, locals)?;
                self.ensure_named_value(expr, &value, &type_info)
            }
            ExprKind::Path { .. } => {
                let value = self.eval_path_expr(module, expr, locals)?;
                self.ensure_named_value(expr, &value, &type_info)
            }
            _ => self.type_error(expr, &target_name),
        }
    }

    fn eval_named_union_expected(
        &mut self,
        module: &ModuleId,
        expr: &Expr,
        union_module: &ModuleId,
        union_name: &str,
        locals: Option<&BTreeMap<String, CfcValueRef>>,
    ) -> Option<CfcValueRef> {
        let Some(union) = self
            .symbols
            .unions
            .get(&(union_module.clone(), union_name.to_string()))
            .cloned()
        else {
            self.errors.push(BuildError::new(
                BuildErrorKind::UnknownType,
                format!("unknown union `{union_name}`"),
                Some(expr.span),
            ));
            return None;
        };
        let branch_refs = union
            .branches
            .iter()
            .map(|branch| TypeRef::Named(branch.clone()))
            .collect::<Vec<_>>();
        self.eval_union_expected(module, expr, &branch_refs, locals)
    }

    fn eval_union_expected(
        &mut self,
        module: &ModuleId,
        expr: &Expr,
        branches: &[TypeRef],
        locals: Option<&BTreeMap<String, CfcValueRef>>,
    ) -> Option<CfcValueRef> {
        match &expr.kind {
            ExprKind::Object(fields) => {
                let Some(kind) = object_string_field(fields, "kind") else {
                    self.errors.push(BuildError::new(
                        BuildErrorKind::TypeMismatch,
                        "union object must include string field `kind`",
                        Some(expr.span),
                    ));
                    return None;
                };
                let Some(branch) =
                    self.select_union_branch_by_kind(module, branches, kind, expr.span)
                else {
                    return None;
                };
                self.eval_expr_with_locals(module, expr, Some(&branch), locals)
            }
            ExprKind::Name(name) => {
                let value = Self::resolve_local(name, locals)
                    .or_else(|| self.eval_data_name(module, name, expr.span))?;
                self.ensure_union_value(expr, &value, module, branches)
            }
            ExprKind::Qualified(parts) => {
                let value = self.eval_qualified_as_path_or_data(module, expr, parts, locals)?;
                self.ensure_union_value(expr, &value, module, branches)
            }
            ExprKind::Path { .. } => {
                let value = self.eval_path_expr(module, expr, locals)?;
                self.ensure_union_value(expr, &value, module, branches)
            }
            _ => self.type_error(expr, "union"),
        }
    }

    pub(super) fn select_union_branch_by_kind(
        &mut self,
        module: &ModuleId,
        branches: &[TypeRef],
        kind: &str,
        span: crate::span::Span,
    ) -> Option<TypeRef> {
        let mut matches = Vec::new();
        for branch in branches {
            let TypeRef::Named(name) = branch else {
                continue;
            };
            let Some((target_module, target_name)) = self.resolve_type_name(module, name, span)
            else {
                continue;
            };
            let Some(type_info) = self
                .symbols
                .types
                .get(&(target_module.clone(), target_name.clone()))
            else {
                continue;
            };
            if type_discriminator(type_info).is_some_and(|value| value == kind) {
                matches.push(branch.clone());
            }
        }
        match matches.len() {
            1 => matches.pop(),
            0 => {
                self.errors.push(BuildError::new(
                    BuildErrorKind::TypeMismatch,
                    format!("no union branch matches kind `{kind}`"),
                    Some(span),
                ));
                None
            }
            _ => {
                self.errors.push(BuildError::new(
                    BuildErrorKind::TypeMismatch,
                    format!("multiple union branches match kind `{kind}`"),
                    Some(span),
                ));
                None
            }
        }
    }

    pub(super) fn ensure_union_value(
        &mut self,
        expr: &Expr,
        value: &CfcValueRef,
        module: &ModuleId,
        branches: &[TypeRef],
    ) -> Option<CfcValueRef> {
        if value.is_pending() {
            return Some(value.clone());
        }
        let borrowed = value.borrow();
        let CfcValue::Object {
            type_name: Some(actual),
            ..
        } = &*borrowed
        else {
            self.errors.push(BuildError::new(
                BuildErrorKind::TypeMismatch,
                format!("expected union, found {}", borrowed.type_name()),
                Some(expr.span),
            ));
            return None;
        };
        if branches.iter().any(|branch| {
            let TypeRef::Named(name) = branch else {
                return false;
            };
            self.resolve_type_name(module, name, expr.span).is_some_and(
                |(branch_module, branch_name)| {
                    actual.module == branch_module && actual.name == branch_name
                },
            )
        }) {
            Some(value.clone())
        } else {
            self.errors.push(BuildError::new(
                BuildErrorKind::TypeMismatch,
                format!("expected union, found `{}`", format_nominal(actual)),
                Some(expr.span),
            ));
            None
        }
    }

    pub(super) fn ensure_named_value(
        &mut self,
        expr: &Expr,
        value: &CfcValueRef,
        type_info: &TypeInfo,
    ) -> Option<CfcValueRef> {
        let expected = CfcNominalType {
            module: type_info.module.clone(),
            name: type_info.def.name.clone(),
        };
        if value.is_pending() {
            return Some(value.clone());
        }
        let borrowed = value.borrow();
        match &*borrowed {
            CfcValue::Object {
                type_name: Some(actual),
                ..
            } if actual == &expected => Some(value.clone()),
            CfcValue::Object {
                type_name: None, ..
            } => {
                self.errors.push(BuildError::new(
                    BuildErrorKind::TypeMismatch,
                    format!(
                        "expected `{}`, found untyped object",
                        format_nominal(&expected)
                    ),
                    Some(expr.span),
                ));
                None
            }
            CfcValue::Object {
                type_name: Some(actual),
                ..
            } => {
                self.errors.push(BuildError::new(
                    BuildErrorKind::TypeMismatch,
                    format!(
                        "expected `{}`, found `{}`",
                        format_nominal(&expected),
                        format_nominal(actual)
                    ),
                    Some(expr.span),
                ));
                None
            }
            other => {
                self.errors.push(BuildError::new(
                    BuildErrorKind::TypeMismatch,
                    format!(
                        "expected `{}`, found `{}`",
                        format_nominal(&expected),
                        other.type_name()
                    ),
                    Some(expr.span),
                ));
                None
            }
        }
    }

    pub(super) fn ensure_string_literal_value(
        &mut self,
        expr: &Expr,
        value: &CfcValueRef,
        expected: &str,
    ) -> Option<CfcValueRef> {
        if matches!(&*value.borrow(), CfcValue::String(actual) if actual == expected) {
            Some(value.clone())
        } else {
            self.type_error(expr, &format!("{expected:?}"))
        }
    }

    fn eval_array(
        &mut self,
        module: &ModuleId,
        expr: &Expr,
        inner: &TypeRef,
        locals: Option<&BTreeMap<String, CfcValueRef>>,
    ) -> Option<CfcValueRef> {
        match &expr.kind {
            ExprKind::Array(items) => {
                let mut out = Vec::new();
                for item in items {
                    out.push(self.eval_expr_with_locals(module, item, Some(inner), locals)?);
                }
                Some(CfcValueRef::new(CfcValue::Array(out)))
            }
            ExprKind::Name(name) => {
                let value = self.resolve_name_value(module, name, expr.span, locals)?;
                self.ensure_array_value(expr, &value)
            }
            ExprKind::Qualified(parts) => {
                let value = self.eval_qualified_as_path_or_data(module, expr, parts, locals)?;
                self.ensure_array_value(expr, &value)
            }
            ExprKind::Path { .. } => {
                let value = self.eval_path_expr(module, expr, locals)?;
                self.ensure_array_value(expr, &value)
            }
            _ => self.type_error(expr, "array"),
        }
    }

    fn eval_dict(
        &mut self,
        module: &ModuleId,
        expr: &Expr,
        key_ty: &TypeRef,
        value_ty: &TypeRef,
        locals: Option<&BTreeMap<String, CfcValueRef>>,
    ) -> Option<CfcValueRef> {
        match &expr.kind {
            ExprKind::Dict(entries) => {
                let mut out = Vec::new();
                for (key, value) in entries {
                    out.push((
                        self.eval_expr_with_locals(module, key, Some(key_ty), locals)?,
                        self.eval_expr_with_locals(module, value, Some(value_ty), locals)?,
                    ));
                }
                Some(CfcValueRef::new(CfcValue::Dict(out)))
            }
            ExprKind::Name(name) => {
                let value = self.resolve_name_value(module, name, expr.span, locals)?;
                self.ensure_dict_value(expr, &value)
            }
            ExprKind::Qualified(parts) => {
                let value = self.eval_qualified_as_path_or_data(module, expr, parts, locals)?;
                self.ensure_dict_value(expr, &value)
            }
            ExprKind::Path { .. } => {
                let value = self.eval_path_expr(module, expr, locals)?;
                self.ensure_dict_value(expr, &value)
            }
            _ => self.type_error(expr, "dict"),
        }
    }

    pub(super) fn ensure_array_value(
        &mut self,
        expr: &Expr,
        value: &CfcValueRef,
    ) -> Option<CfcValueRef> {
        if value.is_pending() || matches!(&*value.borrow(), CfcValue::Array(_)) {
            Some(value.clone())
        } else {
            self.type_error(expr, "array")
        }
    }

    pub(super) fn ensure_dict_value(
        &mut self,
        expr: &Expr,
        value: &CfcValueRef,
    ) -> Option<CfcValueRef> {
        if value.is_pending() || matches!(&*value.borrow(), CfcValue::Dict(_)) {
            Some(value.clone())
        } else {
            self.type_error(expr, "dict")
        }
    }

    pub(super) fn eval_untyped_with_locals(
        &mut self,
        module: &ModuleId,
        expr: &Expr,
        locals: Option<&BTreeMap<String, CfcValueRef>>,
    ) -> Option<CfcValueRef> {
        match &expr.kind {
            ExprKind::Int(value) => Some(CfcValueRef::new(CfcValue::Int(*value))),
            ExprKind::Float(value) => Some(CfcValueRef::new(CfcValue::Float(*value))),
            ExprKind::Bool(value) => Some(CfcValueRef::new(CfcValue::Bool(*value))),
            ExprKind::String(value) => Some(CfcValueRef::new(CfcValue::String(value.clone()))),
            ExprKind::Name(name) => self.resolve_name_value(module, name, expr.span, locals),
            ExprKind::Qualified(parts) => self.eval_qualified_untyped(module, expr, parts, locals),
            ExprKind::Path { .. } => self.eval_path_expr(module, expr, locals),
            ExprKind::Object(fields) => {
                let mut out = BTreeMap::new();
                for field in fields {
                    out.insert(
                        field.name.clone(),
                        self.eval_untyped_with_locals(module, &field.value, Some(&out))?,
                    );
                }
                Some(CfcValueRef::new(CfcValue::Object {
                    type_name: None,
                    fields: out,
                }))
            }
            ExprKind::Array(items) => self.eval_untyped_array(module, expr, items, locals),
            ExprKind::Dict(entries) => self.eval_untyped_dict(module, expr, entries, locals),
        }
    }

    pub(super) fn eval_any_with_locals(
        &mut self,
        module: &ModuleId,
        expr: &Expr,
        locals: Option<&BTreeMap<String, CfcValueRef>>,
    ) -> Option<CfcValueRef> {
        match &expr.kind {
            ExprKind::Int(value) => Some(CfcValueRef::new(CfcValue::Int(*value))),
            ExprKind::Float(value) => Some(CfcValueRef::new(CfcValue::Float(*value))),
            ExprKind::Bool(value) => Some(CfcValueRef::new(CfcValue::Bool(*value))),
            ExprKind::String(value) => Some(CfcValueRef::new(CfcValue::String(value.clone()))),
            ExprKind::Name(name) => self.resolve_name_value(module, name, expr.span, locals),
            ExprKind::Qualified(parts) => self.eval_qualified_untyped(module, expr, parts, locals),
            ExprKind::Path { .. } => self.eval_path_expr(module, expr, locals),
            ExprKind::Object(fields) => {
                let mut out = BTreeMap::new();
                for field in fields {
                    out.insert(
                        field.name.clone(),
                        self.eval_any_with_locals(module, &field.value, Some(&out))?,
                    );
                }
                Some(CfcValueRef::new(CfcValue::Object {
                    type_name: None,
                    fields: out,
                }))
            }
            ExprKind::Array(items) => {
                let mut out = Vec::new();
                for item in items {
                    out.push(self.eval_any_with_locals(module, item, locals)?);
                }
                Some(CfcValueRef::new(CfcValue::Array(out)))
            }
            ExprKind::Dict(entries) => {
                let mut out = Vec::new();
                for (key, value) in entries {
                    out.push((
                        self.eval_any_with_locals(module, key, locals)?,
                        self.eval_any_with_locals(module, value, locals)?,
                    ));
                }
                Some(CfcValueRef::new(CfcValue::Dict(out)))
            }
        }
    }

    pub(super) fn eval_untyped_in_object_state(
        &mut self,
        module: &ModuleId,
        default_module: &ModuleId,
        expr: &Expr,
        state: &mut ObjectEvalState,
    ) -> Option<CfcValueRef> {
        match &expr.kind {
            ExprKind::Int(value) => Some(CfcValueRef::new(CfcValue::Int(*value))),
            ExprKind::Float(value) => Some(CfcValueRef::new(CfcValue::Float(*value))),
            ExprKind::Bool(value) => Some(CfcValueRef::new(CfcValue::Bool(*value))),
            ExprKind::String(value) => Some(CfcValueRef::new(CfcValue::String(value.clone()))),
            ExprKind::Name(name) => {
                self.resolve_name_in_object_state(module, default_module, name, expr.span, state)
            }
            ExprKind::Qualified(parts) => {
                self.eval_qualified_in_object_state(module, default_module, expr, parts, state)
            }
            ExprKind::Path { .. } => {
                self.eval_path_in_object_state(module, default_module, expr, state)
            }
            ExprKind::Object(fields) => {
                let mut out = BTreeMap::new();
                for field in fields {
                    out.insert(
                        field.name.clone(),
                        self.eval_untyped_in_object_state(
                            module,
                            default_module,
                            &field.value,
                            state,
                        )?,
                    );
                }
                Some(CfcValueRef::new(CfcValue::Object {
                    type_name: None,
                    fields: out,
                }))
            }
            ExprKind::Array(items) => {
                self.eval_untyped_array_in_object_state(module, default_module, expr, items, state)
            }
            ExprKind::Dict(entries) => {
                self.eval_untyped_dict_in_object_state(module, default_module, expr, entries, state)
            }
        }
    }

    pub(super) fn eval_any_in_object_state(
        &mut self,
        module: &ModuleId,
        default_module: &ModuleId,
        expr: &Expr,
        state: &mut ObjectEvalState,
    ) -> Option<CfcValueRef> {
        match &expr.kind {
            ExprKind::Int(value) => Some(CfcValueRef::new(CfcValue::Int(*value))),
            ExprKind::Float(value) => Some(CfcValueRef::new(CfcValue::Float(*value))),
            ExprKind::Bool(value) => Some(CfcValueRef::new(CfcValue::Bool(*value))),
            ExprKind::String(value) => Some(CfcValueRef::new(CfcValue::String(value.clone()))),
            ExprKind::Name(name) => {
                self.resolve_name_in_object_state(module, default_module, name, expr.span, state)
            }
            ExprKind::Qualified(parts) => {
                self.eval_qualified_in_object_state(module, default_module, expr, parts, state)
            }
            ExprKind::Path { .. } => {
                self.eval_path_in_object_state(module, default_module, expr, state)
            }
            ExprKind::Object(fields) => {
                let mut out = BTreeMap::new();
                for field in fields {
                    out.insert(
                        field.name.clone(),
                        self.eval_any_in_object_state(module, default_module, &field.value, state)?,
                    );
                }
                Some(CfcValueRef::new(CfcValue::Object {
                    type_name: None,
                    fields: out,
                }))
            }
            ExprKind::Array(items) => {
                let mut out = Vec::new();
                for item in items {
                    out.push(self.eval_any_in_object_state(module, default_module, item, state)?);
                }
                Some(CfcValueRef::new(CfcValue::Array(out)))
            }
            ExprKind::Dict(entries) => {
                let mut out = Vec::new();
                for (key, value) in entries {
                    out.push((
                        self.eval_any_in_object_state(module, default_module, key, state)?,
                        self.eval_any_in_object_state(module, default_module, value, state)?,
                    ));
                }
                Some(CfcValueRef::new(CfcValue::Dict(out)))
            }
        }
    }

    fn eval_untyped_array(
        &mut self,
        module: &ModuleId,
        expr: &Expr,
        items: &[Expr],
        locals: Option<&BTreeMap<String, CfcValueRef>>,
    ) -> Option<CfcValueRef> {
        if items.is_empty() {
            self.errors.push(BuildError::new(
                BuildErrorKind::Inference,
                "cannot infer type of empty array",
                Some(expr.span),
            ));
            return None;
        }
        let mut out = Vec::new();
        let mut inferred = None;
        for item in items {
            let value = self.eval_untyped_with_locals(module, item, locals)?;
            validate_array_item(self, item, &value, &mut inferred)?;
            out.push(value);
        }
        Some(CfcValueRef::new(CfcValue::Array(out)))
    }

    fn eval_untyped_array_in_object_state(
        &mut self,
        module: &ModuleId,
        default_module: &ModuleId,
        expr: &Expr,
        items: &[Expr],
        state: &mut ObjectEvalState,
    ) -> Option<CfcValueRef> {
        if items.is_empty() {
            self.errors.push(BuildError::new(
                BuildErrorKind::Inference,
                "cannot infer type of empty array",
                Some(expr.span),
            ));
            return None;
        }
        let mut out = Vec::new();
        let mut inferred = None;
        for item in items {
            let value = self.eval_untyped_in_object_state(module, default_module, item, state)?;
            validate_array_item(self, item, &value, &mut inferred)?;
            out.push(value);
        }
        Some(CfcValueRef::new(CfcValue::Array(out)))
    }

    fn eval_untyped_dict(
        &mut self,
        module: &ModuleId,
        expr: &Expr,
        entries: &[(Expr, Expr)],
        locals: Option<&BTreeMap<String, CfcValueRef>>,
    ) -> Option<CfcValueRef> {
        if entries.is_empty() {
            self.errors.push(BuildError::new(
                BuildErrorKind::Inference,
                "cannot infer type of empty dict",
                Some(expr.span),
            ));
            return None;
        }
        let mut out = Vec::new();
        let mut inferred_key = None;
        let mut inferred_value = None;
        for (key, value) in entries {
            let key_value = self.eval_untyped_with_locals(module, key, locals)?;
            let value_value = self.eval_untyped_with_locals(module, value, locals)?;
            validate_dict_entry(
                self,
                key,
                value,
                &key_value,
                &value_value,
                &mut inferred_key,
                &mut inferred_value,
            )?;
            out.push((key_value, value_value));
        }
        Some(CfcValueRef::new(CfcValue::Dict(out)))
    }

    fn eval_untyped_dict_in_object_state(
        &mut self,
        module: &ModuleId,
        default_module: &ModuleId,
        expr: &Expr,
        entries: &[(Expr, Expr)],
        state: &mut ObjectEvalState,
    ) -> Option<CfcValueRef> {
        if entries.is_empty() {
            self.errors.push(BuildError::new(
                BuildErrorKind::Inference,
                "cannot infer type of empty dict",
                Some(expr.span),
            ));
            return None;
        }
        let mut out = Vec::new();
        let mut inferred_key = None;
        let mut inferred_value = None;
        for (key, value) in entries {
            let key_value =
                self.eval_untyped_in_object_state(module, default_module, key, state)?;
            let value_value =
                self.eval_untyped_in_object_state(module, default_module, value, state)?;
            validate_dict_entry(
                self,
                key,
                value,
                &key_value,
                &value_value,
                &mut inferred_key,
                &mut inferred_value,
            )?;
            out.push((key_value, value_value));
        }
        Some(CfcValueRef::new(CfcValue::Dict(out)))
    }
}

fn data_has_identity(def: &DataDef) -> bool {
    matches!(
        def.value.kind,
        ExprKind::Object(_) | ExprKind::Array(_) | ExprKind::Dict(_)
    )
}

fn identity_placeholder(def: &DataDef) -> CfcValue {
    match def.value.kind {
        ExprKind::Array(_) => CfcValue::Array(Vec::new()),
        ExprKind::Dict(_) => CfcValue::Dict(Vec::new()),
        ExprKind::Object(_)
        | ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Bool(_)
        | ExprKind::String(_)
        | ExprKind::Name(_)
        | ExprKind::Qualified(_)
        | ExprKind::Path { .. } => empty_object_placeholder(),
    }
}

fn empty_object_placeholder() -> CfcValue {
    CfcValue::Object {
        type_name: None,
        fields: BTreeMap::new(),
    }
}

fn validate_array_item(
    ctx: &mut BuildCtx<'_>,
    item: &Expr,
    value: &CfcValueRef,
    inferred: &mut Option<crate::build::support::ValueSignature>,
) -> Option<()> {
    let signature = value_signature(value);
    if let Some(expected) = inferred {
        if expected != &signature {
            ctx.errors.push(BuildError::new(
                BuildErrorKind::TypeMismatch,
                "array elements must have the same type",
                Some(item.span),
            ));
            return None;
        }
    } else {
        *inferred = Some(signature);
    }
    Some(())
}

fn validate_dict_entry(
    ctx: &mut BuildCtx<'_>,
    key: &Expr,
    value: &Expr,
    key_value: &CfcValueRef,
    value_value: &CfcValueRef,
    inferred_key: &mut Option<crate::build::support::ValueSignature>,
    inferred_value: &mut Option<crate::build::support::ValueSignature>,
) -> Option<()> {
    let key_signature = value_signature(key_value);
    let value_signature = value_signature(value_value);
    if let Some(expected) = inferred_key {
        if expected != &key_signature {
            ctx.errors.push(BuildError::new(
                BuildErrorKind::TypeMismatch,
                "dict keys must have the same type",
                Some(key.span),
            ));
            return None;
        }
    } else {
        *inferred_key = Some(key_signature);
    }
    if let Some(expected) = inferred_value {
        if expected != &value_signature {
            ctx.errors.push(BuildError::new(
                BuildErrorKind::TypeMismatch,
                "dict values must have the same type",
                Some(value.span),
            ));
            return None;
        }
    } else {
        *inferred_value = Some(value_signature);
    }
    Some(())
}

pub(super) fn object_string_field<'a>(
    fields: &'a [crate::ast::ObjectField],
    name: &str,
) -> Option<&'a str> {
    fields.iter().find_map(|field| {
        (field.name == name).then_some(match &field.value.kind {
            ExprKind::String(value) => Some(value.as_str()),
            _ => None,
        })?
    })
}

pub(super) fn type_discriminator(type_info: &TypeInfo) -> Option<&str> {
    type_info.def.fields.iter().find_map(|field| {
        (field.name == "kind").then_some(match &field.ty {
            TypeRef::StringLiteral(value) => Some(value.as_str()),
            _ => None,
        })?
    })
}
