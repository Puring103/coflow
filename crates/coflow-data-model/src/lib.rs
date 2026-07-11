//! Source-neutral runtime data model for Coflow data.
//!
//! This crate is deliberately below every concrete loader. Excel, JSON, tests,
//! and editor integrations should all translate their input into
//! [`CfdInputRecord`] / [`CfdInputValue`] and then build a [`CfdDataModel`].
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
    clippy::too_many_lines,
    clippy::use_self
)]

mod compiler;
mod compiler_context;
mod diagnostic;
mod edge_index;
mod model;
mod origin;
pub mod serde_i64;
mod value_semantics;

pub use diagnostic::{
    format_cfd_dict_key, CfdDiagnostic, CfdDiagnostics, CfdErrorCode, CfdLabel, CfdPath,
    CfdPathSegment, CfdSeverity, CfdStage,
};
pub use model::{
    CfdDataModel, CfdDictKey, CfdDomainId, CfdDomainIndex, CfdEnumValue, CfdInputDictKey,
    CfdInputRecord, CfdInputValue, CfdModelBuilder, CfdObject, CfdPolymorphicIndex, CfdRecord,
    CfdRecordId, CfdTable, CfdTypeId, CfdValue, DimensionFieldLookupError, DimensionFieldValue,
    RefEdge, RefEdgeId, RefSite, SpreadEdge, SpreadEdgeId, SpreadSite,
};
pub use origin::{
    label_to_location, map_diagnostics, MappedDiagnostic, MappedLabel, RecordOrigin,
    SourceDocument, SourceLocation, TextSpan,
};
pub use value_semantics::{
    validate_complete_value_for_schema, validate_fragment_value_for_schema,
    validate_object_type_assignable, CfdValueSemanticContext, CfdValueSemanticError,
    PendingInsertRef,
};
