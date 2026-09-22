mod annotations;
mod budget;
mod constants;
mod defaults;
mod entry;
mod enums;
mod inferred_type;
mod inheritance;
mod lower;
mod namespaces;
mod state;
mod symbols;
mod types;

pub use entry::{build_schema, build_schema_with_limits};

use self::state::{CheckInfo, ConstInfo, EnumInfo, FieldInfo, Symbol, TypeAliasInfo, TypeInfo};
use crate::source::Span;
use crate::{CftDiagnostic, CftErrorCode};
use coflow_language::cft::{CftModuleSet, ModuleId};
use std::collections::BTreeMap;
use std::ops::Deref;

use super::SchemaDeclarations;

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
    type_state: ResolvedTypes<'a>,
    constants: BTreeMap<String, (crate::schema::CftValueType, crate::schema::CftStaticValue)>,
    defaults: BTreeMap<(ModuleId, usize, usize), crate::schema::CftStaticValue>,
}

struct ValueResolver<'s, 'a> {
    resolved_types: &'s ResolvedTypes<'a>,
    resolved_constants:
        BTreeMap<String, (crate::schema::CftValueType, crate::schema::CftStaticValue)>,
    resolved_defaults: BTreeMap<(ModuleId, usize, usize), crate::schema::CftStaticValue>,
    diagnostics: Vec<CftDiagnostic>,
}

pub(super) struct ValidatedSchema<'a> {
    resolved_values: ResolvedValues<'a>,
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
                type_state: types,
                constants: resolved_constants,
                defaults: resolved_defaults,
            },
            diagnostics,
        )
    }
}

impl<'a> Deref for ResolvedValues<'a> {
    type Target = ResolvedTypes<'a>;

    fn deref(&self) -> &Self::Target {
        &self.type_state
    }
}

impl<'a> Deref for ValueResolver<'_, 'a> {
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
        let diagnostics = Vec::new();
        (
            Self {
                resolved_values: values,
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
