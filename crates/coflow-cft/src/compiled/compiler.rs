mod annotations;
mod budget;
mod build;
mod defaults;
mod symbols;
mod types;

use super::support::{ConstInfo, EnumInfo, FieldInfo, Symbol, TypeInfo};
use super::type_checker::TypeChecker;
use super::{CftCompileOptions, CompiledSchema};
use crate::ast::{ConstLiteral, TypeRefKind};
use crate::error::{CftDiagnostic, CftDiagnostics, CftErrorCode};
use crate::module_id::ModuleId;
use crate::module_set::CftModuleSet;
use crate::span::Span;
use coflow_structure::StructuralBudget;
use std::collections::BTreeMap;

pub(super) struct SchemaCompiler<'a> {
    pub(super) modules: &'a CftModuleSet,
    pub(super) diagnostics: Vec<CftDiagnostic>,
    pub(super) symbols: BTreeMap<String, Symbol>,
    pub(super) consts: BTreeMap<String, ConstInfo<'a>>,
    pub(super) types: BTreeMap<String, TypeInfo<'a>>,
    pub(super) enums: BTreeMap<String, EnumInfo<'a>>,
    pub(super) full_fields: BTreeMap<String, BTreeMap<String, FieldInfo>>,
    pub(super) inheritance_chains: BTreeMap<String, Vec<String>>,
    pub(super) budget: StructuralBudget,
}

impl<'a> SchemaCompiler<'a> {
    pub(super) fn new(modules: &'a CftModuleSet, options: CftCompileOptions) -> Self {
        Self {
            modules,
            diagnostics: Vec::new(),
            symbols: BTreeMap::new(),
            consts: BTreeMap::new(),
            types: BTreeMap::new(),
            enums: BTreeMap::new(),
            full_fields: BTreeMap::new(),
            inheritance_chains: BTreeMap::new(),
            budget: StructuralBudget::new(options.structural_limits),
        }
    }

    pub(super) fn compile(&mut self) -> Result<CompiledSchema, CftDiagnostics> {
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
        Ok(self.build_schema())
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
                let mut checker = TypeChecker::new(this, info);
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
    pub(super) fn each_type<F: FnMut(&mut Self, &TypeInfo<'a>)>(&mut self, mut body: F) {
        let keys: Vec<String> = self.types.keys().cloned().collect();
        for key in keys {
            if let Some(info) = self.types.get(&key).cloned() {
                body(self, &info);
            }
        }
    }

    pub(super) fn each_enum<F: FnMut(&mut Self, &EnumInfo<'a>)>(&mut self, mut body: F) {
        let keys: Vec<String> = self.enums.keys().cloned().collect();
        for key in keys {
            if let Some(info) = self.enums.get(&key).cloned() {
                body(self, &info);
            }
        }
    }

    pub(super) fn each_const<F: FnMut(&mut Self, &ConstInfo<'a>)>(&mut self, mut body: F) {
        let keys: Vec<String> = self.consts.keys().cloned().collect();
        for key in keys {
            if let Some(info) = self.consts.get(&key).cloned() {
                body(self, &info);
            }
        }
    }
}
