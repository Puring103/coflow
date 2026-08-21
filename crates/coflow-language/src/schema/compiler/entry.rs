use super::SchemaCompiler;
use crate::module::CftModuleSet;
use crate::schema::{CftDimensionInputs, CftSchema};
use crate::CftDiagnostics;

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
    let mut compiler = SchemaCompiler::new(module_set);
    let declarations = compiler.compile()?;
    let schema = CftSchema::from_declarations(declarations, dimensions, &mut compiler.budget)?;
    Ok(schema)
}
