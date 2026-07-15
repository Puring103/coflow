mod annotations;
mod budget;
mod check;
mod check_primary;
mod defaults;
mod definitions;
mod literals;
mod recovery;
mod tokens;

pub use self::budget::CftParseOptions;
pub(super) use self::budget::Parsed;
use self::tokens::{reserved_keyword_name, token_name};
use crate::diagnostics::{CftDiagnostic, CftDiagnostics, CftErrorCode};
use crate::module::ModuleId;
use crate::syntax::ast::{ModuleAst, NameRef};
use crate::syntax::lexer::{lex, Token, TokenKind};
use crate::syntax::Span;
use coflow_structure::StructuralBudget;

/// Parses one CFT module into its AST.
///
/// # Errors
///
/// Returns diagnostics when lexing fails or when tokens do not match the CFT
/// grammar.
pub fn parse_module(module: &ModuleId, source: &str) -> Result<ModuleAst, CftDiagnostics> {
    parse_module_with_options(module, source, CftParseOptions::default())
}

/// Parses one CFT module with explicit structural resource limits.
///
/// # Errors
///
/// Returns lexical, syntax, or structural-limit diagnostics.
pub fn parse_module_with_options(
    module: &ModuleId,
    source: &str,
    options: CftParseOptions,
) -> Result<ModuleAst, CftDiagnostics> {
    let tokens = lex(module, source)?;
    Parser::new(module, tokens, options).parse_module()
}

struct Parser<'a> {
    module: &'a ModuleId,
    tokens: Vec<Token>,
    pos: usize,
    budget: StructuralBudget,
    open_nesting: u64,
}

impl<'a> Parser<'a> {
    fn new(module: &'a ModuleId, tokens: Vec<Token>, options: CftParseOptions) -> Self {
        Self {
            module,
            tokens,
            pos: 0,
            budget: StructuralBudget::new(options.structural_limits),
            open_nesting: 0,
        }
    }

    pub(super) fn expect_ident(&mut self) -> Result<NameRef, CftDiagnostics> {
        self.expect_name(CftErrorCode::ExpectedIdentifier, true)
    }

    pub(super) fn expect_ident_with_code(
        &mut self,
        code: CftErrorCode,
    ) -> Result<NameRef, CftDiagnostics> {
        self.expect_name(code, false)
    }

    fn expect_name(
        &mut self,
        code: CftErrorCode,
        allow_reserved_keywords: bool,
    ) -> Result<NameRef, CftDiagnostics> {
        let token = self.peek().clone();
        match token.kind {
            TokenKind::Ident(name) => {
                self.bump();
                Ok(NameRef {
                    name,
                    span: token.span,
                })
            }
            _ if allow_reserved_keywords => {
                if let Some(name) = reserved_keyword_name(&token.kind) {
                    self.bump();
                    Ok(NameRef {
                        name: name.to_string(),
                        span: token.span,
                    })
                } else {
                    self.err(code, "expected identifier")
                }
            }
            _ => self.err(code, "expected identifier"),
        }
    }

    pub(super) fn expect_simple(
        &mut self,
        kind: &TokenKind,
        code: CftErrorCode,
    ) -> Result<Span, CftDiagnostics> {
        if self.at(kind) {
            Ok(self.bump().span)
        } else {
            self.err(code, format!("expected `{}`", token_name(kind)))
        }
    }

    pub(super) fn eat(&mut self, kind: &TokenKind) -> Option<Span> {
        if self.at(kind) {
            Some(self.bump().span)
        } else {
            None
        }
    }

    pub(super) fn at(&self, kind: &TokenKind) -> bool {
        std::mem::discriminant(&self.peek().kind) == std::mem::discriminant(kind)
    }

    pub(super) fn next_at(&self, kind: &TokenKind) -> bool {
        self.tokens.get(self.pos + 1).is_some_and(|token| {
            std::mem::discriminant(&token.kind) == std::mem::discriminant(kind)
        })
    }

    pub(super) fn peek_ident_is(&self, name: &str) -> bool {
        matches!(&self.peek().kind, TokenKind::Ident(value) if value == name)
    }

    pub(super) fn bump(&mut self) -> Token {
        let token = self.tokens[self.pos].clone();
        self.pos += 1;
        token
    }

    pub(super) fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn prev_span(&self) -> Span {
        self.tokens[self.pos - 1].span
    }

    pub(super) fn err<T>(
        &self,
        code: CftErrorCode,
        message: impl Into<String>,
    ) -> Result<T, CftDiagnostics> {
        self.err_at(code, self.peek().span, message)
    }

    pub(super) fn err_at<T>(
        &self,
        code: CftErrorCode,
        span: Span,
        message: impl Into<String>,
    ) -> Result<T, CftDiagnostics> {
        Err(CftDiagnostics::one(CftDiagnostic::error(
            code,
            self.module.clone(),
            span,
            message,
        )))
    }
}

/// Folds `-magnitude` where `magnitude > i64::MAX` into the equivalent `i64`.
/// Only `2^63` (i.e. `i64::MIN.unsigned_abs()`) is representable; any larger
/// magnitude is out of range and returns `None`.
pub(super) fn negate_u64_to_i64(magnitude: u64) -> Option<i64> {
    const I64_MIN_MAGNITUDE: u64 = i64::MIN.unsigned_abs();
    if magnitude == I64_MIN_MAGNITUDE {
        Some(i64::MIN)
    } else {
        None
    }
}
