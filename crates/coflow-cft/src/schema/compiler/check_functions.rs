use super::CheckTypeAnalyzer;
use crate::diagnostics::CftErrorCode;
use crate::schema::compiler::checked_type::{
    min_max_supported, types_comparable, unique_supported, unwrap_nullable, CheckedType,
};
use crate::syntax::ast::{CheckExpr, CheckExprKind, NameRef};
use crate::syntax::Span;
use regex::Regex;

impl CheckTypeAnalyzer<'_, '_> {
    pub(super) fn check_call(
        &mut self,
        name: &NameRef,
        args: &[CheckExpr],
        span: Span,
    ) -> CheckedType {
        if self.compiler.enums.contains_key(&name.name) {
            if args.len() != 1 {
                self.diag(
                    CftErrorCode::FunctionArityMismatch,
                    span,
                    "enum constructor expects one argument",
                );
                return CheckedType::Unknown;
            }
            let arg_ty = self.check_expr_value(&args[0]);
            if !types_comparable(&arg_ty, &CheckedType::Int) && arg_ty != CheckedType::Unknown {
                self.diag(
                    CftErrorCode::FunctionArgTypeMismatch,
                    args[0].span,
                    "enum constructor argument must be int",
                );
            }
            return CheckedType::Enum(name.name.clone());
        }

        self.diag(
            CftErrorCode::UnknownFunction,
            name.span,
            format!("unknown function `{}`", name.name),
        );
        for arg in args {
            self.check_expr_value(arg);
        }
        CheckedType::Unknown
    }

