//! Source-neutral runtime data model for Coflow data.
//!
//! This crate is deliberately below every concrete loader. Excel, JSON, tests,
//! and editor integrations should all translate their input into
//! [`LoadedRecordDraft`] / [`LoadedValueDraft`] and then build a [`CfdDataModel`].
//! Runtime `check` execution lives in the `coflow-checker` crate so this crate
//! stays focused on data construction and reference resolution.

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

mod build;
pub mod cell_value;
mod dependencies;
mod diagnostics;
mod indexes;
mod ingest;
mod model;
mod semantics;
pub mod serde_i64;

pub use build::CfdModelBuilder;
pub use coflow_structure::StructuralLimits;
pub use diagnostics::{
    format_cfd_dict_key, label_to_location, map_diagnostics, CfdDiagnostic, CfdDiagnostics,
    CfdErrorCode, CfdLabel, CfdPath, CfdPathSegment, CfdSeverity, CfdStage, MappedDiagnostic,
    MappedLabel, RecordOrigin, SourceDocument, SourceLocation, TextSpan,
};
pub use ingest::{DimensionValueDraft, LoadedDictKeyDraft, LoadedRecordDraft, LoadedValueDraft};
pub use model::{
    CfdDataModel, CfdDictKey, CfdDimensionFieldValues, CfdDimensionValue, CfdEnumValue, CfdObject,
    CfdRecord, CfdRecordId, CfdTable, CfdValue, DimensionFieldLookupError, DimensionRefCoordinate,
    DimensionValueLookup, RecordCoordinate, RefEdge, RefSite, SpreadEdge,
};
pub use semantics::{
    validate_object_type_assignable, validate_value_for_schema, CfdValueSemanticContext,
    CfdValueSemanticError, CfdValueSemanticErrorKind, PendingInsertRef, ValueValidationMode,
    ValueValidationRequest,
};
