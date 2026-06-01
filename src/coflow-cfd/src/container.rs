use crate::ast::ModuleAst;
use crate::build;
use crate::check;
use crate::error::{BuildErrors, CheckError, ParseErrorKind, ParseErrors};
use crate::parser::parse_module;
use crate::span::Span;
use crate::value::CfdValueRef;
use coflow_cft::CftContainer;
use std::collections::BTreeMap;

pub use coflow_cft::ModuleId;

pub(crate) struct DataModule {
    pub(crate) source: String,
    pub(crate) ast: ModuleAst,
}

pub struct CfdContainer {
    pub(crate) type_ctx: CftContainer,
    pub(crate) modules: BTreeMap<ModuleId, DataModule>,
}

#[derive(Debug, Clone)]
pub struct CfdResult {
    modules: BTreeMap<ModuleId, CfdModuleResult>,
}

#[derive(Debug, Clone)]
pub struct CfdModuleResult {
    values: BTreeMap<String, CfdValueRef>,
}

impl CfdResult {
    pub(crate) fn new(modules: BTreeMap<ModuleId, CfdModuleResult>) -> Self {
        Self { modules }
    }

    #[must_use]
    pub fn module(&self, module: &ModuleId) -> Option<&CfdModuleResult> {
        self.modules.get(module)
    }

    pub fn modules(&self) -> impl Iterator<Item = (&ModuleId, &CfdModuleResult)> {
        self.modules.iter()
    }
}

impl CfdModuleResult {
    pub(crate) fn new(values: BTreeMap<String, CfdValueRef>) -> Self {
        Self { values }
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<CfdValueRef> {
        self.values.get(name).cloned()
    }

    pub fn values(&self) -> impl Iterator<Item = (&str, CfdValueRef)> + '_ {
        self.values
            .iter()
            .map(|(name, value)| (name.as_str(), value.clone()))
    }
}

impl CfdContainer {
    #[must_use]
    pub fn new(type_ctx: CftContainer) -> Self {
        Self {
            type_ctx,
            modules: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn type_ctx(&self) -> &CftContainer {
        &self.type_ctx
    }

    pub fn add_module(
        &mut self,
        module: ModuleId,
        source: impl Into<String>,
    ) -> Result<(), ParseErrors> {
        if self.modules.contains_key(&module) {
            return Err(ParseErrors::one_kind(
                ParseErrorKind::Module,
                format!("module `{module}` already exists"),
                Span::new(0, 0),
            ));
        }
        let source = source.into();
        let ast = parse_module(&source)?;
        self.modules.insert(module, DataModule { source, ast });
        Ok(())
    }

    pub fn replace_module(
        &mut self,
        module: ModuleId,
        source: impl Into<String>,
    ) -> Result<(), ParseErrors> {
        if !self.modules.contains_key(&module) {
            return Err(ParseErrors::one_kind(
                ParseErrorKind::Module,
                format!("module `{module}` does not exist"),
                Span::new(0, 0),
            ));
        }
        let source = source.into();
        let ast = parse_module(&source)?;
        self.modules.insert(module, DataModule { source, ast });
        Ok(())
    }

    pub fn source(&self, module: &ModuleId) -> Option<&str> {
        self.modules.get(module).map(|module| module.source.as_str())
    }

    pub fn build_all(&self) -> Result<CfdResult, BuildErrors> {
        build::build_modules(self, self.modules.keys().cloned().collect())
    }

    #[must_use]
    pub fn check(&self, result: &CfdResult) -> Vec<CheckError> {
        check::run(self, result)
    }
}
