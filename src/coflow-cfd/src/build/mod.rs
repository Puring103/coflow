use crate::ast::{DataDef, Expr, ExprKind, PathSegment, TypeName, TypeRef};
use crate::container::{CfdContainer, CfdModuleResult, CfdResult, ModuleId};
use crate::error::{BuildError, BuildErrorKind, BuildErrors};
use crate::value::{CfdNominalType, CfdValue, CfdValueRef};
use std::collections::{BTreeMap, HashMap, HashSet};

mod eval;
mod names;
mod object;
mod path;
mod support;
mod symbols;

pub(crate) fn build_modules(
    container: &CfdContainer,
    module_ids: Vec<ModuleId>,
) -> Result<CfdResult, BuildErrors> {
    let mut ctx = BuildCtx::new(container, module_ids);
    ctx.collect_symbols();
    if ctx.errors.is_empty() {
        ctx.build_values();
    }
    ctx.finish()
}

struct BuildCtx<'a> {
    container: &'a CfdContainer,
    module_ids: Vec<ModuleId>,
    data: HashMap<(ModuleId, String), DataDef>,
    memo: HashMap<(ModuleId, String), CfdValueRef>,
    failed: HashSet<(ModuleId, String)>,
    visiting: HashSet<(ModuleId, String)>,
    visiting_stack: Vec<(ModuleId, String)>,
    results: BTreeMap<ModuleId, CfdModuleResult>,
    errors: Vec<BuildError>,
}

impl<'a> BuildCtx<'a> {
    fn new(container: &'a CfdContainer, module_ids: Vec<ModuleId>) -> Self {
        Self {
            container,
            module_ids,
            data: HashMap::new(),
            memo: HashMap::new(),
            failed: HashSet::new(),
            visiting: HashSet::new(),
            visiting_stack: Vec::new(),
            results: BTreeMap::new(),
            errors: Vec::new(),
        }
    }

    fn finish(self) -> Result<CfdResult, BuildErrors> {
        if self.errors.is_empty() {
            Ok(CfdResult::new(self.results))
        } else {
            Err(BuildErrors::new(self.errors))
        }
    }

    fn collect_symbols(&mut self) {
        let module_ids = self.module_ids.clone();
        for module_id in &module_ids {
            let Some(module) = self.container.modules.get(module_id) else {
                self.errors.push(BuildError::new(
                    BuildErrorKind::Module,
                    format!("unknown module `{module_id}`"),
                    None,
                ));
                continue;
            };
            let mut local_names = HashSet::new();
            for def in &module.ast.items {
                if !local_names.insert(def.name.clone()) {
                    self.errors.push(BuildError::new(
                        BuildErrorKind::DuplicateName,
                        format!("duplicate name `{}`", def.name),
                        Some(def.span),
                    ));
                }
                self.validate_type_ref(def.ty.as_ref(), def.span);
                self.data
                    .insert((module_id.clone(), def.name.clone()), def.clone());
            }
        }
    }

    fn validate_type_ref(&mut self, ty: Option<&TypeRef>, span: crate::Span) {
        let Some(ty) = ty else {
            return;
        };
        match ty {
            TypeRef::Int
            | TypeRef::Float
            | TypeRef::Bool
            | TypeRef::String
            | TypeRef::Null
            | TypeRef::StringLiteral(_)
            | TypeRef::IntLiteral(_)
            | TypeRef::BoolLiteral(_)
            | TypeRef::Any => {}
            TypeRef::Array(inner) => self.validate_type_ref(Some(inner), span),
            TypeRef::Dict(key, value) => {
                self.validate_dict_key_type(key, span);
                self.validate_type_ref(Some(value), span);
            }
            TypeRef::Union(items) => {
                for item in items {
                    self.validate_type_ref(Some(item), span);
                }
            }
            TypeRef::Named(TypeName::Local(name)) => {
                if self.container.type_ctx.resolve_type(name).is_none()
                    && self.container.type_ctx.resolve_enum(name).is_none()
                {
                    self.errors.push(BuildError::new(
                        BuildErrorKind::UnknownType,
                        format!("unknown type `{name}`"),
                        Some(span),
                    ));
                }
            }
        }
    }

