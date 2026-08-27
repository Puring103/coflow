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

pub use entry::build_schema;

use self::checks::CheckTypeAnalyzer;
use self::state::{
    CheckInfo, ConstInfo, EnumInfo, FieldInfo, ModuleScope, Symbol, TypeAliasInfo, TypeInfo,
};
use crate::limits::{StructuralBudget, StructuralLimits};
use crate::module::{CftModuleSet, ModuleId};
use crate::schema::{
    CftConst, CftEnum, CftTopLevelCheck, CftType, CheckName, ConstName, EnumName, TypeName,
};
use crate::syntax::Span;
use crate::{CftDiagnostic, CftDiagnostics, CftErrorCode};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default)]
pub(in crate::schema) struct SchemaDeclarations {
    pub(super) consts: BTreeMap<ConstName, CftConst>,
    pub(super) types: BTreeMap<TypeName, CftType>,
    pub(super) enums: BTreeMap<EnumName, CftEnum>,
    pub(super) checks: BTreeMap<CheckName, CftTopLevelCheck>,
    pub(super) sources: BTreeMap<ModuleId, crate::schema::CftSchemaSource>,
}

pub(super) struct SchemaCompiler<'a> {
    modules: &'a CftModuleSet,
    diagnostics: Vec<CftDiagnostic>,
    symbols: BTreeMap<String, Symbol>,
    consts: BTreeMap<String, ConstInfo<'a>>,
    types: BTreeMap<String, TypeInfo<'a>>,
    aliases: BTreeMap<String, TypeAliasInfo<'a>>,
    resolved_aliases: BTreeMap<String, inferred_type::InferredType>,
    enums: BTreeMap<String, EnumInfo<'a>>,
    checks: BTreeMap<String, CheckInfo<'a>>,
    module_scopes: BTreeMap<ModuleId, ModuleScope>,
    full_fields: BTreeMap<String, BTreeMap<String, FieldInfo>>,
    resolved_defaults: BTreeMap<(ModuleId, usize, usize), crate::schema::CftConstValue>,
    inheritance_chains: BTreeMap<String, Vec<String>>,
    quantifier_bindings:
        BTreeMap<(ModuleId, usize, usize), crate::schema::CftSchemaQuantifierBindings>,
    check_dimensions:
        BTreeMap<(ModuleId, usize, usize), BTreeMap<crate::DimensionName, Vec<usize>>>,
    check_statement_dependencies:
        BTreeMap<(ModuleId, usize, usize), Vec<crate::schema::CheckStatementDependencies>>,
    budget: StructuralBudget,
}

impl<'a> SchemaCompiler<'a> {
    pub(super) fn new(modules: &'a CftModuleSet) -> Self {
        Self {
            modules,
            diagnostics: Vec::new(),
            symbols: BTreeMap::new(),
            consts: BTreeMap::new(),
            types: BTreeMap::new(),
            aliases: BTreeMap::new(),
            resolved_aliases: BTreeMap::new(),
            enums: BTreeMap::new(),
            checks: BTreeMap::new(),
            module_scopes: BTreeMap::new(),
            full_fields: BTreeMap::new(),
            resolved_defaults: BTreeMap::new(),
            inheritance_chains: BTreeMap::new(),
            quantifier_bindings: BTreeMap::new(),
            check_dimensions: BTreeMap::new(),
            check_statement_dependencies: BTreeMap::new(),
            budget: StructuralBudget::new(StructuralLimits::default()),
        }
    }

    pub(super) fn compile(&mut self) -> Result<SchemaDeclarations, CftDiagnostics> {
        if !self.validate_structure() {
            return Err(CftDiagnostics::new(std::mem::take(&mut self.diagnostics)));
        }
        self.report_dangling_annotations();
        self.collect_symbols();
        self.validate_enums();
        self.validate_type_headers();
        self.validate_type_aliases();
        self.validate_field_shapes();
        if !self.validate_inheritance() {
            return Err(CftDiagnostics::new(std::mem::take(&mut self.diagnostics)));
        }
        self.validate_annotations();
        self.build_full_fields();
        self.resolve_constants();
        self.validate_defaults();
        self.validate_checks();

        if !self.diagnostics.is_empty() {
            return Err(CftDiagnostics::new(std::mem::take(&mut self.diagnostics)));
        }
        Ok(self.lower_declarations())
    }

    fn validate_checks(&mut self) {
        self.each_type(|this, info| {
            if let Some(check) = &info.def.check {
                let mut checker = CheckTypeAnalyzer::new(this, info);
                let (dimensions, dependencies) = checker.check_root_stmts(&check.stmts);
                this.check_dimensions.insert(
                    (info.module.clone(), check.span.start, check.span.end),
                    dimensions,
                );
                this.check_statement_dependencies.insert(
                    (info.module.clone(), check.span.start, check.span.end),
                    dependencies,
                );
            }
        });
        let checks: Vec<_> = self
            .checks
            .iter()
            .map(|(name, info)| (name.clone(), info.clone()))
            .collect();
        for (_name, info) in checks {
            let mut checker = CheckTypeAnalyzer::top_level(self, info.module.clone());
            let (dimensions, dependencies) = checker.check_root_stmts(&info.def.block.stmts);
            self.check_dimensions.insert(
                (
                    info.module.clone(),
                    info.def.block.span.start,
                    info.def.block.span.end,
                ),
                dimensions,
            );
            self.check_statement_dependencies.insert(
                (
                    info.module.clone(),
                    info.def.block.span.start,
                    info.def.block.span.end,
                ),
                dependencies,
            );
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

    /// Iterates over every type, releasing the borrow on `self.types` for each
    /// call so the body can use `&mut self`. Only the key snapshot is allocated
    /// upfront; each info is cloned one at a time inside the loop.
    fn each_type<F: FnMut(&mut Self, &TypeInfo<'a>)>(&mut self, mut body: F) {
        let keys: Vec<String> = self.types.keys().cloned().collect();
        for key in keys {
            if let Some(info) = self.types.get(&key).cloned() {
                body(self, &info);
            }
        }
    }

    fn each_enum<F: FnMut(&mut Self, &EnumInfo<'a>)>(&mut self, mut body: F) {
        let keys: Vec<String> = self.enums.keys().cloned().collect();
        for key in keys {
            if let Some(info) = self.enums.get(&key).cloned() {
                body(self, &info);
            }
        }
    }

}
