use super::{ResolvedTypes, ResolvedValues, SymbolTable, ValidatedSchema};
use crate::limits::{StructuralBudget, StructuralLimits};
use crate::module::CftModuleSet;
use crate::schema::{AnalysisBudget, CftDimensionInputs, CftSchema};
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
    build_schema_with_limits(module_set, dimensions, StructuralLimits::default())
}

/// Builds an immutable semantic schema with explicit structural and analysis limits.
///
/// # Errors
///
/// Returns retained parse diagnostics or schema compilation diagnostics.
pub fn build_schema_with_limits(
    module_set: &CftModuleSet,
    dimensions: &CftDimensionInputs,
    limits: StructuralLimits,
) -> Result<CftSchema, CftDiagnostics> {
    if !module_set.diagnostics().is_empty() {
        return Err(module_set.diagnostics().clone());
    }
    // 预算属于流水线调用，而不是任一阶段产物，所有静态分析入口都显式接收它。
    let mut structural_budget = StructuralBudget::new(limits);
    let mut symbols = SymbolTable::new(module_set);
    if !symbols.validate_structure(&mut structural_budget) {
        return Err(CftDiagnostics::new(symbols.diagnostics));
    }
    symbols.report_dangling_annotations();
    symbols.collect_symbols();
    symbols.validate_enums();

    let mut types = ResolvedTypes::new(symbols);
    types.validate_type_headers();
    types.validate_type_aliases();
    types.validate_field_shapes();
    let mut analysis_budget = AnalysisBudget::new(limits);
    if !types.validate_inheritance(&mut analysis_budget) {
        return Err(CftDiagnostics::new(std::mem::take(
            &mut types.previous.diagnostics,
        )));
    }
    types.validate_annotations();
    types.build_full_fields();

    let values = ResolvedValues::resolve(types);

    let mut validated = ValidatedSchema::new(values);
    validated.validate_checks();
    if !validated.previous.previous.previous.diagnostics.is_empty() {
        return Err(CftDiagnostics::new(std::mem::take(
            &mut validated.previous.previous.previous.diagnostics,
        )));
    }
    let declarations = validated.lower_declarations();
    let schema = CftSchema::from_declarations(declarations, dimensions, &mut analysis_budget)?;
    Ok(schema)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::CftErrorCode;
    use crate::module::{parse_modules, CftFile, ModuleId};

    fn modules(source: &str) -> CftModuleSet {
        parse_modules([CftFile::from_source(ModuleId::from("main"), source)])
    }

    #[test]
    fn phase_diagnostics_keep_source_independent_pass_order() {
        let modules = modules(
            "abstract sealed type Child: Missing { type: int; }",
        );
        let diagnostics = build_schema(&modules, &CftDimensionInputs::default())
            .expect_err("schema must fail");
        let codes = diagnostics
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code)
            .collect::<Vec<_>>();
        assert_eq!(
            codes,
            [
                CftErrorCode::ConflictingTypeModifiers,
                CftErrorCode::UnknownNamedType,
                CftErrorCode::ReservedIdentifier,
            ]
        );
    }

    #[test]
    fn analysis_budget_is_passed_to_the_graph_phase() {
        let modules = modules("type Parent {} type Child: Parent {}");
        let diagnostics = build_schema_with_limits(
            &modules,
            &CftDimensionInputs::default(),
            StructuralLimits::new(100, 100, 0),
        )
        .expect_err("inheritance edge must exhaust analysis steps");
        assert_eq!(
            diagnostics
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code)
                .collect::<Vec<_>>(),
            [CftErrorCode::SchemaStructureLimitExceeded]
        );
    }

    #[test]
    fn failed_inheritance_does_not_enter_value_resolution() {
        let modules = modules(
            "type A: B { value: int = Missing; } type B: A {}",
        );
        let diagnostics = build_schema(&modules, &CftDimensionInputs::default())
            .expect_err("inheritance cycle must stop the pipeline");
        assert!(diagnostics
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == CftErrorCode::InheritanceCycle));
        assert!(!diagnostics.diagnostics.iter().any(|diagnostic| {
            matches!(
                diagnostic.code,
                CftErrorCode::UnknownConst | CftErrorCode::InvalidDefaultExpression
            )
        }));
    }
}
