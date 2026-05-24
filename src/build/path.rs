use super::{BuildCtx, ObjectEvalState};
use crate::ast::{Expr, ExprKind, PathSegment, TypeName};
use crate::build::support::build_error;
use crate::container::ModuleId;
use crate::error::BuildError;
use crate::span::Span;
use crate::value::{CfcNominalType, CfcValue, CfcValueRef};
use std::collections::BTreeMap;

impl BuildCtx<'_> {
    pub(super) fn eval_qualified_untyped(
        &mut self,
        module: &ModuleId,
        expr: &Expr,
        parts: &[String],
        locals: Option<&BTreeMap<String, CfcValueRef>>,
    ) -> Option<CfcValueRef> {
        match parts {
            [a, b] => {
                if let Some(enum_info) = self.enums.get(&(module.clone(), a.clone())) {
                    if let Some(value) = enum_info.values.get(b) {
                        return Some(CfcValueRef::new(CfcValue::Enum {
                            enum_type: CfcNominalType {
                                module: module.clone(),
                                name: a.clone(),
                            },
                            variant: b.clone(),
                            value: *value,
                        }));
                    }
                }
                if let Some(dep) = self.try_resolve_import(module, a) {
                    if let Some(enum_value) = self.eval_remote_single_part_enum(expr, &dep, b) {
                        return Some(enum_value);
                    }
                }
                self.eval_qualified_as_path_or_data(module, expr, parts, locals)
            }
            [alias, enum_name, variant] => {
                let dep = self.resolve_import(module, alias, expr.span)?;
                self.eval_enum_value_by_name(expr.span, &dep, enum_name, variant)
            }
            _ => {
                self.errors.push(BuildError {
                    message: "qualified access may only use one import alias".to_string(),
                    span: Some(expr.span),
                });
                None
            }
        }
    }

    fn eval_remote_single_part_enum(
        &mut self,
        expr: &Expr,
        module: &ModuleId,
        variant: &str,
    ) -> Option<CfcValueRef> {
        let mut matches = self.enums.iter().filter(|((enum_module, _), info)| {
            enum_module == module && info.values.contains_key(variant)
        });
        let ((enum_module, enum_name), _) = matches.next()?;
        let enum_module = enum_module.clone();
        let enum_name = enum_name.clone();
        if matches.next().is_some() {
            self.errors.push(BuildError {
                message: format!("ambiguous enum variant `{variant}`"),
                span: Some(expr.span),
            });
            return None;
        }
        self.eval_enum_value_by_name(expr.span, &enum_module, &enum_name, variant)
    }

    pub(super) fn eval_enum_value(
        &mut self,
        module: &ModuleId,
        expr: &Expr,
        enum_module: &ModuleId,
        enum_name: &str,
    ) -> Option<CfcValueRef> {
        match &expr.kind {
            ExprKind::Qualified(parts) if parts.len() == 2 => {
                if parts[0] != enum_name && enum_module == module {
                    return self.type_error(expr, enum_name);
                }
                self.eval_enum_value_by_name(expr.span, enum_module, enum_name, &parts[1])
            }
            ExprKind::Qualified(parts) if parts.len() == 3 => {
                let alias = &parts[0];
                let actual_enum = &parts[1];
                let variant = &parts[2];
                if actual_enum != enum_name {
                    return self.type_error(expr, enum_name);
                }
                let dep = self.resolve_import(module, alias, expr.span)?;
                if &dep != enum_module {
                    return self.type_error(expr, enum_name);
                }
                self.eval_enum_value_by_name(expr.span, enum_module, enum_name, variant)
            }
            ExprKind::Path { root, segments } if segments.len() == 2 => {
                let [PathSegment::Field(actual_enum), PathSegment::Field(variant)] =
                    segments.as_slice()
                else {
                    return self.type_error(expr, enum_name);
                };
                if actual_enum != enum_name {
                    return self.type_error(expr, enum_name);
                }
                let dep = self.resolve_import(module, root, expr.span)?;
                if &dep != enum_module {
                    return self.type_error(expr, enum_name);
                }
                self.eval_enum_value_by_name(expr.span, enum_module, enum_name, variant)
            }
            _ => self.type_error(expr, enum_name),
        }
    }

    fn eval_enum_value_by_name(
        &mut self,
        span: Span,
        module: &ModuleId,
        enum_name: &str,
        variant: &str,
    ) -> Option<CfcValueRef> {
        let Some(enum_info) = self.enums.get(&(module.clone(), enum_name.to_string())) else {
            self.errors.push(BuildError {
                message: format!("unknown enum `{enum_name}`"),
                span: Some(span),
            });
            return None;
        };
        let Some(value) = enum_info.values.get(variant) else {
            self.errors.push(BuildError {
                message: format!("unknown enum variant `{enum_name}.{variant}`"),
                span: Some(span),
            });
            return None;
        };
        Some(CfcValueRef::new(CfcValue::Enum {
            enum_type: CfcNominalType {
                module: module.clone(),
                name: enum_name.to_string(),
            },
            variant: variant.to_string(),
            value: *value,
        }))
    }

    pub(super) fn eval_qualified_as_path_or_data(
        &mut self,
        module: &ModuleId,
        expr: &Expr,
        parts: &[String],
        locals: Option<&BTreeMap<String, CfcValueRef>>,
    ) -> Option<CfcValueRef> {
        match parts {
            [a, b] => {
                if let Some(dep) = self.try_resolve_import(module, a) {
                    if self.data.contains_key(&(dep.clone(), b.clone())) {
                        return self.eval_data_name(&dep, b, expr.span);
                    }
                }
                self.eval_path(
                    module,
                    a,
                    &[PathSegment::Field(b.clone())],
                    expr.span,
                    locals,
                )
            }
            [alias, data, field] => {
                let dep = self.resolve_import(module, alias, expr.span)?;
                let root = self.eval_data_name(&dep, data, expr.span)?;
                self.select_path(root, &[PathSegment::Field(field.clone())], expr.span)
            }
            _ => None,
        }
    }

    pub(super) fn eval_qualified_in_object_state(
        &mut self,
        module: &ModuleId,
        default_module: &ModuleId,
        expr: &Expr,
        parts: &[String],
        state: &mut ObjectEvalState,
    ) -> Option<CfcValueRef> {
        match parts {
            [a, b] => {
                if let Some(dep) = self.try_resolve_import(module, a) {
                    if self.data.contains_key(&(dep.clone(), b.clone())) {
                        return self.eval_data_name(&dep, b, expr.span);
                    }
                }
                self.eval_path_in_object_state_parts(
                    module,
                    default_module,
                    a,
                    &[PathSegment::Field(b.clone())],
                    expr.span,
                    state,
                )
            }
            [alias, data, field] => {
                let dep = self.resolve_import(module, alias, expr.span)?;
                let root = self.eval_data_name(&dep, data, expr.span)?;
                self.select_path(root, &[PathSegment::Field(field.clone())], expr.span)
            }
            _ => None,
        }
    }

    pub(super) fn eval_path_expr(
        &mut self,
        module: &ModuleId,
        expr: &Expr,
        locals: Option<&BTreeMap<String, CfcValueRef>>,
    ) -> Option<CfcValueRef> {
        let ExprKind::Path { root, segments } = &expr.kind else {
            return None;
        };
        self.eval_path(module, root, segments, expr.span, locals)
    }

    pub(super) fn eval_path_in_object_state(
        &mut self,
        module: &ModuleId,
        default_module: &ModuleId,
        expr: &Expr,
        state: &mut ObjectEvalState,
    ) -> Option<CfcValueRef> {
        let ExprKind::Path { root, segments } = &expr.kind else {
            return None;
        };
        self.eval_path_in_object_state_parts(
            module,
            default_module,
            root,
            segments,
            expr.span,
            state,
        )
    }

    fn eval_path_in_object_state_parts(
        &mut self,
        module: &ModuleId,
        default_module: &ModuleId,
        root: &str,
        segments: &[PathSegment],
        span: Span,
        state: &mut ObjectEvalState,
    ) -> Option<CfcValueRef> {
        if state.plans.contains_key(root) || state.parent_locals.contains_key(root) {
            let root_value =
                self.resolve_name_in_object_state(module, default_module, root, span, state)?;
            return self.select_path(root_value, segments, span);
        }
        if self.try_resolve_import(module, root).is_some() {
            return self.eval_imported_path(module, root, segments, span);
        }
        let root_value =
            self.resolve_name_in_object_state(module, default_module, root, span, state)?;
        self.select_path(root_value, segments, span)
    }

    fn eval_path(
        &mut self,
        module: &ModuleId,
        root: &str,
        segments: &[PathSegment],
        span: Span,
        locals: Option<&BTreeMap<String, CfcValueRef>>,
    ) -> Option<CfcValueRef> {
        if let Some(root_value) = Self::resolve_local(root, locals) {
            return self.select_path(root_value, segments, span);
        }
        if self.try_resolve_import(module, root).is_some() {
            return self.eval_imported_path(module, root, segments, span);
        }
        let root_value = self.eval_data_name(module, root, span)?;
        self.select_path(root_value, segments, span)
    }

    fn eval_imported_path(
        &mut self,
        module: &ModuleId,
        root: &str,
        segments: &[PathSegment],
        span: Span,
    ) -> Option<CfcValueRef> {
        let dep = self.resolve_import(module, root, span)?;
        let Some((first, rest)) = segments.split_first() else {
            self.errors.push(BuildError {
                message: format!("import path `{root}` must name a data node"),
                span: Some(span),
            });
            return None;
        };
        let PathSegment::Field(data) = first else {
            self.errors.push(BuildError {
                message: format!("import path `{root}` must select a data node before indexing"),
                span: Some(span),
            });
            return None;
        };
        if !self.data.contains_key(&(dep.clone(), data.clone())) {
            if let [PathSegment::Field(variant)] = rest {
                if self.enums.contains_key(&(dep.clone(), data.clone())) {
                    return self.eval_enum_value_by_name(span, &dep, data, variant);
                }
            }
            self.errors.push(BuildError {
                message: format!("unknown data node `{root}.{data}`"),
                span: Some(span),
            });
            return None;
        }
        let root = self.eval_data_name(&dep, data, span)?;
        self.select_path(root, rest, span)
    }

    fn select_path(
        &mut self,
        mut value: CfcValueRef,
        segments: &[PathSegment],
        span: Span,
    ) -> Option<CfcValueRef> {
        for segment in segments {
            let next = {
                let borrowed = value.borrow();
                match (segment, &*borrowed) {
                    (PathSegment::Field(field), CfcValue::Object { fields, .. }) => {
                        let Some(next) = fields.get(field) else {
                            self.errors.push(BuildError {
                                message: format!("missing field `{field}`"),
                                span: Some(span),
                            });
                            return None;
                        };
                        next.clone()
                    }
                    (PathSegment::Index(index), CfcValue::Array(items)) => {
                        let Some(next) = items.get(*index) else {
                            self.errors.push(BuildError {
                                message: format!("array index `{index}` is out of bounds"),
                                span: Some(span),
                            });
                            return None;
                        };
                        next.clone()
                    }
                    (PathSegment::Index(index), CfcValue::Dict(entries)) => {
                        let Some((_, next)) = entries.get(*index) else {
                            self.errors.push(BuildError {
                                message: format!("dict index `{index}` is out of bounds"),
                                span: Some(span),
                            });
                            return None;
                        };
                        next.clone()
                    }
                    (PathSegment::Field(field), _) => {
                        self.errors.push(BuildError {
                            message: format!("cannot select field `{field}`"),
                            span: Some(span),
                        });
                        return None;
                    }
                    (PathSegment::Index(index), _) => {
                        self.errors.push(BuildError {
                            message: format!("cannot index value at `{index}`"),
                            span: Some(span),
                        });
                        return None;
                    }
                }
            };
            value = next;
        }
        Some(value)
    }

    pub(super) fn resolve_type_name(
        &mut self,
        module: &ModuleId,
        name: &TypeName,
        span: Span,
    ) -> Option<(ModuleId, String)> {
        match name {
            TypeName::Local(name) => Some((module.clone(), name.clone())),
            TypeName::Imported { alias, name } => {
                Some((self.resolve_import(module, alias, span)?, name.clone()))
            }
        }
    }

    fn resolve_import(&mut self, module: &ModuleId, alias: &str, span: Span) -> Option<ModuleId> {
        let Some(module_data) = self.container.modules.get(module) else {
            self.errors
                .push(build_error(format!("unknown module `{module}`")));
            return None;
        };
        let Some(import) = module_data
            .imports
            .iter()
            .find(|import| import.alias == alias)
        else {
            self.errors.push(BuildError {
                message: format!("unknown import alias `{alias}`"),
                span: Some(span),
            });
            return None;
        };
        let Some(dep) = module_data.bindings.get(&import.id) else {
            self.errors.push(BuildError {
                message: format!("unbound import `{alias}`"),
                span: Some(import.span),
            });
            return None;
        };
        Some(dep.clone())
    }

    pub(super) fn try_resolve_import(&self, module: &ModuleId, alias: &str) -> Option<ModuleId> {
        let module_data = self.container.modules.get(module)?;
        let import = module_data
            .imports
            .iter()
            .find(|import| import.alias == alias)?;
        module_data.bindings.get(&import.id).cloned()
    }

    pub(super) fn resolve_local(
        name: &str,
        locals: Option<&BTreeMap<String, CfcValueRef>>,
    ) -> Option<CfcValueRef> {
        locals.and_then(|locals| locals.get(name).cloned())
    }

    pub(super) fn resolve_name_value(
        &mut self,
        module: &ModuleId,
        name: &str,
        span: Span,
        locals: Option<&BTreeMap<String, CfcValueRef>>,
    ) -> Option<CfcValueRef> {
        Self::resolve_local(name, locals).or_else(|| self.eval_data_name(module, name, span))
    }

    pub(super) fn resolve_name_in_object_state(
        &mut self,
        module: &ModuleId,
        default_module: &ModuleId,
        name: &str,
        span: Span,
        state: &mut ObjectEvalState,
    ) -> Option<CfcValueRef> {
        if state.plans.contains_key(name) {
            return self.eval_object_field(module, default_module, state, name);
        }
        state
            .parent_locals
            .get(name)
            .cloned()
            .or_else(|| self.eval_data_name(module, name, span))
    }

    pub(super) fn eval_data_name(
        &mut self,
        module: &ModuleId,
        name: &str,
        span: Span,
    ) -> Option<CfcValueRef> {
        if !self.data.contains_key(&(module.clone(), name.to_string())) {
            self.errors.push(BuildError {
                message: format!("unknown data node `{name}`"),
                span: Some(span),
            });
            return None;
        }
        self.eval_data(module, name)
    }
}
