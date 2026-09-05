mod annotations;
mod budget;
mod checks;
mod constants;
mod defaults;
mod entry;
mod enums;
mod inferred_type;
mod inheritance;
mod lower;
mod state;
mod symbols;
mod types;

pub use entry::{build_schema, build_schema_with_limits};

use self::checks::CheckTypeAnalyzer;
use self::state::{
    CheckInfo, ConstInfo, EnumInfo, FieldInfo, Symbol, TypeAliasInfo, TypeInfo,
};
use crate::module::{CftModuleSet, ModuleId};
use crate::schema::{
    CftConst, CftEnum, CftTopLevelCheck, CftType, CheckName, ConstName, EnumName, TypeName,
};
use crate::source::Span;
use crate::{CftDiagnostic, CftErrorCode};
use std::collections::BTreeMap;
use std::ops::Deref;

#[derive(Debug, Clone, Default)]
pub(in crate::schema) struct SchemaDeclarations {
    pub(super) consts: BTreeMap<ConstName, CftConst>,
    pub(super) types: BTreeMap<TypeName, CftType>,
    pub(super) enums: BTreeMap<EnumName, CftEnum>,
    pub(super) checks: BTreeMap<CheckName, CftTopLevelCheck>,
    pub(super) sources: BTreeMap<ModuleId, crate::schema::CftSchemaSource>,
}

pub(super) struct SymbolTable<'a> {
    modules: &'a CftModuleSet,
    diagnostics: Vec<CftDiagnostic>,
    symbols: BTreeMap<String, Symbol>,
    consts: BTreeMap<String, ConstInfo<'a>>,
    types: BTreeMap<String, TypeInfo<'a>>,
    aliases: BTreeMap<String, TypeAliasInfo<'a>>,
    enums: BTreeMap<String, EnumInfo<'a>>,
    checks: BTreeMap<String, CheckInfo<'a>>,
}

pub(super) struct ResolvedTypes<'a> {
    symbol_table: SymbolTable<'a>,
    diagnostics: Vec<CftDiagnostic>,
    full_fields: BTreeMap<String, BTreeMap<String, FieldInfo>>,
    inheritance_chains: BTreeMap<String, Vec<String>>,
    resolved_aliases: BTreeMap<String, inferred_type::InferredType>,
}

pub(super) struct ResolvedValues<'a> {
    resolved_types: ResolvedTypes<'a>,
    resolved_constants:
        BTreeMap<String, (crate::schema::CftValueType, crate::schema::CftConstValue)>,
    resolved_defaults: BTreeMap<(ModuleId, usize, usize), crate::schema::CftConstValue>,
}

struct ValueResolver<'s, 'a> {
    resolved_types: &'s ResolvedTypes<'a>,
    resolved_constants:
        BTreeMap<String, (crate::schema::CftValueType, crate::schema::CftConstValue)>,
    resolved_defaults: BTreeMap<(ModuleId, usize, usize), crate::schema::CftConstValue>,
    diagnostics: Vec<CftDiagnostic>,
}

pub(super) struct ValidatedSchema<'a> {
    resolved_values: ResolvedValues<'a>,
    quantifier_bindings:
        BTreeMap<(ModuleId, usize, usize), crate::schema::CftSchemaQuantifierBindings>,
    check_dimensions:
        BTreeMap<(ModuleId, usize, usize), BTreeMap<crate::DimensionName, Vec<usize>>>,
    check_statement_dependencies:
        BTreeMap<(ModuleId, usize, usize), Vec<crate::schema::CheckStatementDependencies>>,
}

impl<'a> SymbolTable<'a> {
    fn new(modules: &'a CftModuleSet) -> Self {
        Self {
            modules,
            diagnostics: Vec::new(),
            symbols: BTreeMap::new(),
            consts: BTreeMap::new(),
            types: BTreeMap::new(),
            aliases: BTreeMap::new(),
            enums: BTreeMap::new(),
            checks: BTreeMap::new(),
        }
    }

    pub(super) fn push_diag(
        &mut self,
        code: CftErrorCode,
        module: &ModuleId,
        span: Span,
        message: impl Into<String>,
    ) {
        self.diagnostics
            .push(CftDiagnostic::error(code, module.clone(), span, message));
    }
}

