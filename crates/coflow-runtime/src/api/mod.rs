//! Internal CFD source/writer contract shared by the runtime and editor.
//!
//! Target-language generation lives in `coflow-codegen-api`; data export and
//! serialized loader contracts are intentionally not public from this crate.

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
#![allow(clippy::missing_const_for_fn)]

pub mod diagnostics;
pub mod operations;
pub mod provider;
mod bindings;
pub mod writer;

pub use diagnostics::{
    byte_position, byte_range, map_diagnostics_with_origins, origins_of, path_to_slash,
    source_location_display_path, spreadsheet_cell_name, Diagnostic, DiagnosticContext,
    DiagnosticSet, FlatDiagnostic, Label, Severity, SourceLocation, TextPosition, TextRange,
};
pub use operations::{
    CreateTableRequest, DimensionSourceEntry, DimensionSourceLoadRequest,
    DimensionSourceLoadResult, DimensionSourceManager, DimensionSourceManagerDescriptor,
    DimensionSourceOptionsRequest, DimensionSourceRequest, DimensionSourceResult,
    DimensionSourceSchema, RewriteDimensionRecordRequest,
    SyncHeaderRequest, TableAddressing, TableContext, TableHeaderOptions, TableManager,
    TableManagerDescriptor, TableOperationResult, WriteDimensionValueRequest,
};
pub use provider::{
    DecodedSourceOptions, LoadedSource, ProbeConfidence, ProbeResult,
    ProjectSourceRef, ResolvedSource, SourceLoadContext, SourceLocationSpec, CfdSourceAdapter,
    CfdSourceAdapterDescriptor, SourceResolveContext,
};
pub(crate) use bindings::{
    CfdBindingBundle, CfdBindingError, CfdProviderBindings, CfdSourceSelectionError,
};
pub use writer::{
    DeleteRecordRequest, InsertRecordRequest, RenameRecordRequest, ReorderRecordsOperation,
    ReorderRecordsRequest, SourceTransaction, SourceTransactionCompensation, SourceWriter,
    WriteBatchFailure,
    WriteCellRequest, WriteContext, WriteFieldPathSegment, WriteOutcome, WriteRecordRef,
    WriterCapabilities, WriterDescriptor,
};

pub(crate) use crate::catalog::CfdSourceCatalog;