    fn validate_dict_key_type(&mut self, ty: &TypeRef, span: crate::Span) {
        match ty {
            TypeRef::String | TypeRef::Int | TypeRef::StringLiteral(_) | TypeRef::IntLiteral(_) => {
            }
            TypeRef::Named(TypeName::Local(name))
                if self.container.type_ctx.resolve_enum(name).is_some() => {}
            _ => self.errors.push(BuildError::new(
                BuildErrorKind::InvalidDictKeyType,
                "dict key type must be string, int, or enum",
                Some(span),
            )),
        }
    }

    fn build_values(&mut self) {
        let keys: Vec<_> = self.data.keys().cloned().collect();
        for (module, name) in keys {
            if self.eval_data(&module, &name).is_none() {
                self.errors.push(BuildError::new(
                    BuildErrorKind::Other,
                    format!("failed to build data node `{module}.{name}`"),
                    None,
                ));
            }
        }

        for module_id in &self.module_ids {
            let values = self
                .memo
                .iter()
                .filter(|((module, _), _)| module == module_id)
                .filter(|(key, _)| !self.failed.contains(key))
                .map(|((_, name), value)| (name.clone(), value.clone()))
                .collect();
            self.results
                .insert(module_id.clone(), CfdModuleResult::new(values));
        }
    }

    fn eval_data(&mut self, module: &ModuleId, name: &str) -> Option<CfdValueRef> {
        let key = (module.clone(), name.to_string());
        if let Some(value) = self.memo.get(&key) {
            return (!self.failed.contains(&key)).then(|| value.clone());
        }
        let Some(def) = self.data.get(&key).cloned() else {
            self.errors.push(BuildError::new(
                BuildErrorKind::UnknownName,
                format!("unknown data node `{module}.{name}`"),
                None,
            ));
            return None;
        };
        let placeholder = if data_has_identity(&def) {
            let value = CfdValueRef::pending(identity_placeholder(&def));
            self.memo.insert(key.clone(), value.clone());
            Some(value)
        } else {
            None
        };
        if !self.visiting.insert(key.clone()) {
            if let Some(value) = self.memo.get(&key) {
                if value.is_pending() {
                    return Some(value.clone());
                }
            }
            self.errors.push(BuildError::new(
                BuildErrorKind::Cycle,
                format!("cyclic data reference: {}", self.format_data_cycle(&key)),
                Some(def.span),
            ));
            self.failed.insert(key);
            return None;
        }
        self.visiting_stack.push(key.clone());
        let value = self.eval_expr(module, &def.value, def.ty.as_ref());
        self.visiting_stack.pop();
        self.visiting.remove(&key);
        if let Some(value) = value {
            if let Some(placeholder) = placeholder {
                placeholder.replace(value.borrow().clone());
                Some(placeholder)
            } else {
                self.memo.insert(key, value.clone());
                Some(value)
            }
        } else {
            if placeholder.is_some() {
                self.memo.remove(&key);
            }
            self.failed.insert(key);
            None
        }
    }

    fn format_data_cycle(&self, repeated: &(ModuleId, String)) -> String {
        let start = self
            .visiting_stack
            .iter()
            .position(|key| key == repeated)
            .unwrap_or(0);
        self.visiting_stack[start..]
            .iter()
            .chain(std::iter::once(repeated))
            .map(|(module, name)| format!("{module}.{name}"))
            .collect::<Vec<_>>()
            .join(" -> ")
    }