    pub(super) fn check_method_call(
        &mut self,
        receiver: &CheckExpr,
        name: &NameRef,
        args: &[CheckExpr],
        span: Span,
    ) -> CheckedType {
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
                CheckedType::Unknown
            }
        }
    }

    fn check_len_method(
        &mut self,
        receiver: &CheckExpr,
        args: &[CheckExpr],
        span: Span,
        receiver_ty: &CheckedType,
    ) -> CheckedType {
        if self.expect_arity(args, 0, span).is_err() {
            return CheckedType::Unknown;
        }
        if !matches!(
            unwrap_nullable(receiver_ty),
            CheckedType::Array(_) | CheckedType::Dict(_, _) | CheckedType::Unknown
        ) {
            self.diag(
                CftErrorCode::FunctionArgTypeMismatch,
                receiver.span,
                "len expects an array or dict",
            );
        }
        CheckedType::Int
    }

    fn check_contains_method(
        &mut self,
        receiver: &CheckExpr,
        args: &[CheckExpr],
        span: Span,
        receiver_ty: &CheckedType,
    ) -> CheckedType {
        if self.expect_arity(args, 1, span).is_err() {
            return CheckedType::Bool;
        }
        let value_ty = self.check_expr_value(&args[0]);
        match unwrap_nullable(receiver_ty) {
            CheckedType::Array(elem) => {
                if !types_comparable(elem, &value_ty) && value_ty != CheckedType::Unknown {
                    self.diag(
                        CftErrorCode::FunctionArgTypeMismatch,
                        args[0].span,
                        "contains value type does not match array element type",
                    );
                }
            }
            CheckedType::Dict(key, _) => {
                if !types_comparable(key, &value_ty) && value_ty != CheckedType::Unknown {
                    self.diag(
                        CftErrorCode::FunctionArgTypeMismatch,
                        args[0].span,
                        "contains value type does not match dict key type",
                    );
                }
            }
            CheckedType::Unknown => {}
            _ => self.diag(
                CftErrorCode::FunctionArgTypeMismatch,
                receiver.span,
                "contains expects an array or dict",
            ),
        }
        CheckedType::Bool
    }

    fn check_unique_method(
        &mut self,
        receiver: &CheckExpr,
        args: &[CheckExpr],
        span: Span,
        receiver_ty: &CheckedType,
    ) -> CheckedType {
        if self.expect_arity(args, 0, span).is_err() {
            return CheckedType::Bool;
        }
        match unwrap_nullable(receiver_ty) {
            CheckedType::Array(elem) if unique_supported(elem) => {}
            CheckedType::Array(_) => self.diag(
                CftErrorCode::UniqueUnsupportedElementType,
                receiver.span,
                "isUnique does not support this element type",
            ),
            CheckedType::Unknown => {}
            _ => self.diag(
                CftErrorCode::FunctionArgTypeMismatch,
                receiver.span,
                "isUnique expects an array",
            ),
        }
        CheckedType::Bool
    }

    fn check_min_max_method(
        &mut self,
        receiver: &CheckExpr,
        args: &[CheckExpr],
        span: Span,
        receiver_ty: &CheckedType,
    ) -> CheckedType {
        if self.expect_arity(args, 0, span).is_err() {
            return CheckedType::Unknown;
        }
        match unwrap_nullable(receiver_ty) {
            CheckedType::Array(elem) if min_max_supported(elem) => unwrap_nullable(elem).clone(),
            CheckedType::Array(_) => {
                self.diag(
                    CftErrorCode::FunctionArgTypeMismatch,
                    receiver.span,
                    "min/max expects int, float, or enum arrays",
                );
                CheckedType::Unknown
            }
            CheckedType::Unknown => CheckedType::Unknown,
            _ => {
                self.diag(
                    CftErrorCode::FunctionArgTypeMismatch,
                    receiver.span,
                    "min/max expects an array",
                );
                CheckedType::Unknown
            }
        }
    }

    fn check_sum_method(
        &mut self,
        receiver: &CheckExpr,
        args: &[CheckExpr],
        span: Span,
        receiver_ty: &CheckedType,
    ) -> CheckedType {
        if self.expect_arity(args, 0, span).is_err() {
            return CheckedType::Unknown;
        }
        match unwrap_nullable(receiver_ty) {
            CheckedType::Array(elem) => match unwrap_nullable(elem) {
                CheckedType::Int | CheckedType::Float => unwrap_nullable(elem).clone(),
                _ => {
                    self.diag(
                        CftErrorCode::FunctionArgTypeMismatch,
                        receiver.span,
                        "sum expects an int or float array",
                    );
                    CheckedType::Unknown
                }
            },
            CheckedType::Unknown => CheckedType::Unknown,
            _ => {
                self.diag(
                    CftErrorCode::FunctionArgTypeMismatch,
                    receiver.span,
                    "sum expects an array",
                );
                CheckedType::Unknown
            }
        }
    }

    fn check_keys_method(
        &mut self,
        receiver: &CheckExpr,
        args: &[CheckExpr],
        span: Span,
        receiver_ty: &CheckedType,
    ) -> CheckedType {
        if self.expect_arity(args, 0, span).is_err() {
            return CheckedType::Unknown;
        }
        match unwrap_nullable(receiver_ty) {
            CheckedType::Dict(key, _) => CheckedType::Array(key.clone()),
            CheckedType::Unknown => CheckedType::Unknown,
            _ => {
                self.diag(
                    CftErrorCode::FunctionArgTypeMismatch,
                    receiver.span,
                    "keys expects a dict",
                );
                CheckedType::Unknown
            }
        }
    }

    fn check_values_method(
        &mut self,
        receiver: &CheckExpr,
        args: &[CheckExpr],
        span: Span,
        receiver_ty: &CheckedType,
    ) -> CheckedType {
        if self.expect_arity(args, 0, span).is_err() {
            return CheckedType::Unknown;
        }
        match unwrap_nullable(receiver_ty) {
            CheckedType::Dict(_, value) => CheckedType::Array(value.clone()),
            CheckedType::Unknown => CheckedType::Unknown,
            _ => {
                self.diag(
                    CftErrorCode::FunctionArgTypeMismatch,
                    receiver.span,
                    "values expects a dict",
                );
                CheckedType::Unknown
            }
        }
    }

    fn check_matches_method(
        &mut self,
        receiver: &CheckExpr,
        args: &[CheckExpr],
        span: Span,
        receiver_ty: &CheckedType,
    ) -> CheckedType {
        if self.expect_arity(args, 1, span).is_err() {
            return CheckedType::Bool;
        }
        if !types_comparable(receiver_ty, &CheckedType::String)
            && *receiver_ty != CheckedType::Unknown
        {
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
        CheckedType::Bool
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
