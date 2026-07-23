//! Runtime CFT `check` execution for already-built Coflow data models.

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
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::derive_partial_eq_without_eq,
    clippy::float_cmp,
    clippy::match_same_arms,
    clippy::missing_const_for_fn,
    clippy::option_if_let_else,
    clippy::redundant_pub_crate,
    clippy::single_match_else,
    clippy::use_self
)]

mod dependencies;
mod diagnostics;
mod dimensions;
mod engine;
mod eval;
mod operations;
mod output;
mod request;
mod snapshot;

use coflow_cft::CftSchema;
use coflow_data_model::CfdDataModel;
pub use coflow_structure::StructuralLimits;
pub use dependencies::{DependencyGraph, RecordReadDependency};
pub use diagnostics::{CheckDiagnostic, CheckDiagnosticContext, CheckSchemaLocation};
pub use dimensions::{DimensionCheckRound, DimensionCheckRoundError};
pub use output::{CheckExecutionId, CheckExecutionStats, CheckOutput, RootedCheckDiagnostic};
pub use request::{ChangedPaths, CheckChangeSet, CheckRequest, CheckTargets, DependencyCollection};
pub(crate) use snapshot::CheckRound;
pub use snapshot::CheckSnapshot;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DimensionCheckContext {
    pub(crate) dimension: coflow_cft::DimensionName,
    pub(crate) variant: coflow_cft::VariantName,
}

/// Executes the requested CFT `check` roots and dimension rounds.
///
/// Empty targets perform no work. Diagnostics always retain the record that
/// initiated evaluation, including failures reported on values reached through
/// references.
#[must_use]
pub fn run_checks(
    schema: &CftSchema,
    model: &CfdDataModel,
    request: CheckRequest<'_>,
) -> CheckOutput {
    engine::execute(schema, model, request)
}
