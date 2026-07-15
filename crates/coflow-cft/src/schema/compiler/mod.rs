mod annotations;
mod budget;
mod checked_type;
mod checks;
mod defaults;
mod entry;
mod enums;
mod inheritance;
mod lower;
mod state;
mod symbols;
mod types;

pub use entry::build_schema;

use self::checks::CheckTypeAnalyzer;
use self::state::{ConstInfo, EnumInfo, FieldInfo, Symbol, TypeInfo};
use crate::module::{CftModuleSet, ModuleId};
use crate::schema::{CftConst, CftEnum, CftType, ConstName, EnumName, TypeName};
use crate::syntax::ast::{ConstLiteral, TypeRefKind};
use crate::syntax::Span;
use crate::{CftDiagnostic, CftDiagnostics, CftErrorCode};
use coflow_structure::{StructuralBudget, StructuralLimits};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default)]
pub(in crate::schema) struct SchemaDeclarations {
    pub(super) consts: BTreeMap<ConstName, CftConst>,
    pub(super) types: BTreeMap<TypeName, CftType>,
    pub(super) enums: BTreeMap<EnumName, CftEnum>,
}

pub(super) struct SchemaCompiler<'a> {
    modules: &'a CftModuleSet,
    diagnostics: Vec<CftDiagnostic>,
    symbols: BTreeMap<String, Symbol>,
    consts: BTreeMap<String, ConstInfo<'a>>,
    types: BTreeMap<String, TypeInfo<'a>>,
    enums: BTreeMap<String, EnumInfo<'a>>,
    full_fields: BTreeMap<String, BTreeMap<String, FieldInfo>>,
    inheritance_chains: BTreeMap<String, Vec<String>>,
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
            enums: BTreeMap::new(),
            full_fields: BTreeMap::new(),
            inheritance_chains: BTreeMap::new(),
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
        self.validate_const_type_annotations();
        self.validate_type_headers();
        self.validate_field_shapes();
        if !self.validate_inheritance() {
            return Err(CftDiagnostics::new(std::mem::take(&mut self.diagnostics)));
        }
        self.validate_annotations();
        self.validate_defaults();
        self.build_full_fields();
        self.validate_checks();

        if !self.diagnostics.is_empty() {
            return Err(CftDiagnostics::new(std::mem::take(&mut self.diagnostics)));
        }
        Ok(self.lower_declarations())
    }

    fn validate_const_type_annotations(&mut self) {
        self.each_const(|this, info| {
            let Some(ty) = &info.def.ty else {
                return;
            };
            let type_name = match &ty.kind {
                TypeRefKind::Int => "int",
                TypeRefKind::Float => "float",
                TypeRefKind::Bool => "bool",
                TypeRefKind::String => "string",
                _ => {
                    this.push_diag(
                        CftErrorCode::InvalidConstValue,
                        &info.module,
                        ty.span,
                        "const type annotation must be int, float, bool, or string",
                    );
                    return;
                }
            };
            let matches = matches!(
                (&ty.kind, &info.def.value),
                (TypeRefKind::Int, ConstLiteral::Int(..))
                    | (TypeRefKind::Float, ConstLiteral::Float(..))
                    | (TypeRefKind::Bool, ConstLiteral::Bool(..))
                    | (TypeRefKind::String, ConstLiteral::String(..))
            );
            if !matches {
                let value_span = info.def.value.span();
                this.push_diag(
                    CftErrorCode::InvalidConstValue,
                    &info.module,
                    value_span,
                    format!("const value does not match type annotation `{type_name}`"),
                );
            }
        });
    }

    fn validate_checks(&mut self) {
        self.each_type(|this, info| {
            if let Some(check) = &info.def.check {
                let mut checker = CheckTypeAnalyzer::new(this, info);
                checker.check_stmts(&check.stmts);
            }
        });
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

    fn each_const<F: FnMut(&mut Self, &ConstInfo<'a>)>(&mut self, mut body: F) {
        let keys: Vec<String> = self.consts.keys().cloned().collect();
        for key in keys {
            if let Some(info) = self.consts.get(&key).cloned() {
                body(self, &info);
            }
        }
    }
}
