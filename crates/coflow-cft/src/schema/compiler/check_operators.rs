use super::CheckTypeAnalyzer;
use crate::diagnostics::CftErrorCode;
use crate::schema::compiler::checked_type::{
    ordered_comparable, types_comparable, unwrap_nullable, CheckedType,
};
use crate::syntax::ast::{BinOp, CmpOp, UnaryOp};
use crate::syntax::Span;

impl CheckTypeAnalyzer<'_, '_> {
    pub(super) fn check_unary(&mut self, op: UnaryOp, ty: &CheckedType, span: Span) -> CheckedType {
        if *ty == CheckedType::Unknown {
            return CheckedType::Unknown;
        }
        let unwrapped = unwrap_nullable(ty);
        match op {
            UnaryOp::Not if matches!(unwrapped, CheckedType::Bool) => CheckedType::Bool,
            UnaryOp::Neg | UnaryOp::BitNot if matches!(unwrapped, CheckedType::Int) => {
                CheckedType::Int
            }
            UnaryOp::Neg if matches!(unwrapped, CheckedType::Float) => CheckedType::Float,
            UnaryOp::BitNot if self.is_flag_enum(ty) => ty.clone(),
            UnaryOp::BitNot => {
                self.diag(
                    CftErrorCode::BitwiseRequiresIntOrFlagEnum,
                    span,
                    "bitwise not requires int or flag enum",
                );
                CheckedType::Unknown
            }
            _ => {
                self.diag(
                    CftErrorCode::OperatorTypeMismatch,
                    span,
                    "unary operator does not support this operand type",
                );
                CheckedType::Unknown
            }
        }
    }

    pub(super) fn check_binop(
        &mut self,
        op: BinOp,
        lhs: &CheckedType,
        rhs: &CheckedType,
        span: Span,
    ) -> CheckedType {
        match op {
            BinOp::Or | BinOp::And => {
                if (!types_comparable(lhs, &CheckedType::Bool)
                    || !types_comparable(rhs, &CheckedType::Bool))
                    && *lhs != CheckedType::Unknown
                    && *rhs != CheckedType::Unknown
                {
                    self.diag(
                        CftErrorCode::OperatorTypeMismatch,
                        span,
                        "logical operators require bool operands",
                    );
                }
                CheckedType::Bool
            }
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Pow => {
                if types_comparable(lhs, &CheckedType::Int)
                    && types_comparable(rhs, &CheckedType::Int)
                {
                    CheckedType::Int
                } else if types_comparable(lhs, &CheckedType::Float)
                    && types_comparable(rhs, &CheckedType::Float)
                {
                    CheckedType::Float
                } else {
                    self.operator_mismatch(lhs, rhs, span);
                    CheckedType::Unknown
                }
            }
            BinOp::IntDiv | BinOp::Mod => {
                if types_comparable(lhs, &CheckedType::Int)
                    && types_comparable(rhs, &CheckedType::Int)
                {
                    CheckedType::Int
                } else {
                    self.operator_mismatch(lhs, rhs, span);
                    CheckedType::Unknown
                }
            }
            BinOp::Shl | BinOp::Shr => {
                if types_comparable(lhs, &CheckedType::Int)
                    && types_comparable(rhs, &CheckedType::Int)
                {
                    CheckedType::Int
                } else {
                    self.diag(
                        CftErrorCode::ShiftRequiresInt,
                        span,
                        "shift operators require int operands",
                    );
                    CheckedType::Unknown
                }
            }
            BinOp::BitOr | BinOp::BitXor | BinOp::BitAnd => {
                if types_comparable(lhs, &CheckedType::Int)
                    && types_comparable(rhs, &CheckedType::Int)
                {
                    CheckedType::Int
                } else if types_comparable(lhs, rhs) && self.is_flag_enum(lhs) {
                    lhs.clone()
                } else {
                    self.diag(
                        CftErrorCode::BitwiseRequiresIntOrFlagEnum,
                        span,
                        "bitwise operators require int or the same flag enum",
                    );
                    CheckedType::Unknown
                }
            }
        }
    }

    pub(super) fn check_comparison(
        &mut self,
        op: CmpOp,
        lhs: &CheckedType,
        rhs: &CheckedType,
        span: Span,
    ) -> CheckedType {
        let ok = match op {
            CmpOp::Eq | CmpOp::Ne => types_comparable(lhs, rhs),
            CmpOp::Lt | CmpOp::Le | CmpOp::Gt | CmpOp::Ge => ordered_comparable(lhs, rhs),
        };
        if !ok && *lhs != CheckedType::Unknown && *rhs != CheckedType::Unknown {
            self.diag(
                CftErrorCode::ComparisonTypeMismatch,
                span,
                "comparison operands are not compatible",
            );
        }
        CheckedType::Bool
    }

    fn operator_mismatch(&mut self, lhs: &CheckedType, rhs: &CheckedType, span: Span) {
        if *lhs != CheckedType::Unknown && *rhs != CheckedType::Unknown {
            self.diag(
                CftErrorCode::OperatorTypeMismatch,
                span,
                "operator does not support these operand types",
            );
        }
    }

    fn is_flag_enum(&self, ty: &CheckedType) -> bool {
        let CheckedType::Enum(name) = unwrap_nullable(ty) else {
            return false;
        };
        self.compiler
            .enums
            .get(name)
            .is_some_and(|info| info.is_flag)
    }
}
