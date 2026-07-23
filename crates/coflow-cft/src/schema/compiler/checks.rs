#[path = "check_functions.rs"]
mod functions;
#[path = "check_operators.rs"]
mod operators;

use super::inferred_type::{
    types_assignable, types_comparable, unwrap_nullable, unwrap_reference, InferredType,
};
use super::state::{SymbolKind, TypeInfo};
use super::SchemaCompiler;
use crate::diagnostics::{CftDiagnostic, CftErrorCode};
use crate::schema::CftValueType;
use crate::syntax::ast::{
    CheckExpr, CheckExprKind, CheckFormatSegment, CheckMessageKind, CheckStmt, NameRef,
    TypePredicate,
};
use crate::syntax::Span;
use std::collections::HashMap;

pub(super) struct CheckTypeAnalyzer<'a, 'b> {
    compiler: &'a mut SchemaCompiler<'b>,
    module: crate::ModuleId,
    scope: CheckScope,
    locals: Vec<HashMap<String, InferredType>>,
}

enum CheckScope {
    Record(String),
    TopLevel,
}

fn is_formattable(ty: &InferredType) -> bool {
    match ty {
        InferredType::Null | InferredType::Unknown => true,
        InferredType::Value(
            CftValueType::Int
            | CftValueType::Float
            | CftValueType::Bool
            | CftValueType::String
            | CftValueType::Enum(_),
        ) => true,
        InferredType::Value(CftValueType::Nullable(inner)) => {
            is_formattable(&InferredType::Value((**inner).clone()))
        }
        InferredType::Value(
            CftValueType::Array(_)
            | CftValueType::Dict(_, _)
            | CftValueType::Object(_)
            | CftValueType::RecordRef(_),
        )
        | InferredType::EnumNamespace(_)
        | InferredType::Entry(_, _)
        | InferredType::EmptyArray
        | InferredType::EmptyObject => false,
    }
}

impl<'a, 'b> CheckTypeAnalyzer<'a, 'b> {
    pub(super) fn new(compiler: &'a mut SchemaCompiler<'b>, type_info: &'a TypeInfo<'b>) -> Self {
        Self {
            compiler,
            module: type_info.module.clone(),
            scope: CheckScope::Record(type_info.def.name.clone()),
            locals: Vec::new(),
        }
    }

    pub(super) fn top_level(
        compiler: &'a mut SchemaCompiler<'b>,
        module: crate::ModuleId,
    ) -> Self {
        Self {
            compiler,
            module,
            scope: CheckScope::TopLevel,
            locals: Vec::new(),
        }
    }

    pub(super) fn check_stmts(&mut self, stmts: &[CheckStmt]) {
        for stmt in stmts {
            self.check_stmt(stmt);
        }
    }

