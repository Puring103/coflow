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
    CheckInfo, ConstInfo, EnumInfo, FieldInfo, ModuleScope, Symbol, TypeAliasInfo, TypeInfo,
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
    module_scopes: BTreeMap<ModuleId, ModuleScope>,
}

pub(super) struct ResolvedTypes<'a> {
    previous: SymbolTable<'a>,
    full_fields: BTreeMap<String, BTreeMap<String, FieldInfo>>,
    inheritance_chains: BTreeMap<String, Vec<String>>,
    resolved_aliases: BTreeMap<String, inferred_type::InferredType>,
}

pub(super) struct ResolvedValues<'a> {
    previous: ResolvedTypes<'a>,
    resolved_constants: BTreeMap<String, (crate::schema::CftValueType, crate::schema::CftConstValue)>,
    resolved_defaults: BTreeMap<(ModuleId, usize, usize), crate::schema::CftConstValue>,
}

struct ValueResolver<'s, 'a> {
    previous: &'s ResolvedTypes<'a>,
    resolved_constants: BTreeMap<String, (crate::schema::CftValueType, crate::schema::CftConstValue)>,
    resolved_defaults: BTreeMap<(ModuleId, usize, usize), crate::schema::CftConstValue>,
    diagnostics: Vec<CftDiagnostic>,
}

pub(super) struct ValidatedSchema<'a> {
    previous: ResolvedValues<'a>,
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
            module_scopes: BTreeMap::new(),
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
    fn new(symbols: SymbolTable<'a>) -> Self {
        Self {
            previous: symbols,
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
        self.previous.diagnostics.push(CftDiagnostic::error(
            code,
            module.clone(),
            span,
            message,
        ));
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
        &self.previous
    }
}

impl<'a> ResolvedValues<'a> {
    fn resolve(mut types: ResolvedTypes<'a>) -> Self {
        let (resolved_constants, resolved_defaults, diagnostics) = {
            let mut resolver = ValueResolver {
                previous: &types,
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
        types.previous.diagnostics.extend(diagnostics);
        Self {
            previous: types,
            resolved_constants,
            resolved_defaults,
        }
    }
}

impl<'a> Deref for ResolvedValues<'a> {
    type Target = ResolvedTypes<'a>;

    fn deref(&self) -> &Self::Target {
        &self.previous
    }
}

impl<'s, 'a> Deref for ValueResolver<'s, 'a> {
    type Target = ResolvedTypes<'a>;

    fn deref(&self) -> &Self::Target {
        self.previous
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
        self.diagnostics.push(CftDiagnostic::error(
            code,
            module.clone(),
            span,
            message,
        ));
    }
}

impl<'a> ValidatedSchema<'a> {
    fn new(values: ResolvedValues<'a>) -> Self {
        Self {
            previous: values,
            quantifier_bindings: BTreeMap::new(),
            check_dimensions: BTreeMap::new(),
            check_statement_dependencies: BTreeMap::new(),
        }
    }

    fn validate_checks(&mut self) {
        let mut diagnostics = Vec::new();
        for info in self.previous.types.values() {
            if let Some(check) = &info.def.check {
                let analysis = CheckTypeAnalyzer::new(&self.previous, info)
                    .check_root_stmts(&check.stmts);
                self.check_dimensions.insert(
                    (info.module.clone(), check.span.start, check.span.end),
                    analysis.dimensions,
                );
                self.check_statement_dependencies.insert(
                    (info.module.clone(), check.span.start, check.span.end),
                    analysis.dependencies,
                );
                self.quantifier_bindings.extend(analysis.quantifier_bindings);
                diagnostics.extend(analysis.diagnostics);
            }
        }
        for info in self.previous.checks.values() {
            let analysis = CheckTypeAnalyzer::top_level(&self.previous, info.module.clone())
                .check_root_stmts(&info.def.block.stmts);
            self.check_dimensions.insert(
                (
                    info.module.clone(),
                    info.def.block.span.start,
                    info.def.block.span.end,
                ),
                analysis.dimensions,
            );
            self.check_statement_dependencies.insert(
                (
                    info.module.clone(),
                    info.def.block.span.start,
                    info.def.block.span.end,
                ),
                analysis.dependencies,
            );
            self.quantifier_bindings.extend(analysis.quantifier_bindings);
            diagnostics.extend(analysis.diagnostics);
        }
        self.previous.previous.previous.diagnostics.extend(diagnostics);
    }
}

impl<'a> Deref for ValidatedSchema<'a> {
    type Target = ResolvedValues<'a>;

    fn deref(&self) -> &Self::Target {
        &self.previous
    }
}
