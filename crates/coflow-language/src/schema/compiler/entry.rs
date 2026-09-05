use super::{ResolvedTypes, ResolvedValues, SchemaDeclarations, SymbolTable, ValidatedSchema};
use crate::limits::{StructuralBudget, StructuralLimits};
use crate::module::CftModuleSet;
use crate::schema::{AnalysisBudget, CftDimensionInputs, CftSchema};
use crate::CftDiagnostics;

struct StageOutput<T> {
    product: T,
    diagnostics: Vec<crate::CftDiagnostic>,
}

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
    let symbols = collect_symbols(module_set, &mut structural_budget)?;
    let mut analysis_budget = AnalysisBudget::new(limits);
    let types = resolve_types(symbols, &mut analysis_budget)?;
    let values = resolve_values(types);
    let validated = validate_checks(values)?;
    lower_schema(validated, dimensions, &mut analysis_budget)
}

fn collect_symbols<'a>(
    module_set: &'a CftModuleSet,
    structural_budget: &mut StructuralBudget,
) -> Result<StageOutput<SymbolTable<'a>>, CftDiagnostics> {
    let mut symbols = SymbolTable::new(module_set);
    if !symbols.validate_structure(structural_budget) {
        return Err(CftDiagnostics::new(symbols.diagnostics));
    }
    symbols.report_dangling_annotations();
    symbols.collect_symbols();
    symbols.validate_enums();
    let diagnostics = std::mem::take(&mut symbols.diagnostics);
    Ok(StageOutput {
        product: symbols,
        diagnostics,
    })
}

fn resolve_types<'a>(
    symbols: StageOutput<SymbolTable<'a>>,
    analysis_budget: &mut AnalysisBudget,
) -> Result<StageOutput<ResolvedTypes<'a>>, CftDiagnostics> {
    let StageOutput {
        product: symbol_table,
        mut diagnostics,
    } = symbols;
    let mut types = ResolvedTypes::new(symbol_table);
    types.validate_type_headers();
    types.validate_type_aliases();
    types.validate_field_shapes();
    if !types.validate_inheritance(analysis_budget) {
        diagnostics.extend(std::mem::take(&mut types.diagnostics));
        return Err(CftDiagnostics::new(diagnostics));
    }
    types.validate_annotations();
    types.build_full_fields();
    diagnostics.extend(std::mem::take(&mut types.diagnostics));
    Ok(StageOutput {
        product: types,
        diagnostics,
    })
}

fn resolve_values(types: StageOutput<ResolvedTypes<'_>>) -> StageOutput<ResolvedValues<'_>> {
    let StageOutput {
        product: resolved_types,
        mut diagnostics,
    } = types;
    let (values, stage_diagnostics) = ResolvedValues::resolve(resolved_types);
    diagnostics.extend(stage_diagnostics);
    StageOutput {
        product: values,
        diagnostics,
    }
}

fn validate_checks(
    values: StageOutput<ResolvedValues<'_>>,
) -> Result<StageOutput<ValidatedSchema<'_>>, CftDiagnostics> {
    let StageOutput {
        product: resolved_values,
        mut diagnostics,
    } = values;
    let (validated, stage_diagnostics) = ValidatedSchema::validate(resolved_values);
    diagnostics.extend(stage_diagnostics);
    let output = StageOutput {
        product: validated,
        diagnostics,
    };
    if !output.diagnostics.is_empty() {
        return Err(CftDiagnostics::new(output.diagnostics));
    }
    Ok(output)
}

