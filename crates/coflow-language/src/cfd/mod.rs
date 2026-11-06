//! Schema-free CFD syntax parser.

pub mod ast;
mod parser;
mod structure;

pub use ast::{
    CfdAst, CfdBitExpr, CfdBitExprKind, CfdBitOp, CfdBlock, CfdDimensionBlock, CfdField,
    CfdFieldReference, CfdFormatSegment, CfdFormattedString, CfdRecord, CfdRef, CfdValue,
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

/// 解析一个完整 CFD 值；仅允许值前后的空白与注释，不接受额外字段或记录。
///
/// # Errors
/// 返回值语法、结构预算或未消费输入的诊断，位置直接对应传入源码。
pub fn parse_cfd_value(source: &str) -> Result<CfdValue, CfdSyntaxDiagnostic> {
    parse_cfd_value_with_options(source, CfdParseOptions::default())
}

/// 使用指定结构预算解析一个完整 CFD 值。
///
/// # Errors
/// 返回值语法、结构预算或未消费输入的诊断。
pub fn parse_cfd_value_with_options(
    source: &str,
    options: CfdParseOptions,
) -> Result<CfdValue, CfdSyntaxDiagnostic> {
    parser::parse_value_span(source, crate::Span::new(0, source.len()), options)
}

/// CFT 默认模板直接复用原源码区间，保留引用位置供诊断与源码改写使用。
pub(crate) fn parse_value_span(
    source: &str,
    span: crate::Span,
) -> Result<CfdValue, CfdSyntaxDiagnostic> {
    parser::parse_value_span(source, span, CfdParseOptions::default())
}

/// Produces the lossless token stream consumed by source tooling.
#[must_use]
pub fn tokenize_cfd(source: &str) -> Vec<crate::lexical::LosslessToken> {
    crate::lexical::tokenize_lossless(source)
}
