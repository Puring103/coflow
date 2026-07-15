use crate::error::CftDiagnostics;
use crate::module_set::{CftDimensions, CftModuleSet};
use crate::schema::{compile_module_set, CftCompileOptions};
use crate::CftSchema;

/// Builds an immutable semantic schema from modules that have already been parsed.
///
/// The complete effective schema, including dimension storage declarations and
/// derived indexes, is published only after every compilation step succeeds.
///
/// # Errors
///
/// Returns parse diagnostics retained by the module set or schema/type
/// diagnostics from the semantic compilation pass.
pub fn build_schema(
    module_set: &CftModuleSet,
    dimensions: &CftDimensions,
) -> Result<CftSchema, CftDiagnostics> {
    if !module_set.diagnostics().is_empty() {
        return Err(module_set.diagnostics().clone());
    }
    let options = CftCompileOptions::default();
    let (reflection, mut budget) = compile_module_set(module_set, options)?;
    let sources = module_set
        .modules
        .iter()
        .map(|(id, module)| (id.clone(), module.source().to_string()))
        .collect();
    let schema = CftSchema::from_reflection(
        reflection,
        sources,
        options.structural_limits,
        &mut budget,
    )?;
    crate::dimensions::add_dimension_storage(schema, dimensions)
}
