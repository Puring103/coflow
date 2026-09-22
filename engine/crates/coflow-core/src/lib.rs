//! CFD runtime data model for Coflow.
//!
//! The model is built from parsed CFD records and is shared by checks, editor
//! queries, mutation planning, and target-language code generation. Test and
//! editor integrations may construct the same draft types, but no alternate
//! data format is part of the runtime contract.

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
#![allow(
    clippy::derive_partial_eq_without_eq,
    clippy::missing_const_for_fn,
    clippy::redundant_pub_crate,
    clippy::use_self
)]

mod callable;
pub use callable::{CallableLocation, CallableSource};
mod build;
pub mod check;
pub mod contract;
pub mod loading;
pub mod runtime;
pub mod schema;
pub mod vm;
pub(crate) use coflow_language::diagnostics::{CftDiagnostic, CftDiagnostics, CftErrorCode};
pub(crate) use coflow_language::lexical::is_cft_identifier;
#[cfg(feature = "cft-compiler")]
pub(crate) use coflow_language::lexical::is_cft_reserved_identifier;
pub(crate) use coflow_language::source::Span;
pub use coflow_language::{limits, source};
pub(crate) use schema::*;
mod diagnostics;
mod indexes;
mod ingest;
mod model;
mod semantics;
pub mod serde_float;
pub mod serde_i64;

pub use build::{CfdModelBuildOutput, CfdModelBuilder};
pub use coflow_language::limits::StructuralLimits;
pub use diagnostics::{
    format_cfd_dict_key, label_to_location, map_diagnostics, CfdDiagnostic, CfdDiagnostics,
    CfdErrorCode, CfdLabel, CfdPath, CfdPathSegment, Severity, CfdStage, MappedDiagnostic,
    MappedLabel, RecordOrigin, SourceLocation, TextSpan,
};
pub use ingest::{
    DimensionValueDraft, LoadedDictKeyDraft,
    LoadedRecordDraft, LoadedValueDraft,
};
pub use model::{
    CfdDataModel, CfdDictKey, CfdDimensionFieldValues, CfdDimensionValue, CfdEnumValue,
    CfdObject, CfdRecord, CfdRecordId, CfdTable, CfdValue,
    DimensionFieldLookupError, DimensionRefCoordinate, DimensionValueLookup, RecordCoordinate,
    RefEdge, RefSite,
};
pub use semantics::{
    validate_object_type_assignable, validate_value_for_schema, CfdValueSemanticContext,
    CfdValueSemanticError, CfdValueSemanticErrorKind, PendingInsertRef, ValueValidationMode,
    ValueValidationRequest,
};

#[cfg(test)]
mod allocation_probe;
