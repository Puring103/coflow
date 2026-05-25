use crate::ast::ModuleAst;
use crate::build;
use crate::check;
use crate::error::{BuildError, BuildErrors, CfcError, CheckError, ParseErrors};
use crate::parser::parse_module;
use crate::span::Span;
use crate::value::CfcValueRef;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModuleId(String);

impl ModuleId {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for ModuleId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for ModuleId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl fmt::Display for ModuleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ImportId(pub u32);

#[derive(Debug, Clone)]
pub struct CfcImport {
    pub id: ImportId,
    pub alias: String,
    pub path: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleError {
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindImportError {
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveError {
    pub message: String,
}

impl ResolveError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

#[derive(Debug)]
pub(crate) struct Module {
    pub(crate) source: String,
    pub(crate) ast: ModuleAst,
    pub(crate) imports: Vec<CfcImport>,
    pub(crate) bindings: HashMap<ImportId, ModuleId>,
}

#[derive(Debug, Default)]
pub struct CfcContainer {
    pub(crate) modules: BTreeMap<ModuleId, Module>,
}

#[derive(Debug, Clone)]
pub struct CfcResult {
    root: Option<ModuleId>,
    modules: BTreeMap<ModuleId, CfcModuleResult>,
}

#[derive(Debug, Clone)]
pub struct CfcModuleResult {
    values: BTreeMap<String, CfcValueRef>,
}

impl CfcResult {
    pub(crate) fn new(
        root: Option<ModuleId>,
        modules: BTreeMap<ModuleId, CfcModuleResult>,
    ) -> Self {
        Self { root, modules }
    }

    #[must_use]
    pub fn root_id(&self) -> Option<&ModuleId> {
        self.root.as_ref()
    }

    #[must_use]
    pub fn root(&self) -> Option<&CfcModuleResult> {
        self.root.as_ref().and_then(|id| self.modules.get(id))
    }

    #[must_use]
    pub fn module(&self, module: &ModuleId) -> Option<&CfcModuleResult> {
        self.modules.get(module)
    }

    pub fn modules(&self) -> impl Iterator<Item = (&ModuleId, &CfcModuleResult)> {
        self.modules.iter()
    }
}

impl CfcModuleResult {
    pub(crate) fn new(values: BTreeMap<String, CfcValueRef>) -> Self {
        Self { values }
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<CfcValueRef> {
        self.values.get(name).cloned()
    }

    pub fn values(&self) -> impl Iterator<Item = (&str, CfcValueRef)> + '_ {
        self.values
            .iter()
            .map(|(name, value)| (name.as_str(), value.clone()))
    }
}

impl CfcContainer {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a parsed module to the container.
    ///
    /// # Errors
    ///
    /// Returns parse errors when `source` is invalid, or when `module` already exists.
    pub fn add_module(
        &mut self,
        module: ModuleId,
        source: impl Into<String>,
    ) -> Result<(), ParseErrors> {
        if self.modules.contains_key(&module) {
            return Err(ParseErrors::one(
                format!("module `{module}` already exists"),
                Span::new(0, 0),
            ));
        }
        let source = source.into();
        let ast = parse_module(&source)?;
        let imports = ast
            .imports
            .iter()
            .map(|decl| CfcImport {
                id: decl.id,
                alias: decl.alias.clone(),
                path: decl.path.clone(),
                span: decl.span,
            })
            .collect();
        self.modules.insert(
            module,
            Module {
                source,
                ast,
                imports,
                bindings: HashMap::new(),
            },
        );
        Ok(())
    }

    /// Replaces an existing module after successfully parsing the new source.
    ///
    /// # Errors
    ///
    /// Returns parse errors when `source` is invalid, or when `module` does not exist.
    pub fn replace_module(
        &mut self,
        module: ModuleId,
        source: impl Into<String>,
    ) -> Result<(), ParseErrors> {
        if !self.modules.contains_key(&module) {
            return Err(ParseErrors::one(
                format!("module `{module}` does not exist"),
                Span::new(0, 0),
            ));
        }
        let source = source.into();
        let ast = parse_module(&source)?;
        let imports = ast
            .imports
            .iter()
            .map(|decl| CfcImport {
                id: decl.id,
                alias: decl.alias.clone(),
                path: decl.path.clone(),
                span: decl.span,
            })
            .collect();
        self.modules.insert(
            module,
            Module {
                source,
                ast,
                imports,
                bindings: HashMap::new(),
            },
        );
        Ok(())
    }

    /// Returns imports declared by a module.
    ///
    /// # Errors
    ///
    /// Returns an error when `module` is unknown.
    pub fn imports(&self, module: &ModuleId) -> Result<&[CfcImport], ModuleError> {
        self.modules
            .get(module)
            .map(|module| module.imports.as_slice())
            .ok_or_else(|| ModuleError {
                message: format!("unknown module `{module}`"),
            })
    }

    /// Returns the original source for a module.
    ///
    /// # Errors
    ///
    /// Returns an error when `module` is unknown.
    pub fn source(&self, module: &ModuleId) -> Result<&str, ModuleError> {
        self.modules
            .get(module)
            .map(|module| module.source.as_str())
            .ok_or_else(|| ModuleError {
                message: format!("unknown module `{module}`"),
            })
    }

    /// Binds one import declaration to a dependency module.
    ///
    /// # Errors
    ///
    /// Returns an error when either module is unknown, the import id is invalid, or the import has
    /// already been bound.
    pub fn bind_import(
        &mut self,
        from: &ModuleId,
        import: ImportId,
        dependency: &ModuleId,
    ) -> Result<(), BindImportError> {
        if !self.modules.contains_key(dependency) {
            return Err(BindImportError {
                message: format!("unknown dependency module `{dependency}`"),
            });
        }
        let module = self.modules.get_mut(from).ok_or_else(|| BindImportError {
            message: format!("unknown module `{from}`"),
        })?;
        if !module.imports.iter().any(|decl| decl.id == import) {
            return Err(BindImportError {
                message: format!("unknown import id {import:?}"),
            });
        }
        if module.bindings.contains_key(&import) {
            return Err(BindImportError {
                message: format!("import {import:?} is already bound"),
            });
        }
        if module.bindings.values().any(|bound| bound == dependency) {
            return Err(BindImportError {
                message: format!("module `{from}` imports `{dependency}` more than once"),
            });
        }
        let import_alias = module
            .imports
            .iter()
            .find(|decl| decl.id == import)
            .map(|decl| decl.alias.as_str())
            .unwrap_or_default();
        if module
            .imports
            .iter()
            .any(|decl| decl.id != import && decl.alias == import_alias)
        {
            return Err(BindImportError {
                message: format!("duplicate import alias `{import_alias}`"),
            });
        }
        module.bindings.insert(import, dependency.clone());
        Ok(())
    }

    /// Builds the root module and its import closure.
    ///
    /// # Errors
    ///
    /// Returns build errors when the root module is unknown, imports are unbound, or configuration
    /// values fail validation.
    pub fn build(&self, root: &ModuleId) -> Result<CfcResult, BuildErrors> {
        if !self.modules.contains_key(root) {
            return Err(BuildErrors::new(vec![plain_build_error(format!(
                "unknown root module `{root}`"
            ))]));
        }
        let mut set = BTreeSet::new();
        let mut errors = Vec::new();
        self.collect_closure(root, &mut set, &mut errors);
        if !errors.is_empty() {
            return Err(BuildErrors::new(errors));
        }
        build::build_modules(self, Some(root.clone()), set.into_iter().collect())
    }

    /// Builds all modules currently stored in the container.
    ///
    /// # Errors
    ///
    /// Returns build errors when any module has invalid or unbound references.
    pub fn build_all(&self) -> Result<CfcResult, BuildErrors> {
        let mut errors = Vec::new();
        for (module_id, module) in &self.modules {
            for import in &module.imports {
                if !module.bindings.contains_key(&import.id) {
                    errors.push(BuildError {
                        message: format!(
                            "unbound import `{}` in module `{module_id}`",
                            import.alias
                        ),
                        span: Some(import.span),
                    });
                }
            }
        }
        if !errors.is_empty() {
            return Err(BuildErrors::new(errors));
        }
        build::build_modules(self, None, self.modules.keys().cloned().collect())
    }

    #[must_use]
    pub fn check(&self, result: &CfcResult) -> Vec<CheckError> {
        check::run(self, result)
    }

    /// Loads a root module, resolves its import graph, and builds the resulting closure.
    ///
    /// # Errors
    ///
    /// Returns parse, module lookup, import binding, resolver, or build errors depending on the
    /// phase that fails.
    #[allow(clippy::needless_pass_by_value)]
    pub fn load_graph<R>(
        &mut self,
        root: ModuleId,
        source: impl Into<String>,
        mut resolver: R,
    ) -> Result<CfcResult, CfcError>
    where
        R: FnMut(&ModuleId, &CfcImport) -> Result<(ModuleId, String), ResolveError>,
    {
        self.add_module(root.clone(), source)
            .map_err(CfcError::Parse)?;
        let mut queue = vec![root.clone()];
        let mut seen = HashSet::new();

        while let Some(module_id) = queue.pop() {
            if !seen.insert(module_id.clone()) {
                continue;
            }
            let imports: Vec<_> = self.imports(&module_id).map_err(CfcError::Module)?.to_vec();
            for import in imports {
                let (dependency, source) =
                    resolver(&module_id, &import).map_err(CfcError::Resolve)?;
                if !self.modules.contains_key(&dependency) {
                    self.add_module(dependency.clone(), source)
                        .map_err(CfcError::Parse)?;
                    queue.push(dependency.clone());
                }
                self.bind_import(&module_id, import.id, &dependency)
                    .map_err(CfcError::Import)?;
            }
        }

        self.build(&root).map_err(CfcError::Build)
    }

    fn collect_closure(
        &self,
        id: &ModuleId,
        set: &mut BTreeSet<ModuleId>,
        errors: &mut Vec<BuildError>,
    ) {
        if !set.insert(id.clone()) {
            return;
        }
        let Some(module) = self.modules.get(id) else {
            errors.push(plain_build_error(format!("unknown module `{id}`")));
            return;
        };
        for import in &module.imports {
            match module.bindings.get(&import.id) {
                Some(dep) => self.collect_closure(dep, set, errors),
                None => errors.push(BuildError {
                    message: format!("unbound import `{}`", import.alias),
                    span: Some(import.span),
                }),
            }
        }
    }
}

fn plain_build_error(message: impl Into<String>) -> BuildError {
    BuildError {
        message: message.into(),
        span: None,
    }
}
