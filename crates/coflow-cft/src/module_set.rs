use crate::ast::ModuleAst;
use crate::error::{CftDiagnostic, CftDiagnostics, CftErrorCode};
use crate::module_id::ModuleId;
use crate::parser::parse_module;
use crate::span::Span;
use crate::{DimensionName, VariantName};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One collected CFT module before parsing.
#[derive(Debug, Clone)]
pub struct CftFile {
    module: ModuleId,
    path: PathBuf,
    source: String,
}

impl CftFile {
    #[must_use]
    pub fn new(module: ModuleId, path: PathBuf, source: impl Into<String>) -> Self {
        Self {
            module,
            path,
            source: source.into(),
        }
    }

    /// Creates a collected file whose module id is also its logical path.
    #[must_use]
    pub fn from_source(module: ModuleId, source: impl Into<String>) -> Self {
        let path = PathBuf::from(module.as_str());
        Self::new(module, path, source)
    }
}

/// A CFT module retained for compilation, diagnostics, and language tooling.
#[derive(Debug, Clone)]
pub struct CftModule {
    path: PathBuf,
    source: String,
    pub(crate) ast: Option<ModuleAst>,
}

impl CftModule {
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }
    #[must_use]
    pub const fn ast(&self) -> Option<&ModuleAst> {
        self.ast.as_ref()
    }
}

/// Immutable parse result shared by schema construction and language tooling.
#[derive(Debug, Clone)]
pub struct CftModuleSet {
    pub(crate) modules: BTreeMap<ModuleId, CftModule>,
    pub(crate) diagnostics: CftDiagnostics,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CftDimensionInputs {
    pub(crate) dimensions: BTreeMap<DimensionName, CftDimensionInput>,
    pub(crate) diagnostics: CftDiagnostics,
}

impl Default for CftDimensionInputs {
    fn default() -> Self {
        Self {
            dimensions: BTreeMap::new(),
            diagnostics: CftDiagnostics::new(Vec::new()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CftDimensionInput {
    pub variants: Vec<VariantName>,
}

impl CftDimensionInputs {
    #[must_use]
    pub fn new(entries: impl IntoIterator<Item = (impl Into<String>, Vec<String>)>) -> Self {
        let mut dimensions = BTreeMap::new();
        let mut diagnostics = Vec::new();
        for (dimension, variants) in entries {
            let dimension = dimension.into();
            let Ok(name) = DimensionName::new(dimension.clone()) else {
                diagnostics.push(CftDiagnostic::error(
                    CftErrorCode::InvalidAnnotationArgument,
                    ModuleId::from("__project__"),
                    Span::default(),
                    format!("invalid dimension name `{dimension}`"),
                ));
                continue;
            };
            let mut typed_variants = Vec::new();
            for variant in variants {
                match VariantName::new(variant.clone()) {
                    Ok(variant) => typed_variants.push(variant),
                    Err(_) => diagnostics.push(CftDiagnostic::error(
                        CftErrorCode::InvalidAnnotationArgument,
                        ModuleId::from("__project__"),
                        Span::default(),
                        format!("dimension `{name}` has invalid variant `{variant}`"),
                    )),
                }
            }
            dimensions.insert(
                name,
                CftDimensionInput {
                    variants: typed_variants,
                },
            );
        }
        Self {
            dimensions,
            diagnostics: CftDiagnostics::new(diagnostics),
        }
    }

    #[must_use]
    pub fn dimension(&self, name: &str) -> Option<&CftDimensionInput> {
        self.dimensions.get(name)
    }
}

impl CftModuleSet {
    #[must_use]
    pub fn diagnostics(&self) -> &CftDiagnostics {
        &self.diagnostics
    }

    #[must_use]
    pub fn module(&self, module: &ModuleId) -> Option<&CftModule> {
        self.modules.get(module)
    }

    pub fn modules(&self) -> impl Iterator<Item = (&ModuleId, &CftModule)> {
        self.modules.iter()
    }
}

/// Parses collected CFT files once and retains successful ASTs for later consumers.
#[must_use]
pub fn parse_modules(files: impl IntoIterator<Item = CftFile>) -> CftModuleSet {
    let mut modules = BTreeMap::new();
    let mut diagnostics = Vec::new();

    for file in files {
        if modules.contains_key(&file.module) {
            diagnostics.push(CftDiagnostic::error(
                CftErrorCode::DuplicateModule,
                file.module,
                Span::new(0, 0),
                "duplicate module id",
            ));
            continue;
        }

        let module = file.module;
        let path = file.path;
        let source = file.source;
        let parsed = parse_module(&module, &source);
        let ast = match parsed {
            Ok(ast) => Some(ast),
            Err(errors) => {
                diagnostics.extend(errors.diagnostics);
                None
            }
        };
        modules.insert(module, CftModule { path, source, ast });
    }

    CftModuleSet {
        modules,
        diagnostics: CftDiagnostics::new(diagnostics),
    }
}
