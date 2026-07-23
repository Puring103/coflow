#[path = "check_functions.rs"]
mod functions;
#[path = "check_operators.rs"]
mod operators;

use super::inferred_type::{types_comparable, unwrap_nullable, unwrap_reference, InferredType};
use super::state::{SymbolKind, TypeInfo};
use super::SchemaCompiler;
use crate::diagnostics::{CftDiagnostic, CftErrorCode};
use crate::schema::CftValueType;
use crate::syntax::ast::{CheckExpr, CheckExprKind, CheckStmt, NameRef, TypePredicate};
use crate::syntax::Span;
use std::collections::HashMap;

pub(super) struct CheckTypeAnalyzer<'a, 'b> {
    compiler: &'a mut SchemaCompiler<'b>,
    type_info: &'a TypeInfo<'b>,
    locals: Vec<HashMap<String, InferredType>>,
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
            CheckStmt::Expr { condition, .. } => {
                let ty = self.check_expr_value(condition);
                self.expect_bool(&ty, condition.span);
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
                let col_ty = unwrap_nullable(&col_ty);
                let item_ty = match col_ty {
                    InferredType::Value(CftValueType::Array(inner)) => InferredType::Value(*inner),
                    InferredType::Value(CftValueType::Dict(key, value)) => InferredType::Entry(
                        Box::new(InferredType::Value(*key)),
                        Box::new(InferredType::Value(*value)),
                    ),
                    InferredType::Unknown => InferredType::Unknown,
                    _ => {
                        self.diag(
                            CftErrorCode::QuantifierRequiresCollection,
                            *span,
                            "quantifier target must be an array or dict",
                        );
                        InferredType::Unknown
                    }
                };
                self.locals
                    .push(HashMap::from([(binding.name.clone(), item_ty)]));
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
                InferredType::bool()
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
        if let Some(fields) = self.compiler.full_fields.get(&self.type_info.def.name) {
            if let Some(field) = fields.get(name) {
                return field.inferred_type.clone();
            }
        }
        if name == "id" {
            return InferredType::string();
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
        match unwrap_reference(&unwrap_nullable(&inner_ty)) {
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

    fn check_index(&mut self, inner: &CheckExpr, index: &CheckExpr, span: Span) -> InferredType {
        let inner_ty = self.check_expr_value(inner);
        let index_ty = self.check_expr_value(index);
        let inner_ty = unwrap_nullable(&inner_ty);
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
            self.type_info.module.clone(),
            span,
            message,
        ));
    }
}
