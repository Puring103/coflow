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

#[cfg(feature = "internal-check-bench")]
#[doc(hidden)]
pub mod check_benchmark_support;
mod checks;
mod data_files;
mod data_read;
mod dimensions;
mod files;
mod indexes;
mod load;
mod mutation;
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

pub use data_files::{
    create_data_file, sync_data_header, DataCreateFileOptions, DataFileReport,
    DataSyncHeaderOptions,
};
pub use data_read::{
    data_get, data_list, data_sources, DataGetQuery, DataGetReport, DataListQuery, DataListReport,
    DataRecordInfo, DataRecordSummary, DataSourceInfo, DataSourcesReport,
};
pub use dimensions::{DimensionFieldInfo, DimensionInfo};
pub use files::FileTreeNode;
pub use indexes::{DiagnosticLogicalLocation, DiagnosticsStore, RejectedRecordRef};
// Re-export helpers that hosts (tauri editor, CLI) call when translating
// engine data to a wire format so they don't diverge in path formatting.
pub use coflow_cft::{DimensionName, FieldName, RecordKey, TypeName, VariantName};
pub use coflow_data_model::{CfdPathSegment, RecordCoordinate};
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
pub use search::{RecordSearchHit, RecordSearchMode, RecordSearchResults};
pub use session::ProjectSchemaSession;
pub(crate) use session::ProjectSession;
pub use statistics::ProjectExecutionStats;
