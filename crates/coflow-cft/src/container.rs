use crate::error::{CftDiagnostic, CftDiagnostics, CftErrorCode};
use crate::parser::parse_module;
use crate::schema::{
    compile_container, CftSchemaConst, CftSchemaEnum, CftSchemaModule, CftSchemaType,
    CompiledSchema,
};
use crate::span::Span;
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

#[derive(Debug, Default)]
pub struct CftContainer {
    pub(crate) modules: BTreeMap<ModuleId, CftModule>,
    compiled: Option<CompiledSchema>,
}

impl CftContainer {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
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
        let ast = parse_module(&id, &source)?;
        self.modules.insert(id, CftModule { source, ast });
        self.compiled = None;
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
        let compiled = compile_container(self)?;
        self.compiled = Some(compiled);
        Ok(())
    }

    #[must_use]
    pub fn schema(&self, id: &ModuleId) -> Option<&CftSchemaModule> {
        self.compiled.as_ref()?.modules.get(id)
    }

    #[must_use]
    pub fn resolve_type(&self, name: &str) -> Option<&CftSchemaType> {
        self.compiled.as_ref()?.types.get(name)
    }

    #[must_use]
    pub fn resolve_enum(&self, name: &str) -> Option<&CftSchemaEnum> {
        self.compiled.as_ref()?.enums.get(name)
    }

    #[must_use]
    pub fn resolve_const(&self, name: &str) -> Option<&CftSchemaConst> {
        self.compiled.as_ref()?.consts.get(name)
    }

    pub fn module_ids(&self) -> impl Iterator<Item = &ModuleId> {
        self.modules.keys()
    }

    pub fn all_types(&self) -> impl Iterator<Item = &CftSchemaType> {
        self.compiled
            .as_ref()
            .into_iter()
            .flat_map(|compiled| compiled.types.values())
    }

    pub fn all_enums(&self) -> impl Iterator<Item = &CftSchemaEnum> {
        self.compiled
            .as_ref()
            .into_iter()
            .flat_map(|compiled| compiled.enums.values())
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
        self.modules.get(id).map(|module| module.source.as_str())
    }

    /// Returns true when `actual_type` is `expected_type` itself or a
    /// descendant via single inheritance. Both must be names of known types.
    /// Used by data-model construction (record references and polymorphic
    /// field assignment) and by the runtime check evaluator (`is` predicate).
    #[must_use]
    pub fn is_assignable(&self, actual_type: &str, expected_type: &str) -> bool {
        let mut current = Some(actual_type);
        while let Some(name) = current {
            if name == expected_type {
                return true;
            }
            current = self.resolve_type(name).and_then(|ty| ty.parent.as_deref());
        }
        false
    }

    /// Resolves the integer value of a single enum variant. Returns `None`
    /// when the enum or variant is unknown.
    #[must_use]
    pub fn enum_variant_value(&self, enum_name: &str, variant: &str) -> Option<i64> {
        self.resolve_enum(enum_name)?
            .variants
            .iter()
            .find(|v| v.name == variant)
            .map(|v| v.value)
    }
}
