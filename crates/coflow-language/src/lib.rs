//! Unified CFT/CFD language implementation.
//!
//! CFT schema compilation and CFD syntax parsing intentionally remain separate
//! modules, but share one crate so spans, structural limits and diagnostics do
//! not cross a crate boundary. CFD parsing is schema-free and can be used by
//! editor hosts before a schema is available.

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

pub mod cfd;
pub mod limits;

pub use diagnostics::{
    CftDiagnostic, CftDiagnostics, CftErrorCode, CftLabel, CftSeverity, CftStage,
};
pub use module::{parse_modules, CftFile, CftModule, CftModuleSet, ModuleId};
pub use schema::{
    build_schema, BucketName, CftAnnotation, CftAnnotationValue, CftCheckBuiltin, CftConst, CftConstValue, CftDimension,
    CftDimensionInput, CftDimensionInputError, CftDimensionInputs, CftDisplayMetadata, CftEnum,
    CftEnumValue, CftEnumVariant, CftField, CftFieldDimension, CftNameError, CftSchema,
    CftSchemaBinOp, CftSchemaCheckBlock, CftSchemaCheckExpr, CftSchemaCheckExprKind,
    CftSchemaCheckFormatSegment, CftSchemaCheckMessage, CftSchemaCheckMessageKind,
    CftSchemaCheckStmt, CftSchemaCmpOp, CftSchemaDefaultValue, CftSchemaQuantifierBindings,
    CftSchemaQuantifierKind, CftSchemaSource, CftSchemaTypePredicate, CftSchemaUnaryOp,
    CftFunctionParameter, CftTopLevelCheck, CftType, CftValueType, CheckDependency, CheckField,
    CheckName, CheckOwner, CheckStatementId, CheckStatementInfo, CheckStatementRef, ConstName,
    DimensionName, EnumName, EnumVariantName, FieldName, RecordKey, TypeName, ValueDependencyCycle,
    ValueDependencyMode, ValueDependencyPlan, ValueDependencyStep, VariantName,
};
pub use syntax::{is_cft_identifier, is_cft_reserved_identifier, record_key_ident_error, Span};

pub use cfd::{
    parse_cfd, parse_cfd_with_options, CfdAst, CfdBitExpr, CfdBitExprKind, CfdBitOp, CfdBlock,
    CfdField, CfdFieldReference, CfdFormatSegment, CfdFormattedString, CfdParseOptions, CfdRecord,
    CfdRef, CfdSyntaxDiagnostic, CfdValue,
};
pub use limits::{
    BudgetAxis, BudgetExceeded, StructuralBudget, StructuralLimits, StructureKind, TraversalCursor,
};
