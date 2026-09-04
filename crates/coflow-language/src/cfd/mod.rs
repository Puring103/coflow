//! Schema-free CFD syntax parser.

pub mod ast;
mod function;
mod parser;

pub(crate) use function::validate_function_body;

pub use ast::{
    CfdAst, CfdBitExpr, CfdBitExprKind, CfdBitOp, CfdBlock, CfdField, CfdFieldReference,
    CfdFormatSegment, CfdFormattedString, CfdFunction, CfdNamespaceDecl, CfdRecord, CfdRef,
    CfdUseDecl, CfdValue,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CfdParseOptions {
    pub structural_limits: crate::limits::StructuralLimits,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CfdSyntaxDiagnostic {
    pub message: String,
    pub span: crate::source::Span,
}

#[must_use]
pub fn parse_cfd(source: &str) -> (CfdAst, Vec<CfdSyntaxDiagnostic>) {
    parse_cfd_with_options(source, CfdParseOptions::default())
}

#[must_use]
pub fn parse_cfd_with_options(
    source: &str,
    options: CfdParseOptions,
) -> (CfdAst, Vec<CfdSyntaxDiagnostic>) {
    parser::parse(source, options)
}

/// Produces the lossless token stream consumed by source tooling.
#[must_use]
pub fn tokenize_cfd(source: &str) -> Vec<crate::lexical::LosslessToken> {
    crate::lexical::tokenize_lossless(source)
}
