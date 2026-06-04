//! Source-neutral runtime data model for Coflow data.
//!
//! This crate is deliberately below every concrete loader. Excel, JSON, tests,
//! and editor integrations should all translate their input into
//! [`CfdInputRecord`] / [`CfdInputValue`] and then build a [`CfdDataModel`].
//! Runtime `check` execution lives in the `coflow-checker` crate so this crate
//! stays focused on data construction and reference resolution.

mod compiler;
mod diagnostic;
mod model;
mod schema_view;

pub use diagnostic::{
    CfdDiagnostic, CfdDiagnostics, CfdErrorCode, CfdLabel, CfdPath, CfdPathSegment, CfdSeverity,
    CfdStage,
};
pub use model::{
    CfdDataModel, CfdDictKey, CfdEnumValue, CfdIdValue, CfdIndexKey, CfdInputDictKey,
    CfdInputRecord, CfdInputValue, CfdModelBuilder, CfdPolymorphicIndex, CfdRecord, CfdRecordId,
    CfdTable, CfdValue,
};
