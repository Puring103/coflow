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

mod diagnostics;
mod dimensions;
mod engine;
mod eval;
mod operations;
mod output;
mod task;

use coflow_model::CfdDataModel;
pub use coflow_language::limits::StructuralLimits;
use coflow_language::CftSchema;
pub use diagnostics::{CheckDiagnostic, CheckDiagnosticContext, CheckSchemaLocation};
pub use output::{CheckExecutionStats, CheckOutput, CheckTaskResult};
pub use task::{CheckLimits, CheckProjection, CheckTarget, CheckTask};

/// Executes the requested CFT `check` statement tasks and projections.
///
/// Empty targets perform no work. Diagnostics always retain the record that
/// initiated evaluation, including failures reported on values reached through
/// references.
#[must_use]
pub fn execute_checks(
    schema: &CftSchema,
    model: &CfdDataModel,
    tasks: impl IntoIterator<Item = CheckTask>,
    limits: CheckLimits,
) -> CheckOutput {
    engine::execute_tasks(schema, model, tasks, limits)
}
