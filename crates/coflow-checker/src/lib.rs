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

mod check;

use check::CheckRunner;
use coflow_cft::CftContainer;
use coflow_data_model::{CfdDataModel, CfdDiagnostics, CfdRecordId};
use coflow_project::DimensionConfig;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DimensionCheckContext {
    pub(crate) dimension: String,
    pub(crate) variant: Option<String>,
}

/// Executes CFT `check` blocks against an already-built data model.
///
/// # Errors
///
/// Returns runtime check diagnostics for false conditions or evaluation errors.
pub fn run_checks(schema: &CftContainer, model: &CfdDataModel) -> Result<(), CfdDiagnostics> {
    CheckRunner::new(schema, model).run()
}

/// Runs `check` blocks for the default data plus every configured dimension
/// variant.
///
/// # Errors
///
/// Returns the union of every failing round's diagnostics. Variant diagnostics
/// are prefixed with `[dimension=variant]`.
pub fn run_checks_for_dimensions(
    schema: &CftContainer,
    model: &CfdDataModel,
    dimensions: &BTreeMap<String, DimensionConfig>,
) -> Result<(), CfdDiagnostics> {
    let mut all = Vec::new();
    if let Err(diagnostics) = run_checks(schema, model) {
        all.extend(diagnostics.diagnostics);
    }
    for (dimension, config) in dimensions {
        for variant in &config.variants {
            let context = DimensionCheckContext {
                dimension: dimension.clone(),
                variant: Some(variant.clone()),
            };
            let runner = CheckRunner::with_dimension_context(schema, model, context);
            push_dimension_diagnostics(&mut all, dimension, variant, runner.run());
        }
    }
    diagnostics_result(all)
}

/// Run dimension-aware checks and capture the read-from graph across the
/// default round plus every configured variant round.
///
/// # Errors
///
/// Returns the union of every failing round's diagnostics. The dependency
/// graph is returned even when diagnostics fail.
pub fn run_checks_for_dimensions_with_deps(
    schema: &CftContainer,
    model: &CfdDataModel,
    dimensions: &BTreeMap<String, DimensionConfig>,
) -> (Result<(), CfdDiagnostics>, DependencyGraph) {
    let mut all = Vec::new();
    let (default_result, mut graph) = run_checks_with_deps(schema, model);
    if let Err(diagnostics) = default_result {
        all.extend(diagnostics.diagnostics);
    }
    for (dimension, config) in dimensions {
        for variant in &config.variants {
            let context = DimensionCheckContext {
                dimension: dimension.clone(),
                variant: Some(variant.clone()),
            };
            let runner = CheckRunner::with_dimension_context(schema, model, context);
            let (result, variant_graph) = runner.run_with_deps();
            merge_dependency_graph(&mut graph, variant_graph);
            push_dimension_diagnostics(&mut all, dimension, variant, result);
        }
    }
    (diagnostics_result(all), graph)
}

/// Run checks for only a specified subset of records. Empty input is treated
/// as "no checks to run" and returns Ok.
///
/// # Errors
///
/// Returns runtime check diagnostics for false conditions or evaluation
/// errors discovered while checking the subset.
pub fn run_checks_for(
    schema: &CftContainer,
    model: &CfdDataModel,
    targets: &[CfdRecordId],
) -> Result<(), CfdDiagnostics> {
    if targets.is_empty() {
        return Ok(());
    }
    CheckRunner::new(schema, model).run_for(targets)
}

/// A directional dependency graph captured during a full check run.
///
/// `reads_from[a]` is the set of records `a` reads while evaluating its own
/// check blocks. The session inverts this graph to compute "given that
/// records X changed, which records' checks need to re-run".
#[derive(Debug, Clone, Default)]
pub struct DependencyGraph {
    pub reads_from: BTreeMap<CfdRecordId, BTreeSet<CfdRecordId>>,
}

impl DependencyGraph {
    /// Compute the set of records whose checks may be invalidated when
    /// `changed` records mutate. The output includes the changed records
    /// themselves plus every record that reads them.
    #[must_use]
    pub fn affected_by(&self, changed: &[CfdRecordId]) -> Vec<CfdRecordId> {
        let mut out: BTreeSet<CfdRecordId> = changed.iter().copied().collect();
        let changed_set: BTreeSet<CfdRecordId> = changed.iter().copied().collect();
        for (reader, reads) in &self.reads_from {
            if reads.iter().any(|id| changed_set.contains(id)) {
                out.insert(*reader);
            }
        }
        out.into_iter().collect()
    }
}

/// Run checks against a model and capture the read-from graph in the same
/// pass.
///
/// # Errors
///
/// Returns runtime check diagnostics. The dependency graph is returned in
/// either case (so callers can still wire incremental edits even when the
/// initial state has check failures).
pub fn run_checks_with_deps(
    schema: &CftContainer,
    model: &CfdDataModel,
) -> (Result<(), CfdDiagnostics>, DependencyGraph) {
    CheckRunner::new(schema, model).run_with_deps()
}

fn push_dimension_diagnostics(
    all: &mut Vec<coflow_data_model::CfdDiagnostic>,
    dimension: &str,
    variant: &str,
    result: Result<(), CfdDiagnostics>,
) {
    if let Err(diagnostics) = result {
        for mut diagnostic in diagnostics.diagnostics {
            diagnostic.message = format!("[{dimension}={variant}] {}", diagnostic.message);
            all.push(diagnostic);
        }
    }
}

fn diagnostics_result(
    diagnostics: Vec<coflow_data_model::CfdDiagnostic>,
) -> Result<(), CfdDiagnostics> {
    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(CfdDiagnostics::new(diagnostics))
    }
}

fn merge_dependency_graph(target: &mut DependencyGraph, source: DependencyGraph) {
    for (reader, reads) in source.reads_from {
        target.reads_from.entry(reader).or_default().extend(reads);
    }
}

pub trait CfdCheckExt {
    /// Executes CFT `check` blocks against this already-built data model.
    ///
    /// # Errors
    ///
    /// Returns runtime check diagnostics for false conditions or evaluation
    /// errors.
    fn run_checks(&self, schema: &CftContainer) -> Result<(), CfdDiagnostics>;
}

impl CfdCheckExt for CfdDataModel {
    fn run_checks(&self, schema: &CftContainer) -> Result<(), CfdDiagnostics> {
        run_checks(schema, self)
    }
}
