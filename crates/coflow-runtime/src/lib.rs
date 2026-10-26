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
pub mod automation;
mod catalog;
mod cfd_loader;
#[cfg(feature = "internal-check-bench")]
#[doc(hidden)]
pub mod check_benchmark_support;
mod checks;
pub mod commands;
mod value_drafts;
mod value_path;
pub use coflow_codegen as codegen;
mod dimensions;
mod files;
mod indexes;
mod inspection;
mod limits;
mod loading;
mod mutation;
mod project;
mod publication;
mod sessions;
pub use publication::operation::{publish_source, PersistenceState, PublicationReport};
pub use publication::PublishedFile;
mod write_rules;
mod writes;

pub use api::*;
pub use coflow_checker::{
    execute_checks, CheckDiagnostic, CheckDiagnosticContext, CheckExecutionStats, CheckLimits,
    CheckOutput, CheckSchemaLocation, CheckTarget, CheckTask, CheckTaskResult,
};
pub use coflow_model as data_model;
pub use coflow_model::cell_value;
pub use coflow_model::serde_i64;
pub use coflow_model::{
    validate_object_type_assignable, validate_value_for_schema, CfdDataModel, CfdDiagnostic,
    CfdDiagnostics, CfdDictKey, CfdDimensionFieldValues, CfdDimensionValue, CfdEnumValue,
    CfdErrorCode, CfdFormattedString, CfdLabel, CfdModelBuildOutput, CfdModelBuilder, CfdObject,
    CfdPath, CfdPathSegment, CfdRecord, CfdRecordHandle, CfdSeverity, CfdStage, CfdTable, CfdValue,
    CfdValueSemanticContext, CfdValueSemanticError, CfdValueSemanticErrorKind,
    DimensionFieldLookupError, DimensionRefCoordinate, DimensionValueDraft, DimensionValueLookup,
    LoadedDictKeyDraft, LoadedFieldReference, LoadedFormatSegment, LoadedFormattedString,
    LoadedRecordDraft, LoadedValueDraft, MappedDiagnostic, MappedLabel, PendingInsertRef,
    RecordCoordinate, RecordOrigin, RefEdge, RefSite, TextSpan, ValueValidationMode,
    ValueValidationRequest,
};
pub use dimensions::{DimensionFieldInfo, DimensionInfo};
pub use files::FileTreeNode;
pub use indexes::{DiagnosticLogicalLocation, DiagnosticsStore, RejectedRecordRef};
pub use inspection::diff::{
    ProjectDiff, ProjectDiffChange, ProjectDiffDiagnostic, ProjectDiffDisplay, ProjectDiffValue,
    ProjectFieldDiff, ProjectFileDiff, ProjectRecordDiff, ProjectRecordSnapshot,
};
pub use project::*;
// Re-export helpers that hosts (tauri editor, CLI) call when translating
// engine data to a wire format so they don't diverge in path formatting.
pub use coflow_language::cft::{DimensionName, FieldName, RecordId, TypeName, VariantName};
pub use inspection::query::ProjectQueries;
pub use inspection::records::{
    dict_key_path_text, value_summary, DimensionValueOrigin, DimensionValueState,
    DimensionValueView, EffectiveFieldWrite, FieldShapeInfo, IdAsEnumInfo, RecordReferenceInfo,
    RecordView, RefTargetInfo, WriteOutcome,
};
pub use inspection::schema_inspect::{
    inspect_schema, schema_files, SchemaConstInfo, SchemaConstValueInfo, SchemaDefaultValueInfo,
    SchemaDimensionFieldInfo, SchemaDimensionInfo, SchemaEnumInfo, SchemaEnumVariantInfo,
    SchemaFieldInfo, SchemaFileInfo, SchemaFilesReport, SchemaInspectReport, SchemaTypeInfo,
    SchemaTypeRefInfo,
};
pub use inspection::search::{
    RecordSearchHit, RecordSearchMode, RecordSearchOptions, RecordSearchResults,
};
pub use inspection::statistics::ProjectExecutionStats;
pub use loading::load::{format_cfd_path as format_field_path, DataSourceTextOverride};
pub use loading::project_schema::SchemaTextOverride;
pub use mutation::{
    CreateFieldSource, CreateRecordDraft, CreateRecordFieldDraft, CreateRequiredInput,
    DefaultMaterialization, DimensionValueCoordinate, DimensionValueExpectation, MutationAppliedOp,
    MutationFailedOp, MutationFields, MutationOp, MutationReport, MutationRequest, MutationValue,
    ProjectFileUpdate,
};
pub use sessions::runtime::{
    BuildProjectSession, ProjectRefreshContext, ProjectRuntime, ReadOnlyProjectSession, Runtime,
    WriteProjectSession,
};
pub use sessions::session::ProjectSchemaSession;
pub(crate) use sessions::session::ProjectSession;
