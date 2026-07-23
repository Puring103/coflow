use super::CheckTypeAnalyzer;
use crate::diagnostics::CftErrorCode;
use crate::schema::compiler::inferred_type::{
    min_max_supported, set_element_supported, sorted_element_supported, types_comparable,
    unique_supported, unwrap_nullable, InferredType,
};
use crate::schema::{CftCheckBuiltin, CftValueType};
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
        match CftCheckBuiltin::by_name(&name.name) {
            Some(CftCheckBuiltin::Len) => self.check_len_method(receiver, args, span, &receiver_ty),
            Some(CftCheckBuiltin::Contains) => {
                self.check_contains_method(receiver, args, span, &receiver_ty)
            }
            Some(CftCheckBuiltin::Unique) => {
                self.check_unique_method(receiver, args, span, &receiver_ty)
            }
            Some(CftCheckBuiltin::Min | CftCheckBuiltin::Max) => {
                self.check_min_max_method(receiver, args, span, &receiver_ty)
            }
            Some(CftCheckBuiltin::Sum) => self.check_sum_method(receiver, args, span, &receiver_ty),
            Some(CftCheckBuiltin::Keys) => {
                self.check_keys_method(receiver, args, span, &receiver_ty)
            }
            Some(CftCheckBuiltin::Values) => {
                self.check_values_method(receiver, args, span, &receiver_ty)
            }
            Some(CftCheckBuiltin::Matches) => {
                self.check_matches_method(receiver, args, span, &receiver_ty)
            }
            Some(CftCheckBuiltin::StartsWith | CftCheckBuiltin::EndsWith) => {
                self.check_string_predicate_method(receiver, args, span, &receiver_ty)
            }
            Some(CftCheckBuiltin::IsBlank) => {
                self.check_receiver_method(receiver, args, span, &receiver_ty, 0, "string", |ty| {
                    matches!(ty.value_type(), Some(CftValueType::String))
                })
            }
            Some(CftCheckBuiltin::Abs) => self.check_abs_method(receiver, args, span, &receiver_ty),
            Some(CftCheckBuiltin::IsFinite) => {
                self.check_receiver_method(receiver, args, span, &receiver_ty, 0, "float", |ty| {
                    matches!(ty.value_type(), Some(CftValueType::Float))
                })
            }
            Some(CftCheckBuiltin::ApproxEqual) => {
                self.check_approx_equal_method(receiver, args, span, &receiver_ty)
            }
            Some(CftCheckBuiltin::ContainsKey) => {
                self.check_dict_contains_method(receiver, args, span, &receiver_ty, true)
            }
            Some(CftCheckBuiltin::ContainsValue) => {
                self.check_dict_contains_method(receiver, args, span, &receiver_ty, false)
            }
            Some(CftCheckBuiltin::IsSorted | CftCheckBuiltin::IsStrictlySorted) => {
                self.check_sorted_method(receiver, args, span, &receiver_ty)
            }
            Some(
                CftCheckBuiltin::Intersects
                | CftCheckBuiltin::IsDisjoint
                | CftCheckBuiltin::IsSubsetOf
                | CftCheckBuiltin::IsSupersetOf,
            ) => self.check_set_relation_method(receiver, args, span, &receiver_ty),
            None => {
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
        if self
            .expect_arity(args, CftCheckBuiltin::Len.method_arity(), span)
            .is_err()
        {
            return InferredType::Unknown;
        }
        let receiver_ty = unwrap_nullable(receiver_ty);
        if receiver_ty.array_element().is_none()
            && receiver_ty.dict_types().is_none()
            && !matches!(receiver_ty.value_type(), Some(CftValueType::String))
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
        if self
            .expect_arity(args, CftCheckBuiltin::Contains.method_arity(), span)
            .is_err()
        {
            return InferredType::bool();
        }
        let value_ty = self.check_expr_value(&args[0]);
        let receiver_ty = unwrap_nullable(receiver_ty);
        if matches!(receiver_ty.value_type(), Some(CftValueType::String)) {
            if !types_comparable(&value_ty, &InferredType::string()) && !value_ty.is_unknown() {
                self.diag(
                    CftErrorCode::FunctionArgTypeMismatch,
                    args[0].span,
                    "contains argument must be string",
                );
            }
        } else if let Some(elem) = receiver_ty.array_element() {
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
        if self
            .expect_arity(args, CftCheckBuiltin::Unique.method_arity(), span)
            .is_err()
        {
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
        if self
            .expect_arity(args, CftCheckBuiltin::Min.method_arity(), span)
            .is_err()
        {
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
        if self
            .expect_arity(args, CftCheckBuiltin::Sum.method_arity(), span)
            .is_err()
        {
            return InferredType::Unknown;
        }
        let receiver_ty = unwrap_nullable(receiver_ty);
        if let Some(elem) = receiver_ty.array_element() {
            let elem = unwrap_nullable(&elem);
            if let Some(CftValueType::Int | CftValueType::Float) = elem.value_type() {
                elem
            } else {
                self.diag(
                    CftErrorCode::FunctionArgTypeMismatch,
                    receiver.span,
                    "sum expects an int or float array",
                );
                InferredType::Unknown
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
        if self
            .expect_arity(args, CftCheckBuiltin::Keys.method_arity(), span)
            .is_err()
        {
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
        if self
            .expect_arity(args, CftCheckBuiltin::Values.method_arity(), span)
            .is_err()
        {
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
        if self
            .expect_arity(args, CftCheckBuiltin::Matches.method_arity(), span)
            .is_err()
        {
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

    fn check_string_predicate_method(
        &mut self,
        receiver: &CheckExpr,
        args: &[CheckExpr],
        span: Span,
        receiver_ty: &InferredType,
    ) -> InferredType {
        if self.expect_arity(args, 1, span).is_err() {
            return InferredType::bool();
        }
        let arg_ty = self.check_expr_value(&args[0]);
        let receiver_ty = unwrap_nullable(receiver_ty);
        if !types_comparable(&receiver_ty, &InferredType::string()) && !receiver_ty.is_unknown() {
            self.diag(
                CftErrorCode::FunctionArgTypeMismatch,
                receiver.span,
                "method receiver must be string",
            );
        }
        if !types_comparable(&arg_ty, &InferredType::string()) && !arg_ty.is_unknown() {
            self.diag(
                CftErrorCode::FunctionArgTypeMismatch,
                args[0].span,
                "method argument must be string",
            );
        }
        InferredType::bool()
    }

    fn check_receiver_method(
        &mut self,
        receiver: &CheckExpr,
        args: &[CheckExpr],
        span: Span,
        receiver_ty: &InferredType,
        arity: usize,
        expected: &str,
        supports: impl FnOnce(&InferredType) -> bool,
    ) -> InferredType {
        if self.expect_arity(args, arity, span).is_err() {
            return InferredType::bool();
        }
        let receiver_ty = unwrap_nullable(receiver_ty);
        if !supports(&receiver_ty) && !receiver_ty.is_unknown() {
            self.diag(
                CftErrorCode::FunctionArgTypeMismatch,
                receiver.span,
                format!("method receiver must be {expected}"),
            );
        }
        InferredType::bool()
    }

    fn check_abs_method(
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
        if matches!(
            receiver_ty.value_type(),
            Some(CftValueType::Int | CftValueType::Float)
        ) {
            receiver_ty
        } else {
            if !receiver_ty.is_unknown() {
                self.diag(
                    CftErrorCode::FunctionArgTypeMismatch,
                    receiver.span,
                    "abs receiver must be int or float",
                );
            }
            InferredType::Unknown
        }
    }

    fn check_approx_equal_method(
        &mut self,
        receiver: &CheckExpr,
        args: &[CheckExpr],
        span: Span,
        receiver_ty: &InferredType,
    ) -> InferredType {
        if self.expect_arity(args, 2, span).is_err() {
            return InferredType::bool();
        }
        let receiver_ty = unwrap_nullable(receiver_ty);
        if !types_comparable(&receiver_ty, &InferredType::float()) && !receiver_ty.is_unknown() {
            self.diag(
                CftErrorCode::FunctionArgTypeMismatch,
                receiver.span,
                "approxEqual receiver must be float",
            );
        }
        for arg in args {
            let ty = self.check_expr_value(arg);
            if !types_comparable(&ty, &InferredType::float()) && !ty.is_unknown() {
                self.diag(
                    CftErrorCode::FunctionArgTypeMismatch,
                    arg.span,
                    "approxEqual arguments must be float",
                );
            }
        }
        InferredType::bool()
    }

    fn check_dict_contains_method(
        &mut self,
        receiver: &CheckExpr,
        args: &[CheckExpr],
        span: Span,
        receiver_ty: &InferredType,
        key: bool,
    ) -> InferredType {
        if self.expect_arity(args, 1, span).is_err() {
            return InferredType::bool();
        }
        let arg_ty = self.check_expr_value(&args[0]);
        let receiver_ty = unwrap_nullable(receiver_ty);
        if let Some((key_ty, value_ty)) = receiver_ty.dict_types() {
            let expected = if key { key_ty } else { value_ty };
            if !types_comparable(&expected, &arg_ty) && !arg_ty.is_unknown() {
                self.diag(
                    CftErrorCode::FunctionArgTypeMismatch,
                    args[0].span,
                    "dict contains argument type mismatch",
                );
            }
        } else if !receiver_ty.is_unknown() {
            self.diag(
                CftErrorCode::FunctionArgTypeMismatch,
                receiver.span,
                "method receiver must be dict",
            );
        }
        InferredType::bool()
    }

    fn check_sorted_method(
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
        if let Some(element) = receiver_ty.array_element() {
            if !sorted_element_supported(&element) {
                self.diag(
                    CftErrorCode::FunctionArgTypeMismatch,
                    receiver.span,
                    "sorting requires a non-nullable int, bool, string, or enum array",
                );
            }
        } else if !receiver_ty.is_unknown() {
            self.diag(
                CftErrorCode::FunctionArgTypeMismatch,
                receiver.span,
                "sorting requires an array",
            );
        }
        InferredType::bool()
    }

    fn check_set_relation_method(
        &mut self,
        receiver: &CheckExpr,
        args: &[CheckExpr],
        span: Span,
        receiver_ty: &InferredType,
    ) -> InferredType {
        if self.expect_arity(args, 1, span).is_err() {
            return InferredType::bool();
        }
        let other_ty = unwrap_nullable(&self.check_expr_value(&args[0]));
        let receiver_ty = unwrap_nullable(receiver_ty);
        match (receiver_ty.array_element(), other_ty.array_element()) {
            (Some(left), Some(right)) => {
                if !set_element_supported(&left) || !types_comparable(&left, &right) {
                    self.diag(
                        CftErrorCode::FunctionArgTypeMismatch,
                        receiver.span,
                        "set relation requires compatible int, bool, string, or enum arrays",
                    );
                }
            }
            _ if !receiver_ty.is_unknown() && !other_ty.is_unknown() => {
                self.diag(
                    CftErrorCode::FunctionArgTypeMismatch,
                    receiver.span,
                    "set relation requires two arrays",
                );
            }
            _ => {}
        }
        InferredType::bool()
    }
}
