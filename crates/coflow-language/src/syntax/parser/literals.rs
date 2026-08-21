use super::{negate_u64_to_i64, Parser};
use crate::diagnostics::{CftDiagnostics, CftErrorCode};
use crate::syntax::ast::{ConstLiteral, SignedInt};
use crate::syntax::lexer::TokenKind;
use crate::syntax::Span;

impl Parser<'_> {
    pub(super) fn parse_const_literal(&mut self) -> Result<ConstLiteral, CftDiagnostics> {
        let token = self.peek().clone();
        match token.kind {
            TokenKind::Int(value) => {
                self.bump();
                Ok(ConstLiteral::Int(value, token.span))
            }
            TokenKind::Float(value) => {
                self.bump();
                Ok(ConstLiteral::Float(value, token.span))
            }
            TokenKind::True => {
                self.bump();
                Ok(ConstLiteral::Bool(true, token.span))
            }
            TokenKind::False => {
                self.bump();
                Ok(ConstLiteral::Bool(false, token.span))
            }
            TokenKind::String(value) => {
                self.bump();
                Ok(ConstLiteral::String(value, token.span))
            }
            TokenKind::Minus => self.parse_negative_const_literal(token.span),
            TokenKind::UIntOverflow(_) => self.err(
                CftErrorCode::InvalidIntLiteral,
                "integer literal out of range",
            ),
            _ => self.err(
                CftErrorCode::InvalidConstValue,
                "const value must be an int, float, bool, or string literal",
            ),
        }
    }

    fn parse_negative_const_literal(
        &mut self,
        minus_span: Span,
    ) -> Result<ConstLiteral, CftDiagnostics> {
        self.bump();
        let next = self.peek().clone();
        let span = Span::new(minus_span.start, next.span.end);
        match next.kind {
            TokenKind::Int(value) => {
                self.bump();
                let Some(negated) = value.checked_neg() else {
                    return self.err_at(
                        CftErrorCode::InvalidIntLiteral,
                        span,
                        "negated integer literal overflowed",
                    );
                };
                Ok(ConstLiteral::Int(negated, span))
            }
            TokenKind::UIntOverflow(value) => {
                self.bump();
                let Some(negated) = negate_u64_to_i64(value) else {
                    return self.err_at(
                        CftErrorCode::InvalidIntLiteral,
                        span,
                        "integer literal out of range",
                    );
                };
                Ok(ConstLiteral::Int(negated, span))
            }
            TokenKind::Float(value) => {
                self.bump();
                Ok(ConstLiteral::Float(-value, span))
            }
            _ => self.err(CftErrorCode::InvalidConstValue, "expected numeric literal"),
        }
    }

    pub(super) fn parse_signed_int(&mut self) -> Result<SignedInt, CftDiagnostics> {
        let sign_span = self.eat(&TokenKind::Minus);
        let token = self.peek().clone();
        let span = sign_span.map_or(token.span, |span| span.join(token.span));
        match token.kind {
            TokenKind::Int(value) => {
                self.bump();
                let value = if sign_span.is_some() {
                    let Some(negated) = value.checked_neg() else {
                        return self.err_at(
                            CftErrorCode::InvalidIntLiteral,
                            span,
                            "negated integer literal overflowed",
                        );
                    };
                    negated
                } else {
                    value
                };
                Ok(SignedInt { value, span })
            }
            TokenKind::UIntOverflow(value) => {
                self.bump();
                if sign_span.is_some() {
                    let Some(negated) = negate_u64_to_i64(value) else {
                        return self.err_at(
                            CftErrorCode::InvalidIntLiteral,
                            span,
                            "integer literal out of range",
                        );
                    };
                    Ok(SignedInt {
                        value: negated,
                        span,
                    })
                } else {
                    self.err_at(
                        CftErrorCode::InvalidIntLiteral,
                        span,
                        "integer literal out of range",
                    )
                }
            }
            _ => self.err(CftErrorCode::ExpectedToken, "expected integer literal"),
        }
    }
}