    fn check_stmt(&mut self, stmt: &CheckStmt) {
        match stmt {
            CheckStmt::Expr {
                condition, message, ..
            } => {
                let ty = self.check_expr_value(condition);
                self.expect_bool(&ty, condition.span);
                if let Some(message) = message {
                    if let CheckMessageKind::Formatted(segments) = &message.kind {
                        self.check_format_segments(segments);
                    }
                }
            }
            CheckStmt::When {
                condition, body, ..
            } => {
                let ty = self.check_expr_value(condition);
                self.expect_bool(&ty, condition.span);
                self.check_stmts(body);
            }
            CheckStmt::Quantifier {
                bindings,
                collection,
                body,
                span,
                ..
            } => {
                for binding in bindings {
                    if crate::is_cft_reserved_identifier(&binding.name) {
                        self.diag(
                            CftErrorCode::ReservedIdentifier,
                            binding.span,
                            format!("`{}` is a reserved identifier", binding.name),
                        );
                    }
                    if bindings
                        .iter()
                        .filter(|candidate| candidate.name == binding.name)
                        .count()
                        > 1
                        || self
                            .locals
                            .iter()
                            .rev()
                            .any(|scope| scope.contains_key(&binding.name))
                    {
                        self.diag(
                            CftErrorCode::InvalidQuantifierBindings,
                            binding.span,
                            format!(
                                "quantifier binding `{}` is duplicated or shadows an outer binding",
                                binding.name
                            ),
                        );
                    }
                }
                let col_ty = self.check_expr_value(collection);
                let col_ty = unwrap_nullable(&col_ty);
                let (scope, layout) = match (col_ty, bindings.as_slice()) {
                    (InferredType::Value(CftValueType::Array(inner)), [binding]) => (
                        HashMap::from([(binding.name.clone(), InferredType::Value(*inner))]),
                        crate::schema::CftSchemaQuantifierBindings::Single {
                            binding: binding.name.clone(),
                        },
                    ),
                    (InferredType::Value(CftValueType::Array(inner)), [item, index]) => (
                        HashMap::from([
                            (item.name.clone(), InferredType::Value(*inner)),
                            (index.name.clone(), InferredType::int()),
                        ]),
                        crate::schema::CftSchemaQuantifierBindings::Array {
                            item: item.name.clone(),
                            index: index.name.clone(),
                        },
                    ),
                    (InferredType::Value(CftValueType::Dict(key, value)), [binding]) => (
                        HashMap::from([(
                            binding.name.clone(),
                            InferredType::Entry(
                                Box::new(InferredType::Value(*key)),
                                Box::new(InferredType::Value(*value)),
                            ),
                        )]),
                        crate::schema::CftSchemaQuantifierBindings::Single {
                            binding: binding.name.clone(),
                        },
                    ),
                    (
                        InferredType::Value(CftValueType::Dict(key, value)),
                        [key_binding, value_binding],
                    ) => (
                        HashMap::from([
                            (key_binding.name.clone(), InferredType::Value(*key)),
                            (value_binding.name.clone(), InferredType::Value(*value)),
                        ]),
                        crate::schema::CftSchemaQuantifierBindings::Dict {
                            key: key_binding.name.clone(),
                            value: value_binding.name.clone(),
                        },
                    ),
                    (InferredType::Unknown, [binding]) => (
                        HashMap::from([(binding.name.clone(), InferredType::Unknown)]),
                        crate::schema::CftSchemaQuantifierBindings::Single {
                            binding: binding.name.clone(),
                        },
                    ),
                    (InferredType::Unknown, [first, second]) => (
                        HashMap::from([
                            (first.name.clone(), InferredType::Unknown),
                            (second.name.clone(), InferredType::Unknown),
                        ]),
                        crate::schema::CftSchemaQuantifierBindings::Array {
                            item: first.name.clone(),
                            index: second.name.clone(),
                        },
                    ),
                    _ => {
                        self.diag(
                            CftErrorCode::QuantifierRequiresCollection,
                            *span,
                            "quantifier target must be an array or dict",
                        );
                        match bindings.as_slice() {
                            [binding] => (
                                HashMap::from([(binding.name.clone(), InferredType::Unknown)]),
                                crate::schema::CftSchemaQuantifierBindings::Single {
                                    binding: binding.name.clone(),
                                },
                            ),
                            [first, second] => (
                                HashMap::from([
                                    (first.name.clone(), InferredType::Unknown),
                                    (second.name.clone(), InferredType::Unknown),
                                ]),
                                crate::schema::CftSchemaQuantifierBindings::Array {
                                    item: first.name.clone(),
                                    index: second.name.clone(),
                                },
                            ),
                            _ => return,
                        }
                    }
                };
                self.compiler.quantifier_bindings.insert(
                    (self.module.clone(), span.start, span.end),
                    layout,
                );
                self.locals.push(scope);
                self.check_stmts(body);
                self.locals.pop();
            }
        }
    }

