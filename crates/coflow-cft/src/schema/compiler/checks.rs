#[path = "check_functions.rs"]
mod functions;
#[path = "check_operators.rs"]
mod operators;

use super::checked_type::{types_comparable, unwrap_nullable, unwrap_reference, CheckedType};
use super::state::{SymbolKind, TypeInfo};
use super::SchemaCompiler;
use crate::diagnostics::{CftDiagnostic, CftErrorCode};
use crate::syntax::ast::{CheckExpr, CheckExprKind, CheckStmt, NameRef, TypePredicate};
use crate::syntax::Span;
use std::collections::HashMap;

pub(super) struct CheckTypeAnalyzer<'a, 'b> {
    compiler: &'a mut SchemaCompiler<'b>,
    type_info: &'a TypeInfo<'b>,
    locals: Vec<HashMap<String, CheckedType>>,
}

impl<'a, 'b> CheckTypeAnalyzer<'a, 'b> {
    pub(super) fn new(compiler: &'a mut SchemaCompiler<'b>, type_info: &'a TypeInfo<'b>) -> Self {
        Self {
            compiler,
            type_info,
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
            CheckStmt::Expr(expr) => {
                let ty = self.check_expr_value(expr);
                self.expect_bool(&ty, expr.span);
            }
            CheckStmt::When {
                condition, body, ..
            } => {
                let ty = self.check_expr_value(condition);
                self.expect_bool(&ty, condition.span);
                self.check_stmts(body);
            }
            CheckStmt::Quantifier {
                binding,
                collection,
                body,
                span,
                ..
            } => {
                if crate::is_cft_reserved_identifier(&binding.name) {
                    self.diag(
                        CftErrorCode::ReservedIdentifier,
                        binding.span,
                        format!("`{}` is a reserved identifier", binding.name),
                    );
                }
                let col_ty = self.check_expr_value(collection);
                let item_ty = match unwrap_nullable(&col_ty) {
                    CheckedType::Array(inner) => *inner.clone(),
                    CheckedType::Dict(key, value) => CheckedType::Entry(key.clone(), value.clone()),
                    CheckedType::Unknown => CheckedType::Unknown,
                    _ => {
                        self.diag(
                            CftErrorCode::QuantifierRequiresCollection,
                            *span,
                            "quantifier target must be an array or dict",
                        );
                        CheckedType::Unknown
                    }
                };
                self.locals
                    .push(HashMap::from([(binding.name.clone(), item_ty)]));
                self.check_stmts(body);
                self.locals.pop();
            }
        }
    }

    fn check_expr(&mut self, expr: &CheckExpr) -> CheckedType {
        match &expr.kind {
            CheckExprKind::Int(_) => CheckedType::Int,
            CheckExprKind::Float(_) => CheckedType::Float,
            CheckExprKind::Bool(_) => CheckedType::Bool,
            CheckExprKind::Null => CheckedType::Null,
            CheckExprKind::String(_) => CheckedType::String,
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
            CheckExprKind::CmpChain { first, rest } => {
                let mut lhs_ty = self.check_expr_value(first);
                for (op, rhs) in rest {
                    let rhs_ty = self.check_expr_value(rhs);
                    self.check_comparison(*op, &lhs_ty, &rhs_ty, rhs.span);
                    lhs_ty = rhs_ty;
                }
                CheckedType::Bool
            }
            CheckExprKind::Field { expr: inner, name } => self.check_field(inner, name, expr.span),
            CheckExprKind::Index { expr: inner, index } => {
                self.check_index(inner, index, expr.span)
            }
            CheckExprKind::Is {
                expr: inner,
                predicate,
            } => {
                let inner_ty = self.check_expr_value(inner);
                self.check_is(&inner_ty, predicate, expr.span);
                CheckedType::Bool
            }
            CheckExprKind::Call { name, args } => self.check_call(name, args, expr.span),
            CheckExprKind::MethodCall {
                receiver,
                name,
                args,
            } => self.check_method_call(receiver, name, args, expr.span),
        }
    }

    /// Like `check_expr`, but rejects bare enum-name references in operand
    /// positions (e.g. `Rarity > 5`). Without this guard, the plain
    /// `OperatorTypeMismatch` diagnostic would obscure the real mistake of
    /// using the enum type itself as a value.
    fn check_expr_value(&mut self, expr: &CheckExpr) -> CheckedType {
        let ty = self.check_expr(expr);
        if let CheckedType::EnumNamespace(name) = &ty {
            self.diag(
                CftErrorCode::OperatorTypeMismatch,
                expr.span,
                format!(
                    "enum type `{name}` cannot be used as a value; use `{name}.Variant` or `{name}(0)` instead",
                ),
            );
            return CheckedType::Unknown;
        }
        ty
    }

