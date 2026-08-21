//! Schema-free CFD syntax parser.

pub mod ast;
mod parser;

pub use ast::{
    CfdAst, CfdBitExpr, CfdBitExprKind, CfdBitOp, CfdBlock, CfdField, CfdFieldReference,
    CfdFormatSegment, CfdFormattedString, CfdRecord, CfdRef, CfdValue,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CfdParseOptions {
    pub structural_limits: crate::limits::StructuralLimits,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CfdSyntaxDiagnostic {
    pub message: String,
    pub span: crate::limits::Span,
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
