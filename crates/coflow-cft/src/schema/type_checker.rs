use super::compiler::SchemaCompiler;
use super::support::{
    is_reserved_identifier, min_max_supported, ordered_comparable, types_comparable,
    unique_supported, unwrap_nullable, SymbolKind, Ty, TypeInfo,
};
use crate::ast::{
    BinOp, CheckExpr, CheckExprKind, CheckStmt, CmpOp, NameRef, TypePredicate, UnaryOp,
};
use crate::error::{CftDiagnostic, CftErrorCode};
use crate::span::Span;
use regex::Regex;
use std::collections::HashMap;

pub(super) struct TypeChecker<'a, 'b> {
    compiler: &'a mut SchemaCompiler<'b>,
    type_info: &'a TypeInfo<'b>,
    locals: Vec<HashMap<String, Ty>>,
}

impl<'a, 'b> TypeChecker<'a, 'b> {
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
                if is_reserved_identifier(&binding.name) {
                    self.diag(
                        CftErrorCode::ReservedIdentifier,
                        binding.span,
                        format!("`{}` is a reserved identifier", binding.name),
                    );
                }
                let col_ty = self.check_expr_value(collection);
                let item_ty = match unwrap_nullable(&col_ty) {
                    Ty::Array(inner) => *inner.clone(),
                    Ty::Dict(key, value) => Ty::Entry(key.clone(), value.clone()),
                    Ty::Unknown => Ty::Unknown,
                    _ => {
                        self.diag(
                            CftErrorCode::QuantifierRequiresCollection,
                            *span,
                            "quantifier target must be an array or dict",
                        );
                        Ty::Unknown
                    }
                };
                self.locals
                    .push(HashMap::from([(binding.name.clone(), item_ty)]));
                self.check_stmts(body);
                self.locals.pop();
            }
        }
    }

    fn check_expr(&mut self, expr: &CheckExpr) -> Ty {
        match &expr.kind {
            CheckExprKind::Int(_) => Ty::Int,
            CheckExprKind::Float(_) => Ty::Float,
            CheckExprKind::Bool(_) => Ty::Bool,
            CheckExprKind::Null => Ty::Null,
            CheckExprKind::String(_) => Ty::String,
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
                Ty::Bool
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
                Ty::Bool
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
    fn check_expr_value(&mut self, expr: &CheckExpr) -> Ty {
        let ty = self.check_expr(expr);
        if let Ty::EnumNamespace(name) = &ty {
            self.diag(
                CftErrorCode::OperatorTypeMismatch,
                expr.span,
                format!(
                    "enum type `{name}` cannot be used as a value; use `{name}.Variant` or `{name}(0)` instead",
                ),
            );
            return Ty::Unknown;
        }
        ty
    }

    fn resolve_value_name(&mut self, name: &str, span: Span) -> Ty {
        for scope in self.locals.iter().rev() {
            if let Some(ty) = scope.get(name) {
                return ty.clone();
            }
        }
        if let Some(fields) = self.compiler.full_fields.get(&self.type_info.def.name) {
            if let Some(field) = fields.get(name) {
                return field.check_ty.clone();
            }
        }
        if name == "id" {
            return Ty::String;
        }
        if let Some(info) = self.compiler.consts.get(name) {
            return Ty::from_const(&info.value);
        }
        if self.compiler.enums.contains_key(name) {
            return Ty::EnumNamespace(name.to_string());
        }
        self.diag(
            CftErrorCode::UnknownValueName,
            span,
            format!("unknown value `{name}`"),
        );
        Ty::Unknown
    }

    fn check_field(&mut self, inner: &CheckExpr, name: &NameRef, span: Span) -> Ty {
        if let CheckExprKind::Name(enum_name) = &inner.kind {
            if let Some(enum_info) = self.compiler.enums.get(enum_name) {
                if enum_info.variants.contains(&name.name) {
                    return Ty::Enum(enum_name.clone());
                }
                self.diag(
                    CftErrorCode::TypeUnknownEnumVariant,
                    name.span,
                    format!("unknown enum variant `{}`", name.name),
                );
                return Ty::Unknown;
            }
            if let Some(symbol) = self.compiler.symbols.get(enum_name) {
                if symbol.kind != SymbolKind::Enum {
                    self.diag(
                        CftErrorCode::TypeEnumVariantOnNonEnum,
                        inner.span,
                        "enum variant access used on a non-enum name",
                    );
                    return Ty::Unknown;
                }
            }
        }

        let inner_ty = self.check_expr_value(inner);
        match unwrap_nullable(&inner_ty) {
            Ty::Type(type_name) => {
                if name.name == "id" {
                    return Ty::String;
                }
                let type_known = self.compiler.full_fields.contains_key(type_name);
                let field_ty = self
                    .compiler
                    .full_fields
                    .get(type_name)
                    .and_then(|fields| fields.get(&name.name))
                    .map(|field| field.check_ty.clone());
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
                Ty::Unknown
            }
            Ty::Entry(key, value) => match name.name.as_str() {
                "key" => *key.clone(),
                "value" => *value.clone(),
                _ => {
                    self.diag(
                        CftErrorCode::UnknownField,
                        name.span,
                        "dict entry only has key and value fields",
                    );
                    Ty::Unknown
                }
            },
            Ty::Unknown => Ty::Unknown,
            _ => {
                self.diag(
                    CftErrorCode::FieldAccessOnNonObject,
                    span,
                    "field access requires an object",
                );
                Ty::Unknown
            }
        }
    }

    fn check_index(&mut self, inner: &CheckExpr, index: &CheckExpr, span: Span) -> Ty {
        let inner_ty = self.check_expr_value(inner);
        let index_ty = self.check_expr_value(index);
        match unwrap_nullable(&inner_ty) {
            Ty::Array(elem) => {
                if !types_comparable(&index_ty, &Ty::Int) && index_ty != Ty::Unknown {
                    self.diag(
                        CftErrorCode::IndexTypeMismatch,
                        index.span,
                        "array index must be int",
                    );
                }
                *elem.clone()
            }
            Ty::Dict(key, value) => {
                if !types_comparable(key, &index_ty) && index_ty != Ty::Unknown {
                    self.diag(
                        CftErrorCode::IndexTypeMismatch,
                        index.span,
                        "dict index type does not match key type",
                    );
                }
                *value.clone()
            }
            Ty::Unknown => Ty::Unknown,
            _ => {
                self.diag(
                    CftErrorCode::IndexOnNonIndexable,
                    span,
                    "index access requires an array or dict",
                );
                Ty::Unknown
            }
        }
    }

    fn check_is(&mut self, lhs: &Ty, predicate: &TypePredicate, span: Span) {
        match predicate {
            TypePredicate::Null(_) => {
                if !matches!(lhs, Ty::Nullable(_) | Ty::Unknown) {
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
                if !matches!(unwrap_nullable(lhs), Ty::Type(_) | Ty::Unknown) {
                    self.diag(
                        CftErrorCode::OperatorTypeMismatch,
                        span,
                        "`is` type predicates require an object operand",
                    );
                }
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    fn check_call(&mut self, name: &NameRef, args: &[CheckExpr], span: Span) -> Ty {
        if self.compiler.enums.contains_key(&name.name) {
            if args.len() != 1 {
                self.diag(
                    CftErrorCode::FunctionArityMismatch,
                    span,
                    "enum constructor expects one argument",
                );
                return Ty::Unknown;
            }
            let arg_ty = self.check_expr_value(&args[0]);
            if !types_comparable(&arg_ty, &Ty::Int) && arg_ty != Ty::Unknown {
                self.diag(
                    CftErrorCode::FunctionArgTypeMismatch,
                    args[0].span,
                    "enum constructor argument must be int",
                );
            }
            return Ty::Enum(name.name.clone());
        }

        self.diag(
            CftErrorCode::UnknownFunction,
            name.span,
            format!("unknown function `{}`", name.name),
        );
        for arg in args {
            self.check_expr_value(arg);
        }
        Ty::Unknown
    }

    #[allow(clippy::too_many_lines)]
    fn check_method_call(
        &mut self,
        receiver: &CheckExpr,
        name: &NameRef,
        args: &[CheckExpr],
        span: Span,
    ) -> Ty {
        let receiver_ty = self.check_expr_value(receiver);
        match name.name.as_str() {
            "len" => {
                if self.expect_arity(args, 0, span).is_err() {
                    return Ty::Unknown;
                }
                if !matches!(
                    unwrap_nullable(&receiver_ty),
                    Ty::Array(_) | Ty::Dict(_, _) | Ty::Unknown
                ) {
                    self.diag(
                        CftErrorCode::FunctionArgTypeMismatch,
                        receiver.span,
                        "len expects an array or dict",
                    );
                }
                Ty::Int
            }
            "contains" => {
                if self.expect_arity(args, 1, span).is_err() {
                    return Ty::Bool;
                }
                let value_ty = self.check_expr_value(&args[0]);
                match unwrap_nullable(&receiver_ty) {
                    Ty::Array(elem) => {
                        if !types_comparable(elem, &value_ty) && value_ty != Ty::Unknown {
                            self.diag(
                                CftErrorCode::FunctionArgTypeMismatch,
                                args[0].span,
                                "contains value type does not match array element type",
                            );
                        }
                    }
                    Ty::Dict(key, _) => {
                        if !types_comparable(key, &value_ty) && value_ty != Ty::Unknown {
                            self.diag(
                                CftErrorCode::FunctionArgTypeMismatch,
                                args[0].span,
                                "contains value type does not match dict key type",
                            );
                        }
                    }
                    Ty::Unknown => {}
                    _ => self.diag(
                        CftErrorCode::FunctionArgTypeMismatch,
                        receiver.span,
                        "contains expects an array or dict",
                    ),
                }
                Ty::Bool
            }
            "unique" => {
                if self.expect_arity(args, 0, span).is_err() {
                    return Ty::Bool;
                }
                match unwrap_nullable(&receiver_ty) {
                    Ty::Array(elem) if unique_supported(elem) => {}
                    Ty::Array(_) => self.diag(
                        CftErrorCode::UniqueUnsupportedElementType,
                        receiver.span,
                        "unique does not support this element type",
                    ),
                    Ty::Unknown => {}
                    _ => self.diag(
                        CftErrorCode::FunctionArgTypeMismatch,
                        receiver.span,
                        "unique expects an array",
                    ),
                }
                Ty::Bool
            }
            "min" | "max" => {
                if self.expect_arity(args, 0, span).is_err() {
                    return Ty::Unknown;
                }
                match unwrap_nullable(&receiver_ty) {
                    Ty::Array(elem) if min_max_supported(elem) => unwrap_nullable(elem).clone(),
                    Ty::Array(_) => {
                        self.diag(
                            CftErrorCode::FunctionArgTypeMismatch,
                            receiver.span,
                            "min/max expects int, float, or enum arrays",
                        );
                        Ty::Unknown
                    }
                    Ty::Unknown => Ty::Unknown,
                    _ => {
                        self.diag(
                            CftErrorCode::FunctionArgTypeMismatch,
                            receiver.span,
                            "min/max expects an array",
                        );
                        Ty::Unknown
                    }
                }
            }
            "sum" => {
                if self.expect_arity(args, 0, span).is_err() {
                    return Ty::Unknown;
                }
                match unwrap_nullable(&receiver_ty) {
                    Ty::Array(elem) => match unwrap_nullable(elem) {
                        Ty::Int | Ty::Float => unwrap_nullable(elem).clone(),
                        _ => {
                            self.diag(
                                CftErrorCode::FunctionArgTypeMismatch,
                                receiver.span,
                                "sum expects an int or float array",
                            );
                            Ty::Unknown
                        }
                    },
                    Ty::Unknown => Ty::Unknown,
                    _ => {
                        self.diag(
                            CftErrorCode::FunctionArgTypeMismatch,
                            receiver.span,
                            "sum expects an array",
                        );
                        Ty::Unknown
                    }
                }
            }
            "keys" => {
                if self.expect_arity(args, 0, span).is_err() {
                    return Ty::Unknown;
                }
                match unwrap_nullable(&receiver_ty) {
                    Ty::Dict(key, _) => Ty::Array(key.clone()),
                    Ty::Unknown => Ty::Unknown,
                    _ => {
                        self.diag(
                            CftErrorCode::FunctionArgTypeMismatch,
                            receiver.span,
                            "keys expects a dict",
                        );
                        Ty::Unknown
                    }
                }
            }
            "values" => {
                if self.expect_arity(args, 0, span).is_err() {
                    return Ty::Unknown;
                }
                match unwrap_nullable(&receiver_ty) {
                    Ty::Dict(_, value) => Ty::Array(value.clone()),
                    Ty::Unknown => Ty::Unknown,
                    _ => {
                        self.diag(
                            CftErrorCode::FunctionArgTypeMismatch,
                            receiver.span,
                            "values expects a dict",
                        );
                        Ty::Unknown
                    }
                }
            }
            "matches" => {
                if self.expect_arity(args, 1, span).is_err() {
                    return Ty::Bool;
                }
                if !types_comparable(&receiver_ty, &Ty::String) && receiver_ty != Ty::Unknown {
                    self.diag(
                        CftErrorCode::FunctionArgTypeMismatch,
                        receiver.span,
                        "matches first argument must be string",
                    );
                }
                if let CheckExprKind::String(pattern) = &args[0].kind {
                    if Regex::new(pattern).is_err() {
                        self.diag(
                            CftErrorCode::InvalidRegexPattern,
                            args[0].span,
                            "regex pattern cannot be compiled",
                        );
                    }
                } else {
                    let _ = self.check_expr_value(&args[0]);
                    self.diag(
                        CftErrorCode::RegexPatternMustBeLiteral,
                        args[0].span,
                        "matches pattern must be a string literal",
                    );
                }
                Ty::Bool
            }
            _ => {
                self.diag(
                    CftErrorCode::UnknownFunction,
                    name.span,
                    format!("unknown function `{}`", name.name),
                );
                for arg in args {
                    let _ = self.check_expr_value(arg);
                }
                Ty::Unknown
            }
        }
    }

    fn check_unary(&mut self, op: UnaryOp, ty: &Ty, span: Span) -> Ty {
        if *ty == Ty::Unknown {
            return Ty::Unknown;
        }
        let unwrapped = unwrap_nullable(ty);
        match op {
            UnaryOp::Not if matches!(unwrapped, Ty::Bool) => Ty::Bool,
            UnaryOp::Neg | UnaryOp::BitNot if matches!(unwrapped, Ty::Int) => Ty::Int,
            UnaryOp::Neg if matches!(unwrapped, Ty::Float) => Ty::Float,
            UnaryOp::BitNot if self.is_flag_enum(ty) => ty.clone(),
            UnaryOp::BitNot => {
                self.diag(
                    CftErrorCode::BitwiseRequiresIntOrFlagEnum,
                    span,
                    "bitwise not requires int or flag enum",
                );
                Ty::Unknown
            }
            _ => {
                self.diag(
                    CftErrorCode::OperatorTypeMismatch,
                    span,
                    "unary operator does not support this operand type",
                );
                Ty::Unknown
            }
        }
    }

    fn check_binop(&mut self, op: BinOp, lhs: &Ty, rhs: &Ty, span: Span) -> Ty {
        match op {
            BinOp::Or | BinOp::And => {
                if (!types_comparable(lhs, &Ty::Bool) || !types_comparable(rhs, &Ty::Bool))
                    && *lhs != Ty::Unknown
                    && *rhs != Ty::Unknown
                {
                    self.diag(
                        CftErrorCode::OperatorTypeMismatch,
                        span,
                        "logical operators require bool operands",
                    );
                }
                Ty::Bool
            }
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Pow => {
                if types_comparable(lhs, &Ty::Int) && types_comparable(rhs, &Ty::Int) {
                    Ty::Int
                } else if types_comparable(lhs, &Ty::Float) && types_comparable(rhs, &Ty::Float) {
                    Ty::Float
                } else {
                    self.operator_mismatch(lhs, rhs, span);
                    Ty::Unknown
                }
            }
            BinOp::IntDiv | BinOp::Mod => {
                if types_comparable(lhs, &Ty::Int) && types_comparable(rhs, &Ty::Int) {
                    Ty::Int
                } else {
                    self.operator_mismatch(lhs, rhs, span);
                    Ty::Unknown
                }
            }
            BinOp::Shl | BinOp::Shr => {
                if types_comparable(lhs, &Ty::Int) && types_comparable(rhs, &Ty::Int) {
                    Ty::Int
                } else {
                    self.diag(
                        CftErrorCode::ShiftRequiresInt,
                        span,
                        "shift operators require int operands",
                    );
                    Ty::Unknown
                }
            }
            BinOp::BitOr | BinOp::BitXor | BinOp::BitAnd => {
                if types_comparable(lhs, &Ty::Int) && types_comparable(rhs, &Ty::Int) {
                    Ty::Int
                } else if types_comparable(lhs, rhs) && self.is_flag_enum(lhs) {
                    lhs.clone()
                } else {
                    self.diag(
                        CftErrorCode::BitwiseRequiresIntOrFlagEnum,
                        span,
                        "bitwise operators require int or the same flag enum",
                    );
                    Ty::Unknown
                }
            }
        }
    }

    fn check_comparison(&mut self, op: CmpOp, lhs: &Ty, rhs: &Ty, span: Span) -> Ty {
        let ok = match op {
            CmpOp::Eq | CmpOp::Ne => types_comparable(lhs, rhs),
            CmpOp::Lt | CmpOp::Le | CmpOp::Gt | CmpOp::Ge => ordered_comparable(lhs, rhs),
        };
        if !ok && *lhs != Ty::Unknown && *rhs != Ty::Unknown {
            self.diag(
                CftErrorCode::ComparisonTypeMismatch,
                span,
                "comparison operands are not compatible",
            );
        }
        Ty::Bool
    }

    fn expect_bool(&mut self, ty: &Ty, span: Span) {
        if !types_comparable(ty, &Ty::Bool) && *ty != Ty::Unknown {
            self.diag(
                CftErrorCode::ConditionMustBeBool,
                span,
                "check conditions must be bool",
            );
        }
    }

    fn expect_arity(&mut self, args: &[CheckExpr], expected: usize, span: Span) -> Result<(), ()> {
        if args.len() == expected {
            Ok(())
        } else {
            self.diag(
                CftErrorCode::FunctionArityMismatch,
                span,
                format!("expected {expected} argument(s)"),
            );
            Err(())
        }
    }

    fn operator_mismatch(&mut self, lhs: &Ty, rhs: &Ty, span: Span) {
        if *lhs != Ty::Unknown && *rhs != Ty::Unknown {
            self.diag(
                CftErrorCode::OperatorTypeMismatch,
                span,
                "operator does not support these operand types",
            );
        }
    }

    fn is_flag_enum(&self, ty: &Ty) -> bool {
        let Ty::Enum(name) = unwrap_nullable(ty) else {
            return false;
        };
        self.compiler
            .enums
            .get(name)
            .is_some_and(|info| info.is_flag)
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
