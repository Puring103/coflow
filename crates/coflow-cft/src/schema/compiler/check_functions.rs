use super::CheckTypeAnalyzer;
use crate::diagnostics::CftErrorCode;
use crate::schema::compiler::inferred_type::{
    min_max_supported, types_comparable, unique_supported, unwrap_nullable, InferredType,
};
use crate::schema::CftValueType;
use crate::syntax::ast::{CheckExpr, CheckExprKind, NameRef};
use crate::syntax::Span;
use regex::Regex;

impl CheckTypeAnalyzer<'_, '_> {
    pub(super) fn check_call(
        &mut self,
        name: &NameRef,
        args: &[CheckExpr],
        span: Span,
    ) -> InferredType {
        if self.compiler.enums.contains_key(&name.name) {
            if args.len() != 1 {
                self.diag(
                    CftErrorCode::FunctionArityMismatch,
                    span,
                    "enum constructor expects one argument",
                );
                return InferredType::Unknown;
            }
            let arg_ty = self.check_expr_value(&args[0]);
            if !types_comparable(&arg_ty, &InferredType::int()) && !arg_ty.is_unknown() {
                self.diag(
                    CftErrorCode::FunctionArgTypeMismatch,
                    args[0].span,
                    "enum constructor argument must be int",
                );
            }
            return InferredType::enum_value(crate::EnumName::from_validated(name.name.clone()));
        }

        self.diag(
            CftErrorCode::UnknownFunction,
            name.span,
            format!("unknown function `{}`", name.name),
        );
        for arg in args {
            self.check_expr_value(arg);
        }
        InferredType::Unknown
    }

