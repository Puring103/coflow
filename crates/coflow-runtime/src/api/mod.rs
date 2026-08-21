//! Internal CFD source/writer contract shared by the runtime and editor.
//!
//! Target-language generation lives in the runtime `codegen` module; runtime data stays
//! in the typed CFD model and is never exposed as a serialized export contract.

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
pub mod source;
pub mod writer;

pub use diagnostics::{
    byte_position, byte_range, map_diagnostics_with_origins, origins_of, path_to_slash,
    source_location_display_path, Diagnostic, DiagnosticContext, DiagnosticSet, FlatDiagnostic,
    Label, Severity, SourceLocation, TextPosition, TextRange,
};
pub use operations::{
    CfdDimensionWriter, CfdDimensionWriterDescriptor, CfdWriteContext, DimensionSourceEntry,
    DimensionSourceLoadRequest, DimensionSourceLoadResult, DimensionSourceRequest,
    DimensionSourceResult, DimensionSourceSchema, RewriteDimensionRecordRequest,
    WriteDimensionValueRequest,
};
pub use source::{CfdLoadContext, CfdSource, CfdSourcePath, LoadedCfdSource};
pub use writer::{
    CfdDocumentWriter, CfdWriterDescriptor, DeleteRecordRequest, InsertRecordRequest,
    RenameRecordRequest, ReorderRecordsOperation, ReorderRecordsRequest, SourceTransaction,
    SourceTransactionCompensation, WriteBatchFailure, WriteCellRequest, WriteContext,
    WriteFieldPathSegment, WriteOutcome, WriteRecordRef, WriterCapabilities,
};

pub(crate) use crate::catalog::CfdSourceCatalog;
