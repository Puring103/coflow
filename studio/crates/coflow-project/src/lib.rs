//! Studio 的共享项目层：配置、文件发现、源码工作流和命令编排。

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
mod artifacts;
mod catalog;
mod cfd_loader;
mod checks;
pub mod commands;
pub use coflow_codegen as codegen;
mod diff;
mod dimensions;
mod files;
mod indexes;
mod limits;
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
mod source_snapshot;
pub use source_snapshot::{CfdSourceSnapshot, CfdSourceStore};
mod statistics;
mod write_rules;
mod writes;

pub use api::*;
pub(crate) use coflow_core as data_model;
pub use coflow_core::serde_i64;
pub use coflow_core::{
    CfdDataModel, CfdDiagnostic, CfdDiagnostics, CfdDictKey, CfdDimensionFieldValues,
    CfdDimensionValue, CfdEnumValue, CfdErrorCode, CfdFormattedString, CfdFunction, CfdLabel,
    CfdObject, CfdPath, CfdPathSegment, CfdRecord, CfdRecordId, CfdSeverity, CfdStage, CfdTable,
    CfdValue, DimensionFieldLookupError, DimensionRefCoordinate, DimensionValueDraft,
    DimensionValueLookup, LoadedDictKeyDraft, LoadedFormattedString, LoadedFunction,
    LoadedRecordDraft, LoadedValueDraft, RecordCoordinate, RecordOrigin, TextSpan,
};
pub use diff::{
    ProjectDiff, ProjectDiffChange, ProjectDiffDiagnostic, ProjectDiffValue, ProjectFieldDiff,
    ProjectFileDiff, ProjectRecordDiff, ProjectRecordSnapshot,
};
pub use dimensions::{DimensionFieldInfo, DimensionInfo};
pub use files::FileTreeNode;
pub use indexes::{DiagnosticLogicalLocation, DiagnosticsStore, RejectedRecordRef};
pub use project::*;
// Re-export helpers that hosts (tauri editor, CLI) call when translating
// engine data to a wire format so they don't diverge in path formatting.
pub use coflow_core::schema::{DimensionName, FieldName, RecordKey, TypeName, VariantName};
pub use load::{format_cfd_path as format_field_path, DataSourceTextOverride};
pub use mutation::{
    apply_collection_edit, CollectionEdit, CreateFieldSource, CreateRecordDraft, CreateRecordFieldDraft, CreateRequiredInput,
    DefaultMaterialization, DimensionValueCoordinate, DimensionValueExpectation, MutationAppliedOp,
    MutationFailedOp, MutationFields, MutationOp, MutationReport, MutationRequest, MutationValue,
    ProjectFileUpdate,
};
pub use project_schema::SchemaTextOverride;
pub use query::ProjectQueries;
pub use records::{
    dict_key_path_text, value_summary, DimensionValueOrigin, DimensionValueState,
    DimensionValueView, EffectiveFieldWrite, FieldShapeInfo, IdAsEnumInfo, RecordReferenceInfo,
    RecordView, RefTargetInfo, WriteOutcome,
};
pub use runtime::{
    BuildProjectSession, PreparedSourceUpdate, ProjectRuntime, ReadOnlyProjectSession, Runtime, WriteProjectSession,
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
