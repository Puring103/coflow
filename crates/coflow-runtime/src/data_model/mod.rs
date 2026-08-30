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

mod build;
pub mod cell_value;
mod dependencies;
mod diagnostics;
mod formatted;
mod indexes;
mod ingest;
mod model;
mod semantics;
pub mod serde_i64;

pub use build::{CfdModelBuildOutput, CfdModelBuilder};
pub use coflow_language::limits::StructuralLimits;
pub use diagnostics::{
    format_cfd_dict_key, label_to_location, map_diagnostics, CfdDiagnostic, CfdDiagnostics,
    CfdErrorCode, CfdLabel, CfdPath, CfdPathSegment, CfdSeverity, CfdStage, MappedDiagnostic,
    MappedLabel, RecordOrigin, SourceLocation, TextSpan,
};
pub use formatted::{evaluate_formatted_string, stringify_value};
pub use ingest::{
    DimensionValueDraft, LoadedDictKeyDraft, LoadedFieldReference, LoadedFormatSegment,
    LoadedFormattedString, LoadedFunction, LoadedRecordDraft, LoadedValueDraft,
};
pub use model::{
    CfdDataModel, CfdDictKey, CfdDimensionFieldValues, CfdDimensionValue, CfdEnumValue,
    CfdFormattedString, CfdFunction, CfdObject, CfdRecord, CfdRecordId, CfdTable, CfdValue,
    DimensionFieldLookupError, DimensionRefCoordinate, DimensionValueLookup, RecordCoordinate,
    RefEdge, RefSite,
};
pub use semantics::{
    validate_object_type_assignable, validate_value_for_schema, CfdValueSemanticContext,
    CfdValueSemanticError, CfdValueSemanticErrorKind, PendingInsertRef, ValueValidationMode,
    ValueValidationRequest,
};
