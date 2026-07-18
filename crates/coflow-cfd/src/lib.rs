//! Schema-free CFD syntax parser for Coflow.
//!
//! Parses `.cfd` text into a [`CfdAst`] with source spans, without requiring
//! a compiled CFT schema. Intended for use by language tooling (LSP, syntax
//! highlighting). For data loading use `coflow-loader-cfd`.

#![cfg_attr(
    not(test),
    deny(
        clippy::dbg_macro,
        clippy::expect_used,
        clippy::panic,
        clippy::panic_in_result_fn,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable,
        clippy::unwrap_used
    )
)]
#![allow(clippy::missing_const_for_fn, clippy::use_self)]

pub mod ast;
mod parser;

pub use ast::{CfdAst, CfdBlock, CfdBlockEntry, CfdField, CfdRecord, CfdRef, CfdValue};
pub use coflow_structure::{Span, StructuralLimits};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CfdParseOptions {
    pub structural_limits: StructuralLimits,
}

/// A syntax-level diagnostic produced during CFD parsing.
///
/// Only covers structural errors (missing `{`, unterminated strings, etc.).
/// Schema-level errors (unknown types, wrong field types) are reported by
/// `coflow-loader-cfd`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CfdSyntaxDiagnostic {
    pub message: String,
    pub span: Span,
}

/// Parse `.cfd` source text into an AST.
///
/// Always returns an AST (possibly partial) along with any syntax diagnostics.
/// Parsing continues after errors using best-effort recovery.
#[must_use]
pub fn parse_cfd(source: &str) -> (CfdAst, Vec<CfdSyntaxDiagnostic>) {
    parse_cfd_with_options(source, CfdParseOptions::default())
}

/// Parse `.cfd` source text with explicit structural resource limits.
#[must_use]
pub fn parse_cfd_with_options(
    source: &str,
    options: CfdParseOptions,
) -> (CfdAst, Vec<CfdSyntaxDiagnostic>) {
    parser::parse(source, options)
}
