//! Runtime CFT `check` execution for already-built Coflow data models.

mod check;
mod schema_view;

use check::CheckRunner;
use coflow_cft::CftContainer;
use coflow_data_model::{CfdDataModel, CfdDiagnostics};

/// Executes CFT `check` blocks against an already-built data model.
///
/// # Errors
///
/// Returns runtime check diagnostics for false conditions or evaluation errors.
pub fn run_checks(schema: &CftContainer, model: &CfdDataModel) -> Result<(), CfdDiagnostics> {
    CheckRunner::new(schema, model).run()
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
