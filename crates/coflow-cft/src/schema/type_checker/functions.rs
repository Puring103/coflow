use super::TypeChecker;
use crate::ast::{CheckExpr, CheckExprKind, NameRef};
use crate::error::CftErrorCode;
use crate::schema::support::{
    min_max_supported, types_comparable, unique_supported, unwrap_nullable, Ty,
};
use crate::span::Span;
use regex::Regex;

impl TypeChecker<'_, '_> {
    pub(super) fn check_call(&mut self, name: &NameRef, args: &[CheckExpr], span: Span) -> Ty {
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

    pub(super) fn check_method_call(
        &mut self,
        receiver: &CheckExpr,
        name: &NameRef,
        args: &[CheckExpr],
        span: Span,
    ) -> Ty {
        let receiver_ty = self.check_expr_value(receiver);
        match name.name.as_str() {
            "len" => self.check_len_method(receiver, args, span, &receiver_ty),
            "contains" => self.check_contains_method(receiver, args, span, &receiver_ty),
            "isUnique" => self.check_unique_method(receiver, args, span, &receiver_ty),
            "min" | "max" => self.check_min_max_method(receiver, args, span, &receiver_ty),
            "sum" => self.check_sum_method(receiver, args, span, &receiver_ty),
            "keys" => self.check_keys_method(receiver, args, span, &receiver_ty),
            "values" => self.check_values_method(receiver, args, span, &receiver_ty),
            "matches" => self.check_matches_method(receiver, args, span, &receiver_ty),
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

    fn check_len_method(
        &mut self,
        receiver: &CheckExpr,
        args: &[CheckExpr],
        span: Span,
        receiver_ty: &Ty,
    ) -> Ty {
        if self.expect_arity(args, 0, span).is_err() {
            return Ty::Unknown;
        }
        if !matches!(
            unwrap_nullable(receiver_ty),
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

    fn check_contains_method(
        &mut self,
        receiver: &CheckExpr,
        args: &[CheckExpr],
        span: Span,
        receiver_ty: &Ty,
    ) -> Ty {
        if self.expect_arity(args, 1, span).is_err() {
            return Ty::Bool;
        }
        let value_ty = self.check_expr_value(&args[0]);
        match unwrap_nullable(receiver_ty) {
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

    fn check_unique_method(
        &mut self,
        receiver: &CheckExpr,
        args: &[CheckExpr],
        span: Span,
        receiver_ty: &Ty,
    ) -> Ty {
        if self.expect_arity(args, 0, span).is_err() {
            return Ty::Bool;
        }
        match unwrap_nullable(receiver_ty) {
            Ty::Array(elem) if unique_supported(elem) => {}
            Ty::Array(_) => self.diag(
                CftErrorCode::UniqueUnsupportedElementType,
                receiver.span,
                "isUnique does not support this element type",
            ),
            Ty::Unknown => {}
            _ => self.diag(
                CftErrorCode::FunctionArgTypeMismatch,
                receiver.span,
                "isUnique expects an array",
            ),
        }
        Ty::Bool
    }

    fn check_min_max_method(
        &mut self,
        receiver: &CheckExpr,
        args: &[CheckExpr],
        span: Span,
        receiver_ty: &Ty,
    ) -> Ty {
        if self.expect_arity(args, 0, span).is_err() {
            return Ty::Unknown;
        }
        match unwrap_nullable(receiver_ty) {
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

    fn check_sum_method(
        &mut self,
        receiver: &CheckExpr,
        args: &[CheckExpr],
        span: Span,
        receiver_ty: &Ty,
    ) -> Ty {
        if self.expect_arity(args, 0, span).is_err() {
            return Ty::Unknown;
        }
        match unwrap_nullable(receiver_ty) {
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

    fn check_keys_method(
        &mut self,
        receiver: &CheckExpr,
        args: &[CheckExpr],
        span: Span,
        receiver_ty: &Ty,
    ) -> Ty {
        if self.expect_arity(args, 0, span).is_err() {
            return Ty::Unknown;
        }
        match unwrap_nullable(receiver_ty) {
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

    fn check_values_method(
        &mut self,
        receiver: &CheckExpr,
        args: &[CheckExpr],
        span: Span,
        receiver_ty: &Ty,
    ) -> Ty {
        if self.expect_arity(args, 0, span).is_err() {
            return Ty::Unknown;
        }
        match unwrap_nullable(receiver_ty) {
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

    fn check_matches_method(
        &mut self,
        receiver: &CheckExpr,
        args: &[CheckExpr],
        span: Span,
        receiver_ty: &Ty,
    ) -> Ty {
        if self.expect_arity(args, 1, span).is_err() {
            return Ty::Bool;
        }
        if !types_comparable(receiver_ty, &Ty::String) && *receiver_ty != Ty::Unknown {
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
}
