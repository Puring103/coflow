use crate::error::{CftDiagnostic, CftDiagnostics, CftErrorCode};
use crate::module_set::CftModuleSet;
use crate::parser::{parse_module_with_options, CftParseOptions};
use crate::schema::{
    compile_module_set, CftCompileOptions, CftSchemaConst, CftSchemaEnum, CftSchemaModule,
    CftSchemaType,
};
use crate::span::Span;
use crate::CftSchema;
use coflow_structure::StructuralBudget;
use std::collections::BTreeMap;
use std::fmt;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

impl fmt::Debug for ModuleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CftModule {
    pub(crate) source: String,
    pub(crate) ast: crate::ast::ModuleAst,
}

#[derive(Debug)]
pub struct CftContainer {
    pub(crate) modules: BTreeMap<ModuleId, CftModule>,
    compiled: CftSchema,
    parse_options: CftParseOptions,
}

impl Default for CftContainer {
    fn default() -> Self {
        Self {
            modules: BTreeMap::new(),
            compiled: CftSchema::empty(),
            parse_options: CftParseOptions::default(),
        }
    }
}

impl CftContainer {
    const RUNTIME_MODULE_ID: &'static str = "__runtime__";

    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_parse_options(parse_options: CftParseOptions) -> Self {
        Self {
            parse_options,
            ..Self::default()
        }
    }

    #[must_use]
    pub const fn parse_options(&self) -> CftParseOptions {
        self.parse_options
    }

    /// Registers one module and parses it into AST.
    ///
    /// # Errors
    ///
    /// Returns diagnostics for duplicate module ids, lexical errors, or syntax errors.
    pub fn add_module(
        &mut self,
        id: ModuleId,
        source: impl Into<String>,
    ) -> Result<(), CftDiagnostics> {
        if self.modules.contains_key(&id) {
            return Err(CftDiagnostics::one(CftDiagnostic::error(
                CftErrorCode::DuplicateModule,
                id,
                Span::new(0, 0),
                "duplicate module id",
            )));
        }
        let source = source.into();
        let ast = parse_module_with_options(&id, &source, self.parse_options)?;
        self.modules.insert(id, CftModule { source, ast });
        Ok(())
    }

    /// Finalizes all registered modules into schema and statically type-checks checks.
    ///
    /// # Errors
    ///
    /// Returns schema and type diagnostics. A failed compile leaves the previously
    /// published schema (if any) untouched, so consumers keep observing a stable
    /// reflection until the next successful call.
    pub fn compile(&mut self) -> Result<(), CftDiagnostics> {
        self.compile_with_options(CftCompileOptions::default())
    }

    /// Compiles all modules with explicit structural resource limits.
    ///
    /// # Errors
    ///
    /// Returns schema/type diagnostics. Failed compilation leaves the last
    /// successfully published reflection untouched.
    pub fn compile_with_options(
        &mut self,
        options: CftCompileOptions,
    ) -> Result<(), CftDiagnostics> {
        let module_set = self.module_set();
        let (reflection, mut budget) = compile_module_set(&module_set, options)?;
        let sources = self
            .modules
            .iter()
            .map(|(id, module)| (id.clone(), module.source.clone()))
            .collect();
        let compiled = CftSchema::from_reflection(
            reflection,
            sources,
            options.structural_limits,
            &mut budget,
        )?;
        self.compiled = compiled;
        Ok(())
    }

