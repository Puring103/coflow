//! Shared project runtime for Coflow hosts.

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
#![allow(clippy::multiple_crate_versions)]

mod api;
mod catalog;
mod cfd_loader;
#[cfg(feature = "internal-check-bench")]
#[doc(hidden)]
pub mod check_benchmark_support;
pub mod checker;
mod checks;
pub mod codegen;
pub mod data_model;
mod dimensions;
mod files;
mod indexes;
mod load;
mod mutation;
mod project;
mod project_schema;
mod query;
mod records;
mod runtime;
mod schema_diagnostics;
mod schema_inspect;
mod search;
mod session;
mod session_build;
mod source_resolution;
mod statistics;
mod write_rules;
mod writes;

pub use api::*;
pub use cfd_loader::{
    load_cfd_model, parse_cfd_input_records, CfdTextDiagnostic, CfdTextDiagnostics,
    CfdTextErrorCode, CfdTextLoadError, CfdTextSpan,
};
pub use checker::{
    execute_checks, CheckDiagnostic, CheckDiagnosticContext, CheckExecutionStats, CheckLimits,
    CheckOutput, CheckProjection, CheckSchemaLocation, CheckTarget, CheckTask, CheckTaskResult,
};
pub use data_model::cell_value;
pub use data_model::serde_i64;
pub use data_model::{
    validate_object_type_assignable, validate_value_for_schema, CfdDataModel, CfdDiagnostic,
    CfdDiagnostics, CfdDictKey, CfdDimensionFieldValues, CfdDimensionValue, CfdEnumValue,
    CfdErrorCode, CfdFormattedString, CfdFunction, CfdLabel, CfdModelBuildOutput, CfdModelBuilder,
    CfdObject, CfdPath, CfdPathSegment, CfdRecord, CfdRecordId, CfdSeverity, CfdStage, CfdTable,
    CfdValue, CfdValueSemanticContext, CfdValueSemanticError, CfdValueSemanticErrorKind,
    DimensionFieldLookupError, DimensionRefCoordinate, DimensionValueDraft, DimensionValueLookup,
    LoadedDictKeyDraft, LoadedFieldReference, LoadedFormatSegment, LoadedFormattedString,
    LoadedFunction, LoadedRecordDraft, LoadedValueDraft, MappedDiagnostic, MappedLabel,
    PendingInsertRef, RecordCoordinate, RecordOrigin, RefEdge, RefSite, TextSpan,
    ValueValidationMode, ValueValidationRequest,
};
pub use dimensions::{DimensionFieldInfo, DimensionInfo};
pub use files::FileTreeNode;
pub use indexes::{DiagnosticLogicalLocation, DiagnosticsStore, RejectedRecordRef};
pub use project::*;
// Re-export helpers that hosts (tauri editor, CLI) call when translating
// engine data to a wire format so they don't diverge in path formatting.
pub use coflow_language::StructuralLimits;
pub use coflow_language::{DimensionName, FieldName, RecordKey, TypeName, VariantName};
pub use load::{format_cfd_path as format_field_path, DataSourceTextOverride};
pub use mutation::{
    CreateFieldSource, CreateRecordDraft, CreateRecordFieldDraft, CreateRequiredInput,
    DefaultMaterialization, DimensionValueCoordinate, DimensionValueExpectation, MutationAppliedOp,
    MutationFailedOp, MutationFields, MutationOp, MutationReport, MutationRequest, MutationValue,
};
pub use project_schema::SchemaTextOverride;
pub use query::ProjectQueries;
pub use records::{
    dict_key_path_text, value_summary, DimensionValueOrigin, DimensionValueState,
    DimensionValueView, EffectiveFieldWrite, FieldShapeInfo, IdAsEnumInfo, RecordReferenceInfo,
    RecordView, RefTargetInfo, WriteOutcome,
};
pub use runtime::{
    BuildProjectSession, ProjectRuntime, ReadOnlyProjectSession, Runtime, WriteProjectSession,
};
pub use schema_inspect::{
    inspect_schema, schema_files, SchemaConstInfo, SchemaConstValueInfo, SchemaDefaultValueInfo,
    SchemaDimensionFieldInfo, SchemaDimensionInfo, SchemaEnumInfo, SchemaEnumVariantInfo,
    SchemaFieldInfo, SchemaFileInfo, SchemaFilesReport, SchemaInspectReport, SchemaTypeInfo,
    SchemaTypeRefInfo,
};
pub use search::{RecordSearchHit, RecordSearchMode, RecordSearchOptions, RecordSearchResults};
pub use session::ProjectSchemaSession;
pub(crate) use session::ProjectSession;
pub use statistics::ProjectExecutionStats;
