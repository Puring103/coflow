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
    clippy::needless_pass_by_ref_mut,
    clippy::needless_pass_by_value,
    clippy::option_if_let_else,
    clippy::redundant_pub_crate,
    clippy::single_match_else,
    clippy::too_many_lines,
    clippy::unused_self,
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

use coflow_cft::CftSchema;
use coflow_data_model::CfdDataModel;
pub use coflow_structure::StructuralLimits;
pub use dependencies::DependencyGraph;
pub use dimensions::DimensionCheckRound;
use engine::CheckRunner;
pub use output::{CheckExecutionStats, CheckOutput, RootedCheckDiagnostic};
pub use request::{CheckRequest, CheckTargets, DependencyCollection};

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
    let targets = match request.targets {
        CheckTargets::All => model.records().map(|(id, _)| id).collect::<Vec<_>>(),
        CheckTargets::Records(targets) => targets.to_vec(),
    };
    if targets.is_empty() {
        return CheckOutput::empty(request.dependency_collection);
    }

    let collect_dependencies = request.dependency_collection == DependencyCollection::Reads;
    let (default_diagnostics, mut dependencies) =
        CheckRunner::new(schema, model, request.structural_limits)
            .run_rooted(&targets, collect_dependencies);
    let mut diagnostics = default_diagnostics
        .into_iter()
        .map(|(root, diagnostic)| RootedCheckDiagnostic { root, diagnostic })
        .collect::<Vec<_>>();

    for round in &request.rounds {
        let context = DimensionCheckContext {
            dimension: round.dimension.clone(),
            variant: round.variant.clone(),
        };
        let (round_diagnostics, round_dependencies) =
            CheckRunner::with_dimension_context(schema, model, context, request.structural_limits)
                .run_rooted(&targets, collect_dependencies);
        dependencies.merge(round_dependencies);
        diagnostics.extend(round_diagnostics.into_iter().map(|(root, mut diagnostic)| {
            dimensions::attach_dimension_origins(model, round, &mut diagnostic);
            diagnostic.message = format!(
                "[{}={}] {}",
                round.dimension, round.variant, diagnostic.message
            );
            RootedCheckDiagnostic { root, diagnostic }
        }));
    }

    CheckOutput {
        diagnostics,
        dependencies,
        statistics: CheckExecutionStats {
            requested_roots: targets.len(),
            executed_rounds: targets.len().saturating_mul(request.rounds.len() + 1),
            dependency_collection: request.dependency_collection,
        },
    }
}
