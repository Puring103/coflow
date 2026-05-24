use crate::ast::{DataDef, Expr, TypeDef, TypeRef};
use crate::container::{CfcContainer, CfcModuleResult, CfcResult, ModuleId};
use crate::error::{BuildError, BuildErrors};
use crate::value::CfcValueRef;
use std::collections::{BTreeMap, HashMap, HashSet};

mod eval;
mod object;
mod path;
mod support;
mod symbols;

pub(crate) fn build_modules(
    container: &CfcContainer,
    root: Option<ModuleId>,
    module_ids: Vec<ModuleId>,
) -> Result<CfcResult, BuildErrors> {
    let mut ctx = BuildCtx::new(container, module_ids);
    ctx.collect_symbols();
    if !ctx.errors.is_empty() {
        return ctx.finish(root);
    }
    ctx.build_values();
    ctx.finish(root)
}
#[derive(Clone)]
struct TypeInfo {
    module: ModuleId,
    def: TypeDef,
}

#[derive(Clone)]
struct EnumInfo {
    values: HashMap<String, i64>,
}

#[derive(Clone)]
struct ObjectFieldPlan {
    ty: TypeRef,
    expr: Expr,
}

struct ObjectEvalState {
    plans: HashMap<String, ObjectFieldPlan>,
    values: BTreeMap<String, CfcValueRef>,
    visiting: HashSet<String>,
    parent_locals: BTreeMap<String, CfcValueRef>,
}

struct BuildCtx<'a> {
    container: &'a CfcContainer,
    module_ids: Vec<ModuleId>,
    types: HashMap<(ModuleId, String), TypeInfo>,
    enums: HashMap<(ModuleId, String), EnumInfo>,
    data: HashMap<(ModuleId, String), DataDef>,
    memo: HashMap<(ModuleId, String), CfcValueRef>,
    failed: HashSet<(ModuleId, String)>,
    visiting: HashSet<(ModuleId, String)>,
    results: BTreeMap<ModuleId, CfcModuleResult>,
    errors: Vec<BuildError>,
}

impl<'a> BuildCtx<'a> {
    fn new(container: &'a CfcContainer, module_ids: Vec<ModuleId>) -> Self {
        Self {
            container,
            module_ids,
            types: HashMap::new(),
            enums: HashMap::new(),
            data: HashMap::new(),
            memo: HashMap::new(),
            failed: HashSet::new(),
            visiting: HashSet::new(),
            results: BTreeMap::new(),
            errors: Vec::new(),
        }
    }

    fn finish(self, root: Option<ModuleId>) -> Result<CfcResult, BuildErrors> {
        if self.errors.is_empty() {
            Ok(CfcResult::new(root, self.results))
        } else {
            Err(BuildErrors::new(self.errors))
        }
    }
}
