//! Reference implementation of the **CFT** type-definition language used by
//! Coflow's data pipeline.
//!
//! See `website/docs/docs/reference/cft.md` for the language reference. Hosts
//! collect source files with [`CftFile`], parse them once with
//! [`parse_modules`], and build the immutable effective schema with
//! [`build_schema`]. Loaders, code generators, and editors consume the
//! resulting [`CftSchema`] and its canonical [`CftType`], [`CftField`], and
//! [`CftEnum`] declarations.
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
mod build;
mod compiled;
mod dimensions;
mod error;
mod identifier;
pub mod lexer;
mod module_id;
mod module_set;
mod names;
pub mod parser;
mod schema;
mod span;

pub use build::build_schema;
pub use coflow_structure::StructuralLimits;
pub use compiled::{
    format_schema_type_ref, CftConst, CftConstValue, CftDimension, CftEnum, CftEnumVariant,
    CftField, CftFieldDimension, CftSchemaBinOp, CftSchemaCheckBlock, CftSchemaCheckExpr,
    CftSchemaCheckExprKind, CftSchemaCheckStmt, CftSchemaCmpOp, CftSchemaDefaultValue,
    CftSchemaQuantifierKind, CftSchemaTypePredicate, CftSchemaTypeRef, CftSchemaUnaryOp, CftType,
};
pub use error::{CftDiagnostic, CftDiagnostics, CftErrorCode, CftLabel, CftSeverity, CftStage};
pub use identifier::{is_cft_identifier, is_cft_reserved_identifier, record_key_ident_error};
pub use module_id::ModuleId;
pub use module_set::{
    parse_modules, CftDimensionInput, CftDimensionInputs, CftFile, CftModule, CftModuleSet,
};
pub use names::{
    BucketName, CftNameError, ConstName, DimensionName, EnumName, EnumVariantName, FieldName,
    RecordKey, TypeName, VariantName,
};
pub use parser::CftParseOptions;
pub use schema::{
    CftEnumValue, CftSchema, TypedCheckPlan, TypedCheckSchedule, ValueDependencyCycle,
    ValueDependencyMode, ValueDependencyPlan, ValueDependencyStep,
};
pub use span::Span;
