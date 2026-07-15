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

mod checks;
mod data_files;
mod data_patch;
mod data_read;
mod dimensions;
mod files;
mod indexes;
mod load;
mod mutation;
mod query;
mod records;
mod runtime;
mod project_schema;
mod schema_diagnostics;
mod schema_inspect;
mod session;
mod session_build;
mod source_resolution;
mod write_rules;
mod writes;

pub use data_files::{
    create_data_file, sync_data_header, table_header_layout, DataCreateFileOptions, DataFileReport,
    DataSyncHeaderOptions, TableHeaderLayout,
};
pub use data_patch::{
    DataPatchAppliedOp, DataPatchFailedOp, DataPatchOp, DataPatchReport, DataPatchRequest,
    PatchPathSegment, PatchRecordSelector,
};
pub use data_read::{
    data_get, data_list, data_sources, DataGetQuery, DataGetReport, DataListQuery, DataListReport,
    DataRecordInfo, DataRecordSummary, DataSourceInfo, DataSourcesReport,
};
pub use dimensions::{
    builtin_display_name as dimension_builtin_display_name, dimensions_for_project,
    resolved_display_name as dimension_resolved_display_name, DimensionFieldInfo, DimensionInfo,
};
pub use files::{DimensionGroup, FileTreeNode, FileTreeOptions};
pub use indexes::{DiagnosticLogicalLocation, DiagnosticsStore, RejectedRecordRef};
// Re-export helpers that hosts (tauri editor, CLI) call when translating
// engine data to a wire format so they don't diverge in path formatting.
pub use load::format_cfd_path as format_field_path;
pub use mutation::{
    CreateFieldSource, CreateRecordDraft, CreateRecordFieldDraft, CreateRequiredInput,
    DefaultMaterialization, MutationAppliedOp, MutationFailedOp, MutationFields, MutationOp,
    DimensionValueCoordinate, DimensionValueSelector, MutationReport, MutationRequest,
    MutationValue,
};
pub use query::ProjectQueries;
pub use records::{
    dict_key_path_text, value_summary, EffectiveFieldWrite, FieldShapeInfo, IdAsEnumInfo,
    RecordReferenceInfo, RecordTarget, RecordView, RefTargetInfo, WriteOutcome,
};
pub use runtime::{
    BuildProjectSession, ProjectRuntime, ReadOnlyProjectSession, Runtime, WriteProjectSession,
};
pub use project_schema::SchemaTextOverride;
pub use schema_inspect::{
    inspect_schema, schema_files, SchemaConstInfo, SchemaDimensionFieldInfo, SchemaDimensionInfo,
    SchemaConstValueInfo, SchemaDefaultValueInfo, SchemaEnumInfo, SchemaEnumVariantInfo,
    SchemaFieldInfo, SchemaFileInfo, SchemaFilesReport, SchemaInspectReport, SchemaTypeInfo,
    SchemaTypeRefInfo,
};
pub(crate) use session::ProjectSession;
pub use session::{ProjectSchemaSession, RecordCoordinate};
