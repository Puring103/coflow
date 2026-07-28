use super::CheckTypeAnalyzer;
use crate::diagnostics::CftErrorCode;
use crate::schema::compiler::inferred_type::{
    ordered_comparable, types_comparable, unwrap_nullable, InferredType,
};
use crate::schema::CftValueType;
use crate::syntax::ast::{BinOp, CmpOp, UnaryOp};
use crate::syntax::Span;

impl CheckTypeAnalyzer<'_, '_> {
    pub(super) fn check_unary(
        &mut self,
        op: UnaryOp,
        ty: &InferredType,
        span: Span,
    ) -> InferredType {
        if ty.is_unknown() {
            return InferredType::Unknown;
        }
        let unwrapped = unwrap_nullable(ty);
        match op {
            UnaryOp::Not if matches!(unwrapped.value_type(), Some(CftValueType::Bool)) => {
                InferredType::bool()
            }
            UnaryOp::Neg | UnaryOp::BitNot
                if matches!(unwrapped.value_type(), Some(CftValueType::Int)) =>
            {
                InferredType::int()
            }
            UnaryOp::Neg if matches!(unwrapped.value_type(), Some(CftValueType::Float)) => {
                InferredType::float()
            }
            UnaryOp::BitNot if self.is_flag_enum(ty) => ty.clone(),
            UnaryOp::BitNot => {
                self.diag(
                    CftErrorCode::BitwiseRequiresIntOrFlagEnum,
                    span,
                    "bitwise not requires int or flag enum",
                );
                InferredType::Unknown
            }
            _ => {
                self.diag(
                    CftErrorCode::OperatorTypeMismatch,
                    span,
                    "unary operator does not support this operand type",
                );
                InferredType::Unknown
            }
        }
    }

    pub(super) fn check_binop(
        &mut self,
        op: BinOp,
        lhs: &InferredType,
        rhs: &InferredType,
        span: Span,
    ) -> InferredType {
        match op {
            BinOp::Or | BinOp::And => {
                if (!types_comparable(lhs, &InferredType::bool())
                    || !types_comparable(rhs, &InferredType::bool()))
                    && !lhs.is_unknown()
                    && !rhs.is_unknown()
                {
                    self.diag(
                        CftErrorCode::OperatorTypeMismatch,
                        span,
                        "logical operators require bool operands",
                    );
                }
                InferredType::bool()
            }
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Pow => {
                if types_comparable(lhs, &InferredType::int())
                    && types_comparable(rhs, &InferredType::int())
                {
                    InferredType::int()
                } else if types_comparable(lhs, &InferredType::float())
                    && types_comparable(rhs, &InferredType::float())
                {
                    InferredType::float()
                } else {
                    self.operator_mismatch(lhs, rhs, span);
                    InferredType::Unknown
                }
            }
            BinOp::IntDiv | BinOp::Mod => {
                if types_comparable(lhs, &InferredType::int())
                    && types_comparable(rhs, &InferredType::int())
                {
                    InferredType::int()
                } else {
                    self.operator_mismatch(lhs, rhs, span);
                    InferredType::Unknown
                }
            }
            BinOp::Shl | BinOp::Shr => {
                if types_comparable(lhs, &InferredType::int())
                    && types_comparable(rhs, &InferredType::int())
                {
                    InferredType::int()
                } else {
                    self.diag(
                        CftErrorCode::ShiftRequiresInt,
                        span,
                        "shift operators require int operands",
                    );
                    InferredType::Unknown
                }
            }
            BinOp::BitOr | BinOp::BitXor | BinOp::BitAnd => {
                if types_comparable(lhs, &InferredType::int())
                    && types_comparable(rhs, &InferredType::int())
                {
                    InferredType::int()
                } else if types_comparable(lhs, rhs) && self.is_flag_enum(lhs) {
                    lhs.clone()
                } else {
                    self.diag(
                        CftErrorCode::BitwiseRequiresIntOrFlagEnum,
                        span,
                        "bitwise operators require int or the same flag enum",
                    );
                    InferredType::Unknown
                }
            }
        }
    }

    pub(super) fn check_comparison(
        &mut self,
        op: CmpOp,
        lhs: &InferredType,
        rhs: &InferredType,
        span: Span,
    ) -> InferredType {
        let ok = match op {
            CmpOp::Eq | CmpOp::Ne => types_comparable(lhs, rhs),
            CmpOp::Lt | CmpOp::Le | CmpOp::Gt | CmpOp::Ge => ordered_comparable(lhs, rhs),
        };
        if !ok && !lhs.is_unknown() && !rhs.is_unknown() {
            self.diag(
                CftErrorCode::ComparisonTypeMismatch,
                span,
                "comparison operands are not compatible",
            );
        }
        InferredType::bool()
    }

    fn operator_mismatch(&mut self, lhs: &InferredType, rhs: &InferredType, span: Span) {
        if !lhs.is_unknown() && !rhs.is_unknown() {
            self.diag(
                CftErrorCode::OperatorTypeMismatch,
                span,
                "operator does not support these operand types",
            );
        }
    }

    fn is_flag_enum(&self, ty: &InferredType) -> bool {
        let Some(name) = unwrap_nullable(ty).enum_name().cloned() else {
            return false;
        };
        self.compiler
            .enums
            .get(name.as_str())
            .is_some_and(|info| info.is_flag)
    }
}
