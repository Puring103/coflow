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

pub use artifacts::{ArtifactContent, ArtifactContentKind, ArtifactFile, ArtifactSet};
pub use codegen::{CodeGenerator, CodegenContext, CodegenDescriptor};
pub use data_output::{DataExporter, ExportContext, ExporterDescriptor};
pub use diagnostics::{
    map_diagnostics_with_origins, origins_of, Diagnostic, DiagnosticSet, FlatDiagnostic, Label,
    Severity, SourceLocation,
};
pub use operations::{
    CreateTableRequest, DimensionSourceEntry, DimensionSourceManager,
    DimensionSourceManagerDescriptor, DimensionSourceRequest, DimensionSourceResult,
    SyncHeaderRequest, TableContext, TableManager, TableManagerDescriptor, TableOperationResult,
};
pub use provider::{
    DataLoader, LoadContext, LoadedRecords, LoaderDescriptor, OutputSpec, ProbeConfidence,
    ProbeResult, ProjectSourceRef, ResolvedSource, SourceLocationSpec, SourceResolveContext,
};
pub use registry::{LoaderSelectionError, ProviderRegistrationError, ProviderRegistry};
pub use writer::{
    DataWriter, DeleteRecordRequest, InsertRecordRequest, RenameRecordRequest,
    RewriteRecordReferencesRequest, SpreadRewriteTarget, WriteCellRequest, WriteContext,
    WriteFieldPathSegment, WriteOutcome, WriterCapabilities, WriterDescriptor,
};

pub use coflow_cft::{
    CftAnnotation, CftAnnotationValue, CftContainer, CftSchemaEnum, CftSchemaField, CftSchemaType,
    CftSchemaTypeRef, ModuleId,
};
pub use coflow_data_model::{
    CfdDataModel, CfdDiagnostic, CfdDiagnostics, CfdDictKey, CfdInputRecord, CfdInputValue,
    CfdLabel, CfdObject, CfdPath, CfdPathSegment, CfdRecord, CfdRecordId, CfdTable, CfdValue,
    RecordOrigin, SourceDocument, TextSpan,
};