    /// Atomically registers runtime-built types in the published schema.
    ///
    /// # Errors
    ///
    /// Returns a duplicate-name diagnostic when a type with the same name is
    /// already present. The published snapshot is replaced only after the
    /// whole batch and all derived indexes have been rebuilt.
    pub fn register_runtime_types(
        &mut self,
        types: impl IntoIterator<Item = CftSchemaType>,
    ) -> Result<(), CftDiagnostics> {
        let mut reflection = self.compiled.reflection().clone();
        let sources = self.compiled.sources().clone();
        let structural_limits = self.compiled.structural_limits();
        let runtime_module = ModuleId::from(Self::RUNTIME_MODULE_ID);
        for mut ty in types {
            if reflection.types.contains_key(&ty.name) {
                return Err(CftDiagnostics::one(CftDiagnostic::error(
                    CftErrorCode::DuplicateGlobalName,
                    ty.module.clone(),
                    ty.span,
                    format!("duplicate global name `{}`", ty.name),
                )));
            }
            ty.module = runtime_module.clone();
            for field in &mut ty.fields {
                field.dimension = None;
            }
            for field in &mut ty.all_fields {
                field.dimension = None;
            }

            let name = ty.name.clone();
            reflection
                .modules
                .entry(runtime_module.clone())
                .or_insert_with(|| CftSchemaModule {
                    consts: Vec::new(),
                    types: Vec::new(),
                    enums: Vec::new(),
                })
                .types
                .push(ty.clone());
            reflection.types.insert(name, ty);
        }
        let mut budget = StructuralBudget::new(structural_limits);
        self.compiled =
            CftSchema::from_reflection(reflection, sources, structural_limits, &mut budget)?;
        Ok(())
    }

    #[must_use]
    pub const fn compiled_schema(&self) -> &CftSchema {
        &self.compiled
    }

    #[must_use]
    pub fn schema(&self, id: &ModuleId) -> Option<&CftSchemaModule> {
        self.compiled.reflection().modules.get(id)
    }

    #[must_use]
    pub fn resolve_type(&self, name: &str) -> Option<&CftSchemaType> {
        self.compiled.reflection().types.get(name)
    }

    #[must_use]
    pub fn resolve_enum(&self, name: &str) -> Option<&CftSchemaEnum> {
        self.compiled.reflection().enums.get(name)
    }

    #[must_use]
    pub fn resolve_const(&self, name: &str) -> Option<&CftSchemaConst> {
        self.compiled.reflection().consts.get(name)
    }

    pub fn module_ids(&self) -> impl Iterator<Item = &ModuleId> {
        self.compiled.reflection().modules.keys()
    }

    pub fn all_types(&self) -> impl Iterator<Item = &CftSchemaType> {
        self.compiled.reflection().types.values()
    }

    pub fn all_enums(&self) -> impl Iterator<Item = &CftSchemaEnum> {
        self.compiled.reflection().enums.values()
    }

    #[must_use]
    pub fn has_type(&self, name: &str) -> bool {
        self.resolve_type(name).is_some()
    }

    #[must_use]
    pub fn has_enum(&self, name: &str) -> bool {
        self.resolve_enum(name).is_some()
    }

    #[must_use]
    pub fn source(&self, id: &ModuleId) -> Option<&str> {
        self.compiled.sources().get(id).map(String::as_str)
    }

    fn module_set(&self) -> CftModuleSet {
        CftModuleSet {
            modules: self
                .modules
                .iter()
                .map(|(id, module)| {
                    (
                        id.clone(),
                        crate::ParsedCftModule {
                            path: std::path::PathBuf::from(id.as_str()),
                            source: module.source.clone(),
                            ast: module.ast.clone(),
                        },
                    )
                })
                .collect(),
            diagnostics: CftDiagnostics::new(Vec::new()),
        }
    }
}

/// Builds a semantic schema from modules that have already been parsed.
///
/// # Errors
///
/// Returns parse diagnostics retained by the module set or schema/type
/// diagnostics from the semantic compilation pass.
pub fn build_schema(module_set: &CftModuleSet) -> Result<CftSchema, CftDiagnostics> {
    if !module_set.diagnostics().is_empty() {
        return Err(module_set.diagnostics().clone());
    }
    let (reflection, mut budget) = compile_module_set(module_set, CftCompileOptions::default())?;
    let sources = module_set
        .modules
        .iter()
        .map(|(id, module)| (id.clone(), module.source.clone()))
        .collect();
    CftSchema::from_reflection(
        reflection,
        sources,
        CftCompileOptions::default().structural_limits,
        &mut budget,
    )
}