    fn eval_expr(
        &mut self,
        module: &ModuleId,
        expr: &Expr,
        expected: Option<&TypeRef>,
    ) -> Option<CfdValueRef> {
        match expected {
            Some(TypeRef::Any) | None => self.eval_untyped(module, expr),
            Some(TypeRef::Null) => self
                .eval_untyped(module, expr)
                .and_then(|value| self.ensure_basic_value(expr, value, "null")),
            Some(TypeRef::Int) => self
                .eval_untyped(module, expr)
                .and_then(|value| self.ensure_basic_value(expr, value, "int")),
            Some(TypeRef::IntLiteral(expected)) => self
                .eval_untyped(module, expr)
                .and_then(|value| self.ensure_int_literal(expr, value, *expected)),
            Some(TypeRef::Float) => self
                .eval_untyped(module, expr)
                .and_then(|value| self.ensure_basic_value(expr, value, "float")),
            Some(TypeRef::Bool) => self
                .eval_untyped(module, expr)
                .and_then(|value| self.ensure_basic_value(expr, value, "bool")),
            Some(TypeRef::BoolLiteral(expected)) => self
                .eval_untyped(module, expr)
                .and_then(|value| self.ensure_bool_literal(expr, value, *expected)),
            Some(TypeRef::String) => self
                .eval_untyped(module, expr)
                .and_then(|value| self.ensure_basic_value(expr, value, "string")),
            Some(TypeRef::StringLiteral(expected)) => self
                .eval_untyped(module, expr)
                .and_then(|value| self.ensure_string_literal(expr, value, expected)),
            Some(TypeRef::Array(inner)) => self.eval_array(module, expr, Some(inner)),
            Some(TypeRef::Dict(key, value)) => self.eval_dict(module, expr, Some(key), Some(value)),
            Some(TypeRef::Union(branches)) => self.eval_union(module, expr, branches),
            Some(TypeRef::Named(TypeName::Local(name))) => self.eval_named(module, expr, name),
        }
    }

    fn eval_untyped(&mut self, module: &ModuleId, expr: &Expr) -> Option<CfdValueRef> {
        match &expr.kind {
            ExprKind::Null => Some(CfdValueRef::new(CfdValue::Null)),
            ExprKind::Int(value) => Some(CfdValueRef::new(CfdValue::Int(*value))),
            ExprKind::Float(value) => Some(CfdValueRef::new(CfdValue::Float(*value))),
            ExprKind::Bool(value) => Some(CfdValueRef::new(CfdValue::Bool(*value))),
            ExprKind::String(value) => Some(CfdValueRef::new(CfdValue::String(value.clone()))),
            ExprKind::Name(name) => self.eval_data_name(module, name, expr.span),
            ExprKind::Qualified(parts) => self.eval_enum_or_path(module, expr, parts),
            ExprKind::QualifiedRef { module_id, name } => {
                self.eval_data_name(&ModuleId::from(module_id.clone()), name, expr.span)
            }
            ExprKind::Path { root, segments } => {
                let root = self.eval_data_name(module, root, expr.span)?;
                self.select_path(root, segments, expr.span)
            }
            ExprKind::TypedObject { ty, fields } => {
                let TypeName::Local(name) = ty;
                self.eval_object(module, expr.span, Some(name), fields)
            }
            ExprKind::Object(fields) => self.eval_object(module, expr.span, None, fields),
            ExprKind::Array(items) => self.eval_array_items(module, items, None),
            ExprKind::Dict(entries) => self.eval_dict_entries(module, entries, None, None),
        }
    }

    fn eval_named(
        &mut self,
        module: &ModuleId,
        expr: &Expr,
        name: &str,
    ) -> Option<CfdValueRef> {
        if let Some(enum_def) = self.container.type_ctx.resolve_enum(name) {
            return self.eval_enum_value(expr, enum_def.module, name);
        }
        if self.container.type_ctx.resolve_type(name).is_none() {
            self.errors.push(BuildError::new(
                BuildErrorKind::UnknownType,
                format!("unknown type `{name}`"),
                Some(expr.span),
            ));
            return None;
        }
        match &expr.kind {
            ExprKind::TypedObject { ty, fields } => {
                let TypeName::Local(actual) = ty;
                if actual != name {
                    return self.type_error(expr, name);
                }
                self.eval_object(module, expr.span, Some(name), fields)
            }
            ExprKind::Object(fields) => self.eval_object(module, expr.span, Some(name), fields),
            _ => self
                .eval_untyped(module, expr)
                .and_then(|value| self.ensure_named_value(expr, value, name)),
        }
    }