impl<'a> ResolvedTypes<'a> {
    fn new(symbol_table: SymbolTable<'a>) -> Self {
        Self {
            symbol_table,
            diagnostics: Vec::new(),
            full_fields: BTreeMap::new(),
            inheritance_chains: BTreeMap::new(),
            resolved_aliases: BTreeMap::new(),
        }
    }

    fn push_diag(
        &mut self,
        code: CftErrorCode,
        module: &ModuleId,
        span: Span,
        message: impl Into<String>,
    ) {
        self.diagnostics
            .push(CftDiagnostic::error(code, module.clone(), span, message));
    }

    fn push_budget_error(
        &mut self,
        error: crate::limits::BudgetExceeded,
        module: &ModuleId,
        span: Span,
    ) {
        self.push_diag(
            CftErrorCode::SchemaStructureLimitExceeded,
            module,
            span,
            error.to_string(),
        );
    }
}

impl<'a> Deref for ResolvedTypes<'a> {
    type Target = SymbolTable<'a>;

    fn deref(&self) -> &Self::Target {
        &self.symbol_table
    }
}

impl<'a> ResolvedValues<'a> {
    fn resolve(types: ResolvedTypes<'a>) -> (Self, Vec<CftDiagnostic>) {
        let (resolved_constants, resolved_defaults, diagnostics) = {
            let mut resolver = ValueResolver {
                resolved_types: &types,
                resolved_constants: BTreeMap::new(),
                resolved_defaults: BTreeMap::new(),
                diagnostics: Vec::new(),
            };
            resolver.resolve_constants();
            resolver.validate_defaults();
            (
                resolver.resolved_constants,
                resolver.resolved_defaults,
                resolver.diagnostics,
            )
        };
        (
            Self {
                resolved_types: types,
                resolved_constants,
                resolved_defaults,
            },
            diagnostics,
        )
    }
}

impl<'a> Deref for ResolvedValues<'a> {
    type Target = ResolvedTypes<'a>;

    fn deref(&self) -> &Self::Target {
        &self.resolved_types
    }
}

impl<'s, 'a> Deref for ValueResolver<'s, 'a> {
    type Target = ResolvedTypes<'a>;

    fn deref(&self) -> &Self::Target {
        self.resolved_types
    }
}

impl ValueResolver<'_, '_> {
    fn push_diag(
        &mut self,
        code: CftErrorCode,
        module: &ModuleId,
        span: Span,
        message: impl Into<String>,
    ) {
        self.diagnostics
            .push(CftDiagnostic::error(code, module.clone(), span, message));
    }
}

impl<'a> ValidatedSchema<'a> {
    fn validate(values: ResolvedValues<'a>) -> (Self, Vec<CftDiagnostic>) {
        let mut quantifier_bindings = BTreeMap::new();
        let mut check_dimensions = BTreeMap::new();
        let mut check_statement_dependencies = BTreeMap::new();
        let mut diagnostics = Vec::new();
        for info in values.types.values() {
            if let Some(check) = &info.def.check {
                let analysis = CheckTypeAnalyzer::new(&values, info).check_root_stmts(&check.stmts);
                check_dimensions.insert(
                    (info.module.clone(), check.span.start, check.span.end),
                    analysis.dimensions,
                );
                check_statement_dependencies.insert(
                    (info.module.clone(), check.span.start, check.span.end),
                    analysis.dependencies,
                );
                quantifier_bindings.extend(analysis.quantifier_bindings);
                diagnostics.extend(analysis.diagnostics);
            }
        }
        for info in values.checks.values() {
            let analysis = CheckTypeAnalyzer::top_level(&values, info.module.clone())
                .check_root_stmts(&info.def.block.stmts);
            check_dimensions.insert(
                (
                    info.module.clone(),
                    info.def.block.span.start,
                    info.def.block.span.end,
                ),
                analysis.dimensions,
            );
            check_statement_dependencies.insert(
                (
                    info.module.clone(),
                    info.def.block.span.start,
                    info.def.block.span.end,
                ),
                analysis.dependencies,
            );
            quantifier_bindings.extend(analysis.quantifier_bindings);
            diagnostics.extend(analysis.diagnostics);
        }
        (
            Self {
                resolved_values: values,
                quantifier_bindings,
                check_dimensions,
                check_statement_dependencies,
            },
            diagnostics,
        )
    }
}

impl<'a> Deref for ValidatedSchema<'a> {
    type Target = ResolvedValues<'a>;

    fn deref(&self) -> &Self::Target {
        &self.resolved_values
    }
}