    fn check_expr(&mut self, expr: &CheckExpr) -> InferredType {
        match &expr.kind {
            CheckExprKind::Int(_) => InferredType::int(),
            CheckExprKind::Float(_) => InferredType::float(),
            CheckExprKind::Bool(_) => InferredType::bool(),
            CheckExprKind::Null => InferredType::Null,
            CheckExprKind::String(_) => InferredType::string(),
            CheckExprKind::FormattedString(segments) => {
                self.check_format_segments(segments);
                InferredType::string()
            }
            CheckExprKind::Name(name) => self.resolve_value_name(name, expr.span),
            CheckExprKind::Unary { op, expr: inner } => {
                let ty = self.check_expr_value(inner);
                self.check_unary(*op, &ty, expr.span)
            }
            CheckExprKind::BinOp { op, lhs, rhs } => {
                let lhs_ty = self.check_expr_value(lhs);
                let rhs_ty = self.check_expr_value(rhs);
                self.check_binop(*op, &lhs_ty, &rhs_ty, expr.span)
            }
            CheckExprKind::Coalesce { lhs, rhs } => self.check_coalesce(lhs, rhs, expr.span),
            CheckExprKind::CmpChain { first, rest } => {
                let mut lhs_ty = self.check_expr_value(first);
                for (op, rhs) in rest {
                    let rhs_ty = self.check_expr_value(rhs);
                    self.check_comparison(*op, &lhs_ty, &rhs_ty, rhs.span);
                    lhs_ty = rhs_ty;
                }
                InferredType::bool()
            }
            CheckExprKind::Field { expr: inner, name } => self.check_field(inner, name, expr.span),
            CheckExprKind::SafeField { expr: inner, name } => {
                self.check_safe_field(inner, name, expr.span)
            }
            CheckExprKind::Index { expr: inner, index } => {
                self.check_index(inner, index, expr.span)
            }
            CheckExprKind::SafeIndex { expr: inner, index } => {
                self.check_safe_index(inner, index, expr.span)
            }
            CheckExprKind::Is {
                expr: inner,
                predicate,
            } => {
                let inner_ty = self.check_expr_value(inner);
                self.check_is(&inner_ty, predicate, expr.span);
                InferredType::bool()
            }
            CheckExprKind::Call { name, args } => self.check_call(name, args, expr.span),
            CheckExprKind::MethodCall {
                receiver,
                name,
                args,
            } => self.check_method_call(receiver, name, args, expr.span),
        }
    }

    fn check_format_segments(&mut self, segments: &[CheckFormatSegment]) {
        for segment in segments {
            let CheckFormatSegment::Expr(expr) = segment else {
                continue;
            };
            let ty = self.check_expr_value(expr);
            if !is_formattable(&ty) {
                self.diag(
                    CftErrorCode::OperatorTypeMismatch,
                    expr.span,
                    "formatted string interpolation requires a scalar, enum, or null value",
                );
            }
        }
    }

    /// Like `check_expr`, but rejects bare enum-name references in operand
    /// positions (e.g. `Rarity > 5`). Without this guard, the plain
    /// `OperatorTypeMismatch` diagnostic would obscure the real mistake of
    /// using the enum type itself as a value.
    fn check_expr_value(&mut self, expr: &CheckExpr) -> InferredType {
        let ty = self.check_expr(expr);
        if let InferredType::EnumNamespace(name) = &ty {
            self.diag(
                CftErrorCode::OperatorTypeMismatch,
                expr.span,
                format!(
                    "enum type `{name}` cannot be used as a value; use `{name}.Variant` or `{name}(0)` instead",
                ),
            );
            return InferredType::Unknown;
        }
        ty
    }

    fn resolve_value_name(&mut self, name: &str, span: Span) -> InferredType {
        for scope in self.locals.iter().rev() {
            if let Some(ty) = scope.get(name) {
                return ty.clone();
            }
        }
        if let CheckScope::Record(type_name) = &self.scope {
            if let Some(fields) = self.compiler.full_fields.get(type_name) {
                if let Some(field) = fields.get(name) {
                    return field.inferred_type.clone();
                }
            }
            if name == "id" {
                return InferredType::string();
            }
        }
        if let Some(info) = self.compiler.consts.get(name) {
            return InferredType::from_const(&info.value);
        }
        if self.compiler.enums.contains_key(name) {
            return InferredType::EnumNamespace(crate::EnumName::from_validated(name.to_string()));
        }
        self.diag(
            CftErrorCode::UnknownValueName,
            span,
            format!("unknown value `{name}`"),
        );
        InferredType::Unknown
    }

