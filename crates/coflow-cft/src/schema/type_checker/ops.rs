use super::TypeChecker;
use crate::ast::{BinOp, CmpOp, UnaryOp};
use crate::error::CftErrorCode;
use crate::schema::support::{ordered_comparable, types_comparable, unwrap_nullable, Ty};
use crate::span::Span;

impl TypeChecker<'_, '_> {
    pub(super) fn check_unary(&mut self, op: UnaryOp, ty: &Ty, span: Span) -> Ty {
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

    pub(super) fn check_binop(&mut self, op: BinOp, lhs: &Ty, rhs: &Ty, span: Span) -> Ty {
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

    pub(super) fn check_comparison(&mut self, op: CmpOp, lhs: &Ty, rhs: &Ty, span: Span) -> Ty {
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
}
