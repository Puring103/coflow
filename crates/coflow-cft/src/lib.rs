//! Reference implementation of the **CFT** type-definition language used by
//! Coflow's data pipeline.
//!
//! See `website/docs/docs/reference/cft.md` for the language reference. The crate exposes
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
mod compiled_schema;
mod container;
mod dimensions;
mod error;
mod identifier;
pub mod lexer;
mod module_set;
pub mod parser;
mod schema;
mod span;

pub use coflow_structure::StructuralLimits;
pub use compiled_schema::{
    CftDimensionFieldMeta, CftEnumMeta, CftEnumValueMeta, CftEnumVariantMeta, CftFieldMeta,
    CftTypeMeta, CftSchema, TypedCheckPlan, TypedCheckSchedule, ValueDependencyCycle,
    ValueDependencyMode, ValueDependencyPlan, ValueDependencyStep,
};
pub use container::{build_schema, CftContainer, ModuleId};
pub use error::{CftDiagnostic, CftDiagnostics, CftErrorCode, CftLabel, CftSeverity, CftStage};
pub use identifier::{is_cft_identifier, is_cft_reserved_identifier, record_key_ident_error};
pub use module_set::{
    parse_modules, CftDimensions, CftFile, CftModuleFile, CftModuleSet, ParsedCftModule,
};
pub use parser::CftParseOptions;
pub use schema::{
    format_schema_type_ref, CftAnnotation, CftAnnotationValue, CftCompileOptions, CftConstValue,
    CftSchemaBinOp, CftSchemaCheckBlock, CftSchemaCheckExpr, CftSchemaCheckExprKind,
    CftSchemaCheckStmt, CftSchemaCmpOp, CftSchemaConst, CftSchemaDefaultValue, CftSchemaEnum,
    CftSchemaEnumVariant, CftSchemaField, CftSchemaModule, CftSchemaQuantifierKind, CftSchemaType,
    CftSchemaTypePredicate, CftSchemaTypeRef, CftSchemaUnaryOp, Dimension, DimensionSpec,
};
pub use span::Span;