    fn eval_object(
        &mut self,
        module: &ModuleId,
        span: crate::Span,
        ty: Option<&str>,
        fields: &[crate::ast::ObjectField],
    ) -> Option<CfdValueRef> {
        let type_name = ty.map(|name| {
            let schema = self.container.type_ctx.resolve_type(name).unwrap_or_else(|| {
                todo!("CftContainer type lookup reported missing type after validation")
            });
            CfdNominalType {
                module: schema.module,
                name: name.to_string(),
            }
        });
        let mut out = BTreeMap::new();
        let mut names = HashSet::new();
        for field in fields {
            if !names.insert(field.name.clone()) {
                self.errors.push(BuildError::new(
                    BuildErrorKind::DuplicateField,
                    format!("duplicate field `{}`", field.name),
                    Some(field.span),
                ));
                continue;
            }
            out.insert(field.name.clone(), self.eval_untyped(module, &field.value)?);
        }
        let _ = span;
        Some(CfdValueRef::new(CfdValue::Object {
            type_name,
            fields: out,
        }))
    }

    fn eval_array(
        &mut self,
        module: &ModuleId,
        expr: &Expr,
        expected: Option<&TypeRef>,
    ) -> Option<CfdValueRef> {
        let ExprKind::Array(items) = &expr.kind else {
            return self
                .eval_untyped(module, expr)
                .and_then(|value| self.ensure_basic_value(expr, value, "array"));
        };
        self.eval_array_items(module, items, expected)
    }

    fn eval_array_items(
        &mut self,
        module: &ModuleId,
        items: &[Expr],
        expected: Option<&TypeRef>,
    ) -> Option<CfdValueRef> {
        let mut out = Vec::new();
        for item in items {
            out.push(self.eval_expr(module, item, expected)?);
        }
        Some(CfdValueRef::new(CfdValue::Array(out)))
    }

    fn eval_dict(
        &mut self,
        module: &ModuleId,
        expr: &Expr,
        key_ty: Option<&TypeRef>,
        value_ty: Option<&TypeRef>,
    ) -> Option<CfdValueRef> {
        let ExprKind::Dict(entries) = &expr.kind else {
            return self
                .eval_untyped(module, expr)
                .and_then(|value| self.ensure_basic_value(expr, value, "dict"));
        };
        self.eval_dict_entries(module, entries, key_ty, value_ty)
    }

    fn eval_dict_entries(
        &mut self,
        module: &ModuleId,
        entries: &[(Expr, Expr)],
        key_ty: Option<&TypeRef>,
        value_ty: Option<&TypeRef>,
    ) -> Option<CfdValueRef> {
        let mut out = Vec::new();
        for (key, value) in entries {
            out.push((
                self.eval_expr(module, key, key_ty)?,
                self.eval_expr(module, value, value_ty)?,
            ));
        }
        Some(CfdValueRef::new(CfdValue::Dict(out)))
    }

    fn eval_union(
        &mut self,
        module: &ModuleId,
        expr: &Expr,
        branches: &[TypeRef],
    ) -> Option<CfdValueRef> {
        for branch in branches {
            let before = self.errors.len();
            if let Some(value) = self.eval_expr(module, expr, Some(branch)) {
                return Some(value);
            }
            self.errors.truncate(before);
        }
        self.type_error(expr, "union")
    }

    fn eval_enum_or_path(
        &mut self,
        module: &ModuleId,
        expr: &Expr,
        parts: &[String],
    ) -> Option<CfdValueRef> {
        if let [enum_name, variant] = parts {
            if let Some(enum_def) = self.container.type_ctx.resolve_enum(enum_name) {
                return self.eval_enum_variant(expr.span, enum_def.module, enum_name, variant);
            }
        }
        if let [root, field] = parts {
            let root = self.eval_data_name(module, root, expr.span)?;
            return self.select_path(root, &[PathSegment::Field(field.clone())], expr.span);
        }
        self.errors.push(BuildError::new(
            BuildErrorKind::Path,
            "qualified access must be enum.variant or data.field",
            Some(expr.span),
        ));
        None
    }