    fn check_field(&mut self, inner: &CheckExpr, name: &NameRef, span: Span) -> InferredType {
        if let CheckExprKind::Name(enum_name) = &inner.kind {
            if let Some(enum_info) = self.compiler.enums.get(enum_name) {
                if enum_info.variants.contains(&name.name) {
                    return InferredType::enum_value(crate::EnumName::from_validated(
                        enum_name.clone(),
                    ));
                }
                self.diag(
                    CftErrorCode::TypeUnknownEnumVariant,
                    name.span,
                    format!("unknown enum variant `{}`", name.name),
                );
                return InferredType::Unknown;
            }
            if let Some(symbol) = self.compiler.symbols.get(enum_name) {
                if symbol.kind != SymbolKind::Enum {
                    self.diag(
                        CftErrorCode::TypeEnumVariantOnNonEnum,
                        inner.span,
                        "enum variant access used on a non-enum name",
                    );
                    return InferredType::Unknown;
                }
            }
        }

        let inner_ty = self.check_expr_value(inner);
        self.check_field_type(&unwrap_nullable(&inner_ty), name, span)
    }

    fn check_field_type(
        &mut self,
        inner_ty: &InferredType,
        name: &NameRef,
        span: Span,
    ) -> InferredType {
        match unwrap_reference(inner_ty) {
            InferredType::Value(CftValueType::Object(type_name)) => {
                if name.name == "id" {
                    return InferredType::string();
                }
                let type_known = self.compiler.full_fields.contains_key(type_name.as_str());
                let field_ty = self
                    .compiler
                    .full_fields
                    .get(type_name.as_str())
                    .and_then(|fields| fields.get(&name.name))
                    .map(|field| field.inferred_type.clone());
                if let Some(ty) = field_ty {
                    return ty;
                }
                if type_known {
                    self.diag(
                        CftErrorCode::UnknownField,
                        name.span,
                        format!("unknown field `{}`", name.name),
                    );
                }
                InferredType::Unknown
            }
            InferredType::Entry(key, value) => match name.name.as_str() {
                "key" => *key,
                "value" => *value,
                _ => {
                    self.diag(
                        CftErrorCode::UnknownField,
                        name.span,
                        "dict entry only has key and value fields",
                    );
                    InferredType::Unknown
                }
            },
            InferredType::Unknown => InferredType::Unknown,
            _ => {
                self.diag(
                    CftErrorCode::FieldAccessOnNonObject,
                    span,
                    "field access requires an object",
                );
                InferredType::Unknown
            }
        }
    }

    fn check_safe_field(&mut self, inner: &CheckExpr, name: &NameRef, span: Span) -> InferredType {
        let inner_ty = self.check_expr_value(inner);
        if !inner_ty.is_nullable() {
            if !inner_ty.is_unknown() {
                self.diag(
                    CftErrorCode::OperatorTypeMismatch,
                    inner.span,
                    "safe field access requires a nullable receiver",
                );
            }
            return InferredType::Unknown;
        }
        let projected = self.check_field_type(&unwrap_nullable(&inner_ty), name, span);
        InferredType::nullable(projected)
    }

    fn check_index(&mut self, inner: &CheckExpr, index: &CheckExpr, span: Span) -> InferredType {
        let inner_ty = self.check_expr_value(inner);
        self.check_index_type(&unwrap_nullable(&inner_ty), index, span)
    }

