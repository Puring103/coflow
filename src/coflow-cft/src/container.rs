use crate::ast::{Item, ModuleAst};
use crate::error::{ParseErrorKind, ParseErrors};
use crate::parser::parse_module;
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleError {
    pub message: String,
}

#[derive(Debug)]
pub(crate) struct TypeModule {
    pub(crate) source: String,
    pub(crate) ast: ModuleAst,
}

#[derive(Debug, Default)]
pub struct CftContainer {
    pub(crate) modules: BTreeMap<ModuleId, TypeModule>,
    pub(crate) type_names: BTreeMap<String, ModuleId>,
    pub(crate) enum_names: BTreeMap<String, ModuleId>,
}

impl CftContainer {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a parsed type module to the container.
    ///
    /// # Errors
    ///
    /// Returns parse errors when `source` is invalid, when `module` already exists, or when a type
    /// or enum name has already been registered.
    pub fn add_module(
        &mut self,
        module: ModuleId,
        source: impl Into<String>,
    ) -> Result<(), ParseErrors> {
        if self.modules.contains_key(&module) {
            return Err(module_error(format!("module `{module}` already exists")));
        }

        let source = source.into();
        let ast = parse_module(&source)?;
        let mut type_names = BTreeMap::new();
        let mut enum_names = BTreeMap::new();
        for item in &ast.items {
            match item {
                Item::Type(def) => {
                    if type_names.insert(def.name.clone(), def.span).is_some() {
                        return Err(module_error(format!("duplicate type `{}`", def.name)));
                    }
                    if self.type_names.contains_key(&def.name) {
                        return Err(module_error(format!("duplicate type `{}`", def.name)));
                    }
                }
                Item::Enum(def) => {
                    if enum_names.insert(def.name.clone(), def.span).is_some() {
                        return Err(module_error(format!("duplicate enum `{}`", def.name)));
                    }
                    if self.enum_names.contains_key(&def.name) {
                        return Err(module_error(format!("duplicate enum `{}`", def.name)));
                    }
                }
            }
        }

        for name in type_names.keys() {
            self.type_names.insert(name.clone(), module.clone());
        }
        for name in enum_names.keys() {
            self.enum_names.insert(name.clone(), module.clone());
        }
        self.modules.insert(module, TypeModule { source, ast });
        Ok(())
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
}

fn module_error(message: impl Into<String>) -> ParseErrors {
    ParseErrors::one_kind(ParseErrorKind::Module, message, Span::new(0, 0))
}