    fn eval_enum_value(
        &mut self,
        expr: &Expr,
        enum_module: ModuleId,
        enum_name: &str,
    ) -> Option<CfdValueRef> {
        let ExprKind::Qualified(parts) = &expr.kind else {
            return self.type_error(expr, enum_name);
        };
        let [actual_enum, variant] = parts.as_slice() else {
            return self.type_error(expr, enum_name);
        };
        if actual_enum != enum_name {
            return self.type_error(expr, enum_name);
        }
        self.eval_enum_variant(expr.span, enum_module, enum_name, variant)
    }

    fn eval_enum_variant(
        &mut self,
        span: crate::Span,
        enum_module: ModuleId,
        enum_name: &str,
        variant: &str,
    ) -> Option<CfdValueRef> {
        let Some(enum_def) = self.container.type_ctx.resolve_enum(enum_name) else {
            self.errors.push(BuildError::new(
                BuildErrorKind::UnknownType,
                format!("unknown enum `{enum_name}`"),
                Some(span),
            ));
            return None;
        };
        let mut next = 0;
        for candidate in enum_def.variants {
            let value = candidate.value.unwrap_or(next);
            next = value + 1;
            if candidate.name == variant {
                return Some(CfdValueRef::new(CfdValue::Enum {
                    enum_type: CfdNominalType {
                        module: enum_module,
                        name: enum_name.to_string(),
                    },
                    variant: variant.to_string(),
                    value,
                }));
            }
        }
        self.errors.push(BuildError::new(
            BuildErrorKind::UnknownEnumVariant,
            format!("unknown enum variant `{enum_name}.{variant}`"),
            Some(span),
        ));
        None
    }

    fn eval_data_name(
        &mut self,
        module: &ModuleId,
        name: &str,
        span: crate::Span,
    ) -> Option<CfdValueRef> {
        if !self.data.contains_key(&(module.clone(), name.to_string())) {
            self.errors.push(BuildError::new(
                BuildErrorKind::UnknownName,
                format!("unknown data node `{module}.{name}`"),
                Some(span),
            ));
            return None;
        }
        self.eval_data(module, name)
    }

    fn select_path(
        &mut self,
        mut value: CfdValueRef,
        segments: &[PathSegment],
        span: crate::Span,
    ) -> Option<CfdValueRef> {
        for segment in segments {
            while let Some(inner) = {
                let borrowed = value.borrow();
                match &*borrowed {
                    CfdValue::Union { value, .. } => Some(value.clone()),
                    _ => None,
                }
            } {
                value = inner;
            }
            let next = {
                let borrowed = value.borrow();
                match (segment, &*borrowed) {
                    (PathSegment::Field(field), CfdValue::Object { fields, .. }) => {
                        let Some(next) = fields.get(field) else {
                            self.errors.push(BuildError::new(
                                BuildErrorKind::Path,
                                format!("missing field `{field}`"),
                                Some(span),
                            ));
                            return None;
                        };
                        next.clone()
                    }
                    (PathSegment::Index(index), CfdValue::Array(items)) => {
                        let Some(next) = items.get(*index) else {
                            self.errors.push(BuildError::new(
                                BuildErrorKind::Path,
                                format!("array index `{index}` is out of bounds"),
                                Some(span),
                            ));
                            return None;
                        };
                        next.clone()
                    }
                    (PathSegment::Index(index), CfdValue::Dict(entries)) => {
                        let Some((_, next)) = entries.get(*index) else {
                            self.errors.push(BuildError::new(
                                BuildErrorKind::Path,
                                format!("dict index `{index}` is out of bounds"),
                                Some(span),
                            ));
                            return None;
                        };
                        next.clone()
                    }
                    (PathSegment::Field(field), _) => {
                        self.errors.push(BuildError::new(
                            BuildErrorKind::Path,
                            format!("cannot select field `{field}`"),
                            Some(span),
                        ));
                        return None;
                    }
                    (PathSegment::Index(index), _) => {
                        self.errors.push(BuildError::new(
                            BuildErrorKind::Path,
                            format!("cannot index value at `{index}`"),
                            Some(span),
                        ));
                        return None;
                    }
                }
            };
            value = next;
        }
        Some(value)
    }