fn lower_schema(
    validated: StageOutput<ValidatedSchema<'_>>,
    dimensions: &CftDimensionInputs,
    analysis_budget: &mut AnalysisBudget,
) -> Result<CftSchema, CftDiagnostics> {
    debug_assert!(validated.diagnostics.is_empty());
    let declarations: SchemaDeclarations = validated.product.lower_declarations();
    CftSchema::from_declarations(declarations, dimensions, analysis_budget)
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
        let modules = modules("abstract sealed type Child: Missing { type: int; }");
        let diagnostics =
            build_schema(&modules, &CftDimensionInputs::default()).expect_err("schema must fail");
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
        let modules = modules("type A: B { value: int = Missing; } type B: A {}");
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

    #[test]
    fn symbol_stage_returns_complete_declarations() {
        let modules = modules("enum State { Ready = 1 } type Item { state: State; }");
        let mut budget = StructuralBudget::new(StructuralLimits::default());
        let symbols = collect_symbols(&modules, &mut budget).expect("symbol stage");
        assert_eq!(symbols.product.enums.len(), 1);
        assert_eq!(symbols.product.types.len(), 1);
        assert!(symbols.product.diagnostics.is_empty());
    }

    #[test]
    fn stage_outputs_accumulate_diagnostics_without_back_writing() {
        let modules = modules("type Item: Missing {} type Item {}");
        let limits = StructuralLimits::default();
        let mut structural = StructuralBudget::new(limits);
        let symbols = collect_symbols(&modules, &mut structural).expect("symbol stage");
        assert_eq!(symbols.diagnostics.len(), 1);
        assert!(symbols.product.diagnostics.is_empty());

        let mut analysis = AnalysisBudget::new(limits);
        let types = resolve_types(symbols, &mut analysis).expect("type stage");
        assert_eq!(types.diagnostics.len(), 2);
        assert!(types.product.diagnostics.is_empty());
        assert_eq!(
            types
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code)
                .collect::<Vec<_>>(),
            [
                CftErrorCode::DuplicateGlobalName,
                CftErrorCode::UnknownNamedType,
            ]
        );
    }

    #[test]
    fn symbol_stage_enforces_its_own_structural_budget() {
        let modules = modules("type Item {}");
        let mut budget = StructuralBudget::new(StructuralLimits::new(100, 0, 100));
        let Err(diagnostics) = collect_symbols(&modules, &mut budget) else {
            panic!("symbol stage must enforce its node budget");
        };
        assert_eq!(diagnostics.diagnostics.len(), 1);
        assert_eq!(
            diagnostics.diagnostics[0].code,
            CftErrorCode::SchemaStructureLimitExceeded
        );
    }

    #[test]
    fn type_and_value_stages_publish_resolved_products() {
        let modules = modules(
            "const Default = 3; type Parent { base: int; } type Child: Parent { value: int = Default; }",
        );
        let limits = StructuralLimits::default();
        let mut structural = StructuralBudget::new(limits);
        let symbols = collect_symbols(&modules, &mut structural).expect("symbol stage");
        let mut analysis = AnalysisBudget::new(limits);
        let types = resolve_types(symbols, &mut analysis).expect("type stage");
        assert_eq!(types.product.full_fields["Child"].len(), 2);
        let values = resolve_values(types);
        assert_eq!(values.product.resolved_constants.len(), 1);
        assert_eq!(values.product.resolved_defaults.len(), 1);
    }

    #[test]
    fn value_stage_reports_its_errors_through_the_stage_output() {
        let modules = modules("const Broken = Missing;");
        let limits = StructuralLimits::default();
        let mut structural = StructuralBudget::new(limits);
        let symbols = collect_symbols(&modules, &mut structural).expect("symbol stage");
        let mut analysis = AnalysisBudget::new(limits);
        let types = resolve_types(symbols, &mut analysis).expect("type stage");
        let values = resolve_values(types);
        assert_eq!(values.diagnostics.len(), 1);
        assert_eq!(values.diagnostics[0].code, CftErrorCode::UnknownConst);
        assert!(values.product.resolved_constants.is_empty());
    }

    #[test]
    fn check_stage_publishes_analysis_for_every_check_block() {
        let modules = modules("type Item { value: int; check { value > 0; } }");
        let limits = StructuralLimits::default();
        let mut structural = StructuralBudget::new(limits);
        let symbols = collect_symbols(&modules, &mut structural).expect("symbol stage");
        let mut analysis = AnalysisBudget::new(limits);
        let types = resolve_types(symbols, &mut analysis).expect("type stage");
        let validated = validate_checks(resolve_values(types)).expect("check stage");
        assert_eq!(validated.product.check_dimensions.len(), 1);
        assert_eq!(validated.product.check_statement_dependencies.len(), 1);
    }

    #[test]
    fn check_stage_rejects_diagnostics_before_lowering() {
        let modules = modules("check Invalid { 1 + true; }");
        let limits = StructuralLimits::default();
        let mut structural = StructuralBudget::new(limits);
        let symbols = collect_symbols(&modules, &mut structural).expect("symbol stage");
        let mut analysis = AnalysisBudget::new(limits);
        let types = resolve_types(symbols, &mut analysis).expect("type stage");
        let Err(diagnostics) = validate_checks(resolve_values(types)) else {
            panic!("check stage must reject invalid operands");
        };
        assert!(!diagnostics.diagnostics.is_empty());
    }

    #[test]
    fn lower_stage_publishes_only_a_fully_validated_schema() {
        let modules = modules("type Item { value: int; }");
        let limits = StructuralLimits::default();
        let mut structural = StructuralBudget::new(limits);
        let symbols = collect_symbols(&modules, &mut structural).expect("symbol stage");
        let mut analysis = AnalysisBudget::new(limits);
        let types = resolve_types(symbols, &mut analysis).expect("type stage");
        let validated = validate_checks(resolve_values(types)).expect("check stage");
        let schema = lower_schema(validated, &CftDimensionInputs::default(), &mut analysis)
            .expect("lower stage");
        assert!(schema.resolve_type("Item").is_some());
    }
}