    pub(super) fn check_method_call(
        &mut self,
        receiver: &CheckExpr,
        name: &NameRef,
        args: &[CheckExpr],
        span: Span,
    ) -> InferredType {
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
                InferredType::Unknown
            }
        }
    }

    fn check_len_method(
        &mut self,
        receiver: &CheckExpr,
        args: &[CheckExpr],
        span: Span,
        receiver_ty: &InferredType,
    ) -> InferredType {
        if self.expect_arity(args, 0, span).is_err() {
            return InferredType::Unknown;
        }
        let receiver_ty = unwrap_nullable(receiver_ty);
        if receiver_ty.array_element().is_none()
            && receiver_ty.dict_types().is_none()
            && !receiver_ty.is_unknown()
        {
            self.diag(
                CftErrorCode::FunctionArgTypeMismatch,
                receiver.span,
                "len expects an array or dict",
            );
        }
        InferredType::int()
    }

    fn check_contains_method(
        &mut self,
        receiver: &CheckExpr,
        args: &[CheckExpr],
        span: Span,
        receiver_ty: &InferredType,
    ) -> InferredType {
        if self.expect_arity(args, 1, span).is_err() {
            return InferredType::bool();
        }
        let value_ty = self.check_expr_value(&args[0]);
        let receiver_ty = unwrap_nullable(receiver_ty);
        if let Some(elem) = receiver_ty.array_element() {
            if !types_comparable(&elem, &value_ty) && !value_ty.is_unknown() {
                self.diag(
                    CftErrorCode::FunctionArgTypeMismatch,
                    args[0].span,
                    "contains value type does not match array element type",
                );
            }
        } else if let Some((key, _)) = receiver_ty.dict_types() {
            if !types_comparable(&key, &value_ty) && !value_ty.is_unknown() {
                self.diag(
                    CftErrorCode::FunctionArgTypeMismatch,
                    args[0].span,
                    "contains value type does not match dict key type",
                );
            }
        } else if !receiver_ty.is_unknown() {
            self.diag(
                CftErrorCode::FunctionArgTypeMismatch,
                receiver.span,
                "contains expects an array or dict",
            );
        }
        InferredType::bool()
    }

    fn check_unique_method(
        &mut self,
        receiver: &CheckExpr,
        args: &[CheckExpr],
        span: Span,
        receiver_ty: &InferredType,
    ) -> InferredType {
        if self.expect_arity(args, 0, span).is_err() {
            return InferredType::bool();
        }
        let receiver_ty = unwrap_nullable(receiver_ty);
        if let Some(elem) = receiver_ty.array_element() {
            if !unique_supported(&elem) {
                self.diag(
                    CftErrorCode::UniqueUnsupportedElementType,
                    receiver.span,
                    "isUnique does not support this element type",
                );
            }
        } else if !receiver_ty.is_unknown() {
            self.diag(
                CftErrorCode::FunctionArgTypeMismatch,
                receiver.span,
                "isUnique expects an array",
            );
        }
        InferredType::bool()
    }

    fn check_min_max_method(
        &mut self,
        receiver: &CheckExpr,
        args: &[CheckExpr],
        span: Span,
        receiver_ty: &InferredType,
    ) -> InferredType {
        if self.expect_arity(args, 0, span).is_err() {
            return InferredType::Unknown;
        }
        let receiver_ty = unwrap_nullable(receiver_ty);
        if let Some(elem) = receiver_ty.array_element() {
            if min_max_supported(&elem) {
                unwrap_nullable(&elem)
            } else {
                self.diag(
                    CftErrorCode::FunctionArgTypeMismatch,
                    receiver.span,
                    "min/max expects int, float, or enum arrays",
                );
                InferredType::Unknown
            }
        } else if receiver_ty.is_unknown() {
            InferredType::Unknown
        } else {
            self.diag(
                CftErrorCode::FunctionArgTypeMismatch,
                receiver.span,
                "min/max expects an array",
            );
            InferredType::Unknown
        }
    }

    fn check_sum_method(
        &mut self,
        receiver: &CheckExpr,
        args: &[CheckExpr],
        span: Span,
        receiver_ty: &InferredType,
    ) -> InferredType {
        if self.expect_arity(args, 0, span).is_err() {
            return InferredType::Unknown;
        }
        let receiver_ty = unwrap_nullable(receiver_ty);
        if let Some(elem) = receiver_ty.array_element() {
            let elem = unwrap_nullable(&elem);
            match elem.value_type() {
                Some(CftValueType::Int | CftValueType::Float) => elem,
                _ => {
                    self.diag(
                        CftErrorCode::FunctionArgTypeMismatch,
                        receiver.span,
                        "sum expects an int or float array",
                    );
                    InferredType::Unknown
                }
            }
        } else if receiver_ty.is_unknown() {
            InferredType::Unknown
        } else {
            self.diag(
                CftErrorCode::FunctionArgTypeMismatch,
                receiver.span,
                "sum expects an array",
            );
            InferredType::Unknown
        }
    }

    fn check_keys_method(
        &mut self,
        receiver: &CheckExpr,
        args: &[CheckExpr],
        span: Span,
        receiver_ty: &InferredType,
    ) -> InferredType {
        if self.expect_arity(args, 0, span).is_err() {
            return InferredType::Unknown;
        }
        let receiver_ty = unwrap_nullable(receiver_ty);
        if let Some((key, _)) = receiver_ty.dict_types() {
            InferredType::array(key)
        } else if receiver_ty.is_unknown() {
            InferredType::Unknown
        } else {
            self.diag(
                CftErrorCode::FunctionArgTypeMismatch,
                receiver.span,
                "keys expects a dict",
            );
            InferredType::Unknown
        }
    }

    fn check_values_method(
        &mut self,
        receiver: &CheckExpr,
        args: &[CheckExpr],
        span: Span,
        receiver_ty: &InferredType,
    ) -> InferredType {
        if self.expect_arity(args, 0, span).is_err() {
            return InferredType::Unknown;
        }
        let receiver_ty = unwrap_nullable(receiver_ty);
        if let Some((_, value)) = receiver_ty.dict_types() {
            InferredType::array(value)
        } else if receiver_ty.is_unknown() {
            InferredType::Unknown
        } else {
            self.diag(
                CftErrorCode::FunctionArgTypeMismatch,
                receiver.span,
                "values expects a dict",
            );
            InferredType::Unknown
        }
    }

    fn check_matches_method(
        &mut self,
        receiver: &CheckExpr,
        args: &[CheckExpr],
        span: Span,
        receiver_ty: &InferredType,
    ) -> InferredType {
        if self.expect_arity(args, 1, span).is_err() {
            return InferredType::bool();
        }
        if !types_comparable(receiver_ty, &InferredType::string()) && !receiver_ty.is_unknown() {
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
        InferredType::bool()
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
