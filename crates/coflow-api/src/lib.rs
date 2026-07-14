//! Public provider API for Coflow loaders, exporters, and code generators.

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

pub mod artifacts;
pub mod codegen;
pub mod data_output;
pub mod diagnostics;
pub mod operations;
pub mod provider;
pub mod registry;
pub mod writer;

pub use artifacts::{
    ArtifactContent, ArtifactContentKind, ArtifactFile, ArtifactSet, ArtifactSetError,
};
pub use codegen::{CodeGenerator, CodegenContext, CodegenDescriptor};
pub use data_output::{DataExporter, ExportContext, ExporterDescriptor};
pub use diagnostics::{
    byte_position, byte_range, map_diagnostics_with_origins, origins_of, path_to_slash,
    source_location_display_path, spreadsheet_cell_name, Diagnostic, DiagnosticSet, FlatDiagnostic,
    Label, Severity, SourceLocation, TextPosition, TextRange,
};
pub use operations::{
    CreateTableRequest, DimensionSourceEntry, DimensionSourceManager,
    DimensionSourceManagerDescriptor, DimensionSourceOptionsRequest, DimensionSourceRequest,
    DimensionSourceResult, SyncHeaderRequest, TableAddressing, TableContext, TableHeaderOptions,
    TableManager, TableManagerDescriptor, TableOperationResult,
};
pub use provider::{
    DecodedOutputOptions, DecodedSourceOptions, LoadedSource, ProbeConfidence, ProbeResult,
    ProjectSourceRef, ResolvedSource, SourceLoadContext, SourceLocationSpec, SourceProvider,
    SourceProviderDescriptor, SourceResolveContext,
};
pub use registry::{
    ProviderBundle, ProviderRegistrationError, ProviderRegistry, SourceProviderSelectionError,
};
pub use writer::{
    DeleteRecordRequest, InsertRecordRequest, RenameRecordRequest, RewriteRecordReferencesRequest,
    SourceTransaction, SourceWriter, SpreadRewriteTarget,
    WriteBatchFailure, WriteCellRequest, WriteContext, WriteFieldPathSegment, WriteOutcome,
    WriterCapabilities, WriterDescriptor,
};
