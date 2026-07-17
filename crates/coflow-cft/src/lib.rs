//! Reference implementation of the **CFT** type-definition language used by
//! Coflow's data pipeline.
//!
//! See `website/docs/docs/reference/03-language/01-cft.md` for the language reference. Hosts
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

mod diagnostics;
mod module;
mod schema;
pub mod syntax;

pub use diagnostics::{
    CftDiagnostic, CftDiagnostics, CftErrorCode, CftLabel, CftSeverity, CftStage,
};
pub use module::{parse_modules, CftFile, CftModule, CftModuleSet, ModuleId};
pub use schema::{
    build_schema, BucketName, CftConst, CftConstValue, CftDimension, CftDimensionInput,
    CftDimensionInputError, CftDimensionInputs, CftEnum, CftEnumValue, CftEnumVariant, CftField,
    CftFieldDimension, CftNameError, CftSchema, CftSchemaBinOp, CftSchemaCheckBlock,
    CftSchemaCheckExpr, CftSchemaCheckExprKind, CftSchemaCheckStmt, CftSchemaCmpOp,
    CftSchemaDefaultValue, CftSchemaQuantifierKind, CftSchemaTypePredicate, CftSchemaUnaryOp,
    CftType, CftValueType, ConstName, DimensionName, EnumName, EnumVariantName, FieldName,
    RecordKey, ScheduledCheckBlock, TypeName, TypedCheckPlan, TypedCheckSchedule,
    ValueDependencyCycle, ValueDependencyMode, ValueDependencyPlan, ValueDependencyStep,
    VariantName,
};
pub use syntax::{is_cft_identifier, is_cft_reserved_identifier, record_key_ident_error, Span};