    fn ensure_basic_value(
        &mut self,
        expr: &Expr,
        value: CfdValueRef,
        expected: &str,
    ) -> Option<CfdValueRef> {
        let matches = {
            let borrowed = value.borrow();
            matches!(
                (&*borrowed, expected),
                (CfdValue::Null, "null")
                    | (CfdValue::Int(_), "int")
                    | (CfdValue::Float(_), "float")
                    | (CfdValue::Bool(_), "bool")
                    | (CfdValue::String(_), "string")
                    | (CfdValue::Array(_), "array")
                    | (CfdValue::Dict(_), "dict")
            )
        };
        if matches {
            Some(value)
        } else {
            self.type_error(expr, expected)
        }
    }

    fn ensure_int_literal(
        &mut self,
        expr: &Expr,
        value: CfdValueRef,
        expected: i64,
    ) -> Option<CfdValueRef> {
        if matches!(&*value.borrow(), CfdValue::Int(actual) if *actual == expected) {
            Some(value)
        } else {
            self.type_error(expr, &expected.to_string())
        }
    }

    fn ensure_bool_literal(
        &mut self,
        expr: &Expr,
        value: CfdValueRef,
        expected: bool,
    ) -> Option<CfdValueRef> {
        if matches!(&*value.borrow(), CfdValue::Bool(actual) if *actual == expected) {
            Some(value)
        } else {
            self.type_error(expr, &expected.to_string())
        }
    }

    fn ensure_string_literal(
        &mut self,
        expr: &Expr,
        value: CfdValueRef,
        expected: &str,
    ) -> Option<CfdValueRef> {
        if matches!(&*value.borrow(), CfdValue::String(actual) if actual == expected) {
            Some(value)
        } else {
            self.type_error(expr, &format!("{expected:?}"))
        }
    }

    fn ensure_named_value(
        &mut self,
        expr: &Expr,
        value: CfdValueRef,
        expected: &str,
    ) -> Option<CfdValueRef> {
        if matches!(&*value.borrow(), CfdValue::Object { type_name: Some(actual), .. } if actual.name == expected)
            || matches!(&*value.borrow(), CfdValue::Enum { enum_type, .. } if enum_type.name == expected)
        {
            Some(value)
        } else {
            self.type_error(expr, expected)
        }
    }

    fn type_error<T>(&mut self, expr: &Expr, expected: &str) -> Option<T> {
        self.errors.push(BuildError::new(
            BuildErrorKind::TypeMismatch,
            format!("expected `{expected}`"),
            Some(expr.span),
        ));
        None
    }
}

fn data_has_identity(def: &DataDef) -> bool {
    matches!(
        def.value.kind,
        ExprKind::TypedObject { .. } | ExprKind::Object(_) | ExprKind::Array(_) | ExprKind::Dict(_)
    )
}

fn identity_placeholder(def: &DataDef) -> CfdValue {
    match def.value.kind {
        ExprKind::Array(_) => CfdValue::Array(Vec::new()),
        ExprKind::Dict(_) => CfdValue::Dict(Vec::new()),
        ExprKind::TypedObject { .. }
        | ExprKind::Object(_)
        | ExprKind::Null
        | ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Bool(_)
        | ExprKind::String(_)
        | ExprKind::Name(_)
        | ExprKind::Qualified(_)
        | ExprKind::QualifiedRef { .. }
        | ExprKind::Path { .. } => CfdValue::Object {
            type_name: None,
            fields: BTreeMap::new(),
        },
    }
}
