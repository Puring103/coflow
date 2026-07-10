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

mod data_files;
mod data_patch;
mod data_read;
mod dimensions;
mod files;
mod indexes;
mod load;
mod mutation;
mod records;
mod runtime;
mod schema_build;
mod schema_inspect;
mod session;
mod session_build;
mod write_rules;
mod writes;

pub use data_files::{
    create_data_file, sync_data_header, DataCreateFileOptions, DataFileReport,
    DataSyncHeaderOptions,
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
pub use indexes::{
    DiagnosticLogicalLocation, DiagnosticsStore, FileIndex, RecordIndex, RecordRef,
    RejectedRecordRef, ResolvedSourceEntry, SourceId, SourceIndex,
};
// Re-export helpers that hosts (tauri editor, CLI) call when translating
// engine data to a wire format so they don't diverge in path formatting.
pub use load::{configured_project_source, format_cfd_path as format_field_path};
pub use mutation::{
    CreateFieldSource, CreateRecordDraft, CreateRecordFieldDraft, CreateRequiredInput,
    DefaultMaterialization, MutationAppliedOp, MutationFailedOp, MutationFields, MutationOp,
    MutationReport, MutationRequest, MutationValue, PreparedMutation,
};
pub use records::{
    dict_key_path_text, value_summary, EffectiveFieldWrite, RecordTarget, RecordView,
    RefTargetInfo, WriteOutcome,
};
pub use runtime::{BuildProjectSession, ReadOnlyProjectSession, Runtime};
pub use schema_build::build_project_schema_session;
pub use schema_inspect::{
    inspect_schema, schema_files, SchemaAnnotation, SchemaAnnotationValueInfo, SchemaConstInfo,
    SchemaConstValueInfo, SchemaDefaultValueInfo, SchemaEnumInfo, SchemaEnumVariantInfo,
    SchemaFieldInfo, SchemaFileInfo, SchemaFilesReport, SchemaInspectReport, SchemaTypeInfo,
    SchemaTypeRefInfo,
};
pub use session::{ProjectSchemaSession, ProjectSession, RecordCoordinate};
pub use session_build::{build_project_session_for_build, open_project_session_read_only};
