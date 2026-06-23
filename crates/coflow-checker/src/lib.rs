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
mod schema_view;

use check::CheckRunner;
use coflow_cft::CftContainer;
use coflow_data_model::{CfdDataModel, CfdDiagnostics, CfdRecordId};
use std::collections::{BTreeMap, BTreeSet};

/// Per-language translation overrides for `@localized` fields.
///
/// Keys are formatted as `{Bucket}/{record_key}/{field_path}` (see
/// `docs/spec/13-localization.md` §3) and values are the cell text exactly as
/// stored in the CSV translation table. The checker substitutes string-typed
/// localized fields with the corresponding entry when a translation is
/// present; missing keys / non-string fields fall back to the default value.
#[derive(Debug, Clone, Default)]
pub struct LocalizationOverrides {
    pub language: String,
    pub translations: BTreeMap<String, String>,
}

impl LocalizationOverrides {
    /// Convenience constructor.
    #[must_use]
    pub fn new(language: impl Into<String>, translations: BTreeMap<String, String>) -> Self {
        Self {
            language: language.into(),
            translations,
        }
    }
}

/// Executes CFT `check` blocks against an already-built data model.
///
/// # Errors
///
/// Returns runtime check diagnostics for false conditions or evaluation errors.
pub fn run_checks(schema: &CftContainer, model: &CfdDataModel) -> Result<(), CfdDiagnostics> {
    CheckRunner::new(schema, model).run()
}

/// Runs `check` blocks once per declared language, substituting `@localized`
/// string-typed field values from the supplied `LocalizationOverrides`. The
/// default-language round (`run_checks`) should be executed separately by the
/// caller; this entry point only runs the per-language rounds and aggregates
/// their diagnostics.
///
/// # Errors
///
/// Returns the union of every per-language round's diagnostics. Each
/// diagnostic message is prefixed with `[lang=<code>]` so callers can
/// distinguish rounds without a typed channel.
pub fn run_checks_for_languages(
    schema: &CftContainer,
    model: &CfdDataModel,
    overrides: &[LocalizationOverrides],
) -> Result<(), CfdDiagnostics> {
    if overrides.is_empty() {
        return Ok(());
    }
    let mut all = Vec::new();
    for over in overrides {
        let runner = CheckRunner::with_localization(schema, model, over.clone());
        if let Err(diagnostics) = runner.run() {
            for mut diagnostic in diagnostics.diagnostics {
                diagnostic.message = format!("[lang={}] {}", over.language, diagnostic.message);
                all.push(diagnostic);
            }
        }
    }
    if all.is_empty() {
        Ok(())
    } else {
        Err(CfdDiagnostics::new(all))
    }
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
