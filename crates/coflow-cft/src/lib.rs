//! Reference implementation of the **CFT** type-definition language used by
//! Coflow's data pipeline.
//!
//! See `docs/spec/01-cft.md` for the language specification. The crate exposes
//! a [`CftContainer`] that hosts batch-registered modules and produces a
//! schema after [`CftContainer::compile`] succeeds; loaders, code generators,
//! and editors consume the resulting [`CftSchemaModule`] / [`CftSchemaType`] /
//! [`CftSchemaEnum`] reflection types.
//!
//! Diagnostics are stable across releases: every error carries an immutable
//! code (see [`CftErrorCode`]) and a stage tag (lex / syn / schema / type),
//! so tools can rely on numeric IDs rather than human-readable messages.

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
#![allow(
    clippy::missing_const_for_fn,
    clippy::redundant_pub_crate,
    clippy::use_self
)]

pub mod ast;
mod container;
mod error;
pub mod lexer;
pub mod parser;
mod schema;
mod span;

pub use container::{CftContainer, ModuleId};
pub use error::{CftDiagnostic, CftDiagnostics, CftErrorCode, CftLabel, CftSeverity, CftStage};
pub use schema::{
    CftAnnotation, CftAnnotationValue, CftConstValue, CftSchemaBinOp, CftSchemaCheckBlock,
    CftSchemaCheckExpr, CftSchemaCheckExprKind, CftSchemaCheckStmt, CftSchemaCmpOp, CftSchemaConst,
    CftSchemaDefaultValue, CftSchemaEnum, CftSchemaEnumVariant, CftSchemaField, CftSchemaModule,
    CftSchemaQuantifierKind, CftSchemaType, CftSchemaTypePredicate, CftSchemaTypeRef,
    CftSchemaUnaryOp,
};
pub use span::Span;
