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
    path: PathBuf,
    source: String,
    ast: ModuleAst,
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
    modules: BTreeMap<ModuleId, ParsedCftModule>,
    diagnostics: CftDiagnostics,
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

        match parse_module(&file.module, &file.source) {
            Ok(ast) => {
                modules.insert(
                    file.module,
                    ParsedCftModule {
                        path: file.path,
                        source: file.source,
                        ast,
                    },
                );
            }
            Err(errors) => diagnostics.extend(errors.diagnostics),
        }
    }

    CftModuleSet {
        modules,
        diagnostics: CftDiagnostics::new(diagnostics),
    }
}
