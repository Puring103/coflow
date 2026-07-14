use crate::ast::ModuleAst;
use crate::container::ModuleId;
use crate::error::{CftDiagnostic, CftDiagnostics, CftErrorCode};
use crate::parser::parse_module;
use crate::span::Span;
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
}

/// A parsed CFT module retained for diagnostics and language tooling.
#[derive(Debug, Clone)]
pub struct ParsedCftModule {
    pub(crate) path: PathBuf,
    pub(crate) source: String,
    pub(crate) ast: ModuleAst,
}

/// The collected text of a CFT module, retained even when parsing fails.
#[derive(Debug, Clone)]
pub struct CftModuleFile {
    path: PathBuf,
    source: String,
}

impl CftModuleFile {
    pub(crate) fn new(path: PathBuf, source: String) -> Self {
        Self { path, source }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }
}

impl ParsedCftModule {
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    #[must_use]
    pub const fn ast(&self) -> &ModuleAst {
        &self.ast
    }
}

/// Immutable parse result shared by schema construction and language tooling.
#[derive(Debug, Clone)]
pub struct CftModuleSet {
    pub(crate) files: BTreeMap<ModuleId, CftModuleFile>,
    pub(crate) modules: BTreeMap<ModuleId, ParsedCftModule>,
    pub(crate) diagnostics: CftDiagnostics,
}

/// Dimension variants that affect the effective CFT schema.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CftDimensions {
    pub(crate) variants: BTreeMap<String, Vec<String>>,
}

impl CftDimensions {
    #[must_use]
    pub fn new(entries: impl IntoIterator<Item = (impl Into<String>, Vec<String>)>) -> Self {
        Self {
            variants: entries
                .into_iter()
                .map(|(dimension, variants)| (dimension.into(), variants))
                .collect(),
        }
    }

    #[must_use]
    pub fn variants(&self, dimension: &str) -> Option<&[String]> {
        self.variants.get(dimension).map(Vec::as_slice)
    }
}

impl CftModuleSet {
    #[must_use]
    pub fn diagnostics(&self) -> &CftDiagnostics {
        &self.diagnostics
    }

    #[must_use]
    pub fn module(&self, module: &ModuleId) -> Option<&ParsedCftModule> {
        self.modules.get(module)
    }

    pub fn modules(&self) -> impl Iterator<Item = (&ModuleId, &ParsedCftModule)> {
        self.modules.iter()
    }

    #[must_use]
    pub fn file(&self, module: &ModuleId) -> Option<&CftModuleFile> {
        self.files.get(module)
    }

    pub fn files(&self) -> impl Iterator<Item = (&ModuleId, &CftModuleFile)> {
        self.files.iter()
    }
}

/// Parses collected CFT files once and retains successful ASTs for later consumers.
#[must_use]
pub fn parse_modules(files: impl IntoIterator<Item = CftFile>) -> CftModuleSet {
    let mut collected = BTreeMap::new();
    let mut modules = BTreeMap::new();
    let mut diagnostics = Vec::new();

    for file in files {
        if collected.contains_key(&file.module) {
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
        collected.insert(
            module.clone(),
            CftModuleFile {
                path: path.clone(),
                source: source.clone(),
            },
        );
        match parsed {
            Ok(ast) => {
                modules.insert(
                    module,
                    ParsedCftModule {
                        path,
                        source,
                        ast,
                    },
                );
            }
            Err(errors) => diagnostics.extend(errors.diagnostics),
        }
    }

    CftModuleSet {
        files: collected,
        modules,
        diagnostics: CftDiagnostics::new(diagnostics),
    }
}
