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
