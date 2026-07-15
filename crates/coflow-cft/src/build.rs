use crate::error::CftDiagnostics;
use crate::module_set::{CftDimensionInputs, CftModuleSet};
use crate::compiled::{compile_module_set, CftCompileOptions};
use crate::CftSchema;

/// Builds an immutable semantic schema from modules that have already been parsed.
///
/// The complete effective schema is published only after every compilation
/// step and dimension binding succeeds.
///
/// # Errors
///
/// Returns parse diagnostics retained by the module set or schema/type
/// diagnostics from the semantic compilation pass.
pub fn build_schema(
    module_set: &CftModuleSet,
    dimensions: &CftDimensionInputs,
) -> Result<CftSchema, CftDiagnostics> {
    if !module_set.diagnostics().is_empty() {
        return Err(module_set.diagnostics().clone());
    }
    let options = CftCompileOptions::default();
    let (compiled, mut budget) = compile_module_set(module_set, options)?;
    let sources = module_set
        .modules
        .iter()
        .map(|(id, module)| (id.clone(), module.source().to_string()))
        .collect();
    let schema = CftSchema::from_compiled(
        compiled,
        sources,
        dimensions,
        &mut budget,
    )?;
    Ok(schema)
}