    fn check_index_type(
        &mut self,
        inner_ty: &InferredType,
        index: &CheckExpr,
        span: Span,
    ) -> InferredType {
        let index_ty = self.check_expr_value(index);
        if let Some(elem) = inner_ty.array_element() {
            if !types_comparable(&index_ty, &InferredType::int()) && !index_ty.is_unknown() {
                self.diag(
                    CftErrorCode::IndexTypeMismatch,
                    index.span,
                    "array index must be int",
                );
            }
            elem
        } else if let Some((key, value)) = inner_ty.dict_types() {
            if !types_comparable(&key, &index_ty) && !index_ty.is_unknown() {
                self.diag(
                    CftErrorCode::IndexTypeMismatch,
                    index.span,
                    "dict index type does not match key type",
                );
            }
            value
        } else if inner_ty.is_unknown() {
            InferredType::Unknown
        } else {
            self.diag(
                CftErrorCode::IndexOnNonIndexable,
                span,
                "index access requires an array or dict",
            );
            InferredType::Unknown
        }
    }

    fn check_safe_index(
        &mut self,
        inner: &CheckExpr,
        index: &CheckExpr,
        span: Span,
    ) -> InferredType {
        let inner_ty = self.check_expr_value(inner);
        if !inner_ty.is_nullable() {
            if !inner_ty.is_unknown() {
                self.diag(
                    CftErrorCode::OperatorTypeMismatch,
                    inner.span,
                    "safe index access requires a nullable receiver",
                );
            }
            self.check_expr_value(index);
            return InferredType::Unknown;
        }
        let projected = self.check_index_type(&unwrap_nullable(&inner_ty), index, span);
        InferredType::nullable(projected)
    }

    fn check_coalesce(&mut self, lhs: &CheckExpr, rhs: &CheckExpr, span: Span) -> InferredType {
        let lhs_ty = self.check_expr_value(lhs);
        let rhs_ty = self.check_expr_value(rhs);
        if !lhs_ty.is_nullable() {
            if !lhs_ty.is_unknown() {
                self.diag(
                    CftErrorCode::OperatorTypeMismatch,
                    span,
                    "left operand of `??` must be nullable",
                );
            }
            return InferredType::Unknown;
        }
        let result = unwrap_nullable(&lhs_ty);
        if !types_assignable(&result, &rhs_ty) {
            self.diag(
                CftErrorCode::OperatorTypeMismatch,
                rhs.span,
                "right operand of `??` must match the non-null left operand type",
            );
        }
        result
    }

    fn check_is(&mut self, lhs: &InferredType, predicate: &TypePredicate, span: Span) {
        match predicate {
            TypePredicate::Null(_) => {
                if !lhs.is_nullable() && !lhs.is_unknown() {
                    self.diag(
                        CftErrorCode::OperatorTypeMismatch,
                        span,
                        "`is null` requires a nullable operand",
                    );
                }
            }
            TypePredicate::Type(name) => {
                match self.compiler.symbols.get(&name.name) {
                    Some(symbol) if symbol.kind == SymbolKind::Type => {}
                    _ => {
                        self.diag(
                            CftErrorCode::InvalidIsPredicate,
                            name.span,
                            "is predicate must name a type or null",
                        );
                        return;
                    }
                }
                let operand = unwrap_reference(&unwrap_nullable(lhs));
                if operand.object_name().is_none() && !operand.is_unknown() {
                    self.diag(
                        CftErrorCode::OperatorTypeMismatch,
                        span,
                        "`is` type predicates require an object operand",
                    );
                }
            }
        }
    }

    fn expect_bool(&mut self, ty: &InferredType, span: Span) {
        if !types_comparable(ty, &InferredType::bool()) && !ty.is_unknown() {
            self.diag(
                CftErrorCode::ConditionMustBeBool,
                span,
                "check conditions must be bool",
            );
        }
    }

    fn diag(&mut self, code: CftErrorCode, span: Span, message: impl Into<String>) {
        self.compiler.diagnostics.push(CftDiagnostic::error(
            code,
            self.module.clone(),
            span,
            message,
        ));
    }
}