    fn resolve_value_name(&mut self, name: &str, span: Span) -> CheckedType {
        for scope in self.locals.iter().rev() {
            if let Some(ty) = scope.get(name) {
                return ty.clone();
            }
        }
        if let Some(fields) = self.compiler.full_fields.get(&self.type_info.def.name) {
            if let Some(field) = fields.get(name) {
                return field.checked_type.clone();
            }
        }
        if name == "id" {
            return CheckedType::String;
        }
        if let Some(info) = self.compiler.consts.get(name) {
            return CheckedType::from_const(&info.value);
        }
        if self.compiler.enums.contains_key(name) {
            return CheckedType::EnumNamespace(name.to_string());
        }
        self.diag(
            CftErrorCode::UnknownValueName,
            span,
            format!("unknown value `{name}`"),
        );
        CheckedType::Unknown
    }

    fn check_field(&mut self, inner: &CheckExpr, name: &NameRef, span: Span) -> CheckedType {
        if let CheckExprKind::Name(enum_name) = &inner.kind {
            if let Some(enum_info) = self.compiler.enums.get(enum_name) {
                if enum_info.variants.contains(&name.name) {
                    return CheckedType::Enum(enum_name.clone());
                }
                self.diag(
                    CftErrorCode::TypeUnknownEnumVariant,
                    name.span,
                    format!("unknown enum variant `{}`", name.name),
                );
                return CheckedType::Unknown;
            }
            if let Some(symbol) = self.compiler.symbols.get(enum_name) {
                if symbol.kind != SymbolKind::Enum {
                    self.diag(
                        CftErrorCode::TypeEnumVariantOnNonEnum,
                        inner.span,
                        "enum variant access used on a non-enum name",
                    );
                    return CheckedType::Unknown;
                }
            }
        }

        let inner_ty = self.check_expr_value(inner);
        match unwrap_reference(unwrap_nullable(&inner_ty)) {
            CheckedType::Type(type_name) => {
                if name.name == "id" {
                    return CheckedType::String;
                }
                let type_known = self.compiler.full_fields.contains_key(type_name);
                let field_ty = self
                    .compiler
                    .full_fields
                    .get(type_name)
                    .and_then(|fields| fields.get(&name.name))
                    .map(|field| field.checked_type.clone());
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
                CheckedType::Unknown
            }
            CheckedType::Entry(key, value) => match name.name.as_str() {
                "key" => *key.clone(),
                "value" => *value.clone(),
                _ => {
                    self.diag(
                        CftErrorCode::UnknownField,
                        name.span,
                        "dict entry only has key and value fields",
                    );
                    CheckedType::Unknown
                }
            },
            CheckedType::Unknown => CheckedType::Unknown,
            _ => {
                self.diag(
                    CftErrorCode::FieldAccessOnNonObject,
                    span,
                    "field access requires an object",
                );
                CheckedType::Unknown
            }
        }
    }

    fn check_index(&mut self, inner: &CheckExpr, index: &CheckExpr, span: Span) -> CheckedType {
        let inner_ty = self.check_expr_value(inner);
        let index_ty = self.check_expr_value(index);
        match unwrap_nullable(&inner_ty) {
            CheckedType::Array(elem) => {
                if !types_comparable(&index_ty, &CheckedType::Int)
                    && index_ty != CheckedType::Unknown
                {
                    self.diag(
                        CftErrorCode::IndexTypeMismatch,
                        index.span,
                        "array index must be int",
                    );
                }
                *elem.clone()
            }
            CheckedType::Dict(key, value) => {
                if !types_comparable(key, &index_ty) && index_ty != CheckedType::Unknown {
                    self.diag(
                        CftErrorCode::IndexTypeMismatch,
                        index.span,
                        "dict index type does not match key type",
                    );
                }
                *value.clone()
            }
            CheckedType::Unknown => CheckedType::Unknown,
            _ => {
                self.diag(
                    CftErrorCode::IndexOnNonIndexable,
                    span,
                    "index access requires an array or dict",
                );
                CheckedType::Unknown
            }
        }
    }

    fn check_is(&mut self, lhs: &CheckedType, predicate: &TypePredicate, span: Span) {
        match predicate {
            TypePredicate::Null(_) => {
                if !matches!(lhs, CheckedType::Nullable(_) | CheckedType::Unknown) {
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
                if !matches!(
                    unwrap_reference(unwrap_nullable(lhs)),
                    CheckedType::Type(_) | CheckedType::Unknown
                ) {
                    self.diag(
                        CftErrorCode::OperatorTypeMismatch,
                        span,
                        "`is` type predicates require an object operand",
                    );
                }
            }
        }
    }

    fn expect_bool(&mut self, ty: &CheckedType, span: Span) {
        if !types_comparable(ty, &CheckedType::Bool) && *ty != CheckedType::Unknown {
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
            self.type_info.module.clone(),
            span,
            message,
        ));
    }
}
