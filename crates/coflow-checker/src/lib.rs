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
use coflow_cft::CompiledSchema;
use coflow_data_model::{CfdDataModel, CfdDiagnostics, CfdRecordId};
pub use coflow_structure::StructuralLimits;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CheckOptions {
    pub structural_limits: StructuralLimits,
}

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
pub fn run_checks(schema: &CompiledSchema, model: &CfdDataModel) -> Result<(), CfdDiagnostics> {
    run_checks_with_options(schema, model, CheckOptions::default())
}

/// Executes CFT `check` blocks with explicit structural resource limits.
///
/// # Errors
///
/// Returns runtime check diagnostics, including `CFD-CHECK-020` when a limit
/// is exhausted.
pub fn run_checks_with_options(
    schema: &CompiledSchema,
    model: &CfdDataModel,
    options: CheckOptions,
) -> Result<(), CfdDiagnostics> {
    CheckRunner::new(schema, model, options.structural_limits).run()
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DimensionCheckPlan {
    rounds: Vec<DimensionCheckRound>,
}

impl DimensionCheckPlan {
    #[must_use]
    pub fn new(rounds: impl IntoIterator<Item = DimensionCheckRound>) -> Self {
        Self {
            rounds: rounds.into_iter().collect(),
        }
    }

    #[must_use]
    pub fn from_variants(
        dimension: impl Into<String>,
        variants: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        let dimension = dimension.into();
        Self::new(
            variants
                .into_iter()
                .map(|variant| DimensionCheckRound::new(dimension.clone(), variant)),
        )
    }

    #[must_use]
    pub fn rounds(&self) -> &[DimensionCheckRound] {
        &self.rounds
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DimensionCheckRound {
    pub dimension: String,
    pub variant: String,
}

impl DimensionCheckRound {
    #[must_use]
    pub fn new(dimension: impl Into<String>, variant: impl Into<String>) -> Self {
        Self {
            dimension: dimension.into(),
            variant: variant.into(),
        }
    }
}

/// Runs `check` blocks for the default data plus every configured dimension
/// variant.
///
/// # Errors
///
/// Returns the union of every failing round's diagnostics. Variant diagnostics
/// are prefixed with `[dimension=variant]`.
pub fn run_checks_for_dimensions(
    schema: &CompiledSchema,
    model: &CfdDataModel,
    plan: &DimensionCheckPlan,
) -> Result<(), CfdDiagnostics> {
    run_checks_for_dimensions_with_options(schema, model, plan, CheckOptions::default())
}

/// Runs default and dimension rounds with explicit structural resource limits.
///
/// # Errors
///
/// Returns the union of runtime check diagnostics from every round.
pub fn run_checks_for_dimensions_with_options(
    schema: &CompiledSchema,
    model: &CfdDataModel,
    plan: &DimensionCheckPlan,
    options: CheckOptions,
) -> Result<(), CfdDiagnostics> {
    let mut all = Vec::new();
    if let Err(diagnostics) = run_checks_with_options(schema, model, options) {
        all.extend(diagnostics.diagnostics);
    }
    for round in plan.rounds() {
        let context = DimensionCheckContext {
            dimension: round.dimension.clone(),
            variant: Some(round.variant.clone()),
        };
        let runner =
            CheckRunner::with_dimension_context(schema, model, context, options.structural_limits);
        push_dimension_diagnostics(&mut all, &round.dimension, &round.variant, runner.run());
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
    schema: &CompiledSchema,
    model: &CfdDataModel,
    plan: &DimensionCheckPlan,
) -> (Result<(), CfdDiagnostics>, DependencyGraph) {
    run_checks_for_dimensions_with_deps_and_options(schema, model, plan, CheckOptions::default())
}

/// Runs dimension-aware checks with dependencies and explicit structural limits.
///
/// # Errors
///
/// Returns the union of runtime diagnostics and the dependency graph collected
/// before any failing root was stopped.
pub fn run_checks_for_dimensions_with_deps_and_options(
    schema: &CompiledSchema,
    model: &CfdDataModel,
    plan: &DimensionCheckPlan,
    options: CheckOptions,
) -> (Result<(), CfdDiagnostics>, DependencyGraph) {
    let mut all = Vec::new();
    let (default_result, mut graph) = run_checks_with_deps_and_options(schema, model, options);
    if let Err(diagnostics) = default_result {
        all.extend(diagnostics.diagnostics);
    }
    for round in plan.rounds() {
        let context = DimensionCheckContext {
            dimension: round.dimension.clone(),
            variant: Some(round.variant.clone()),
        };
        let runner =
            CheckRunner::with_dimension_context(schema, model, context, options.structural_limits);
        let (result, variant_graph) = runner.run_with_deps();
        merge_dependency_graph(&mut graph, variant_graph);
        push_dimension_diagnostics(&mut all, &round.dimension, &round.variant, result);
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
    schema: &CompiledSchema,
    model: &CfdDataModel,
    targets: &[CfdRecordId],
) -> Result<(), CfdDiagnostics> {
    run_checks_for_with_options(schema, model, targets, CheckOptions::default())
}

/// Runs checks for selected records with explicit structural limits.
///
/// # Errors
///
/// Returns runtime check diagnostics for the selected records.
pub fn run_checks_for_with_options(
    schema: &CompiledSchema,
    model: &CfdDataModel,
    targets: &[CfdRecordId],
    options: CheckOptions,
) -> Result<(), CfdDiagnostics> {
    if targets.is_empty() {
        return Ok(());
    }
    CheckRunner::new(schema, model, options.structural_limits).run_for(targets)
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
    schema: &CompiledSchema,
    model: &CfdDataModel,
) -> (Result<(), CfdDiagnostics>, DependencyGraph) {
    run_checks_with_deps_and_options(schema, model, CheckOptions::default())
}

/// Runs checks with dependency collection and explicit structural limits.
///
/// # Errors
///
/// Returns runtime check diagnostics and the dependency graph collected before
/// any failing root was stopped.
pub fn run_checks_with_deps_and_options(
    schema: &CompiledSchema,
    model: &CfdDataModel,
    options: CheckOptions,
) -> (Result<(), CfdDiagnostics>, DependencyGraph) {
    CheckRunner::new(schema, model, options.structural_limits).run_with_deps()
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
    fn run_checks(&self, schema: &CompiledSchema) -> Result<(), CfdDiagnostics>;
}

impl CfdCheckExt for CfdDataModel {
    fn run_checks(&self, schema: &CompiledSchema) -> Result<(), CfdDiagnostics> {
        run_checks(schema, self)
    }
}
