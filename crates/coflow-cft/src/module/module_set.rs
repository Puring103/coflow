use crate::diagnostics::{CftDiagnostic, CftDiagnostics, CftErrorCode};
use crate::module::ModuleId;
use crate::syntax::ast::ModuleAst;
use crate::syntax::parser::parse_module;
use crate::syntax::Span;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

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
    source: Arc<str>,
    pub(crate) ast: Option<Arc<ModuleAst>>,
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
    pub fn ast(&self) -> Option<&ModuleAst> {
        self.ast.as_deref()
    }

    #[must_use]
    pub fn shared_source(&self) -> Arc<str> {
        Arc::clone(&self.source)
    }

    #[must_use]
    pub fn shared_ast(&self) -> Option<Arc<ModuleAst>> {
        self.ast.as_ref().map(Arc::clone)
    }
}

/// Immutable parse result shared by schema construction and language tooling.
#[derive(Debug, Clone)]
pub struct CftModuleSet {
    pub(crate) modules: BTreeMap<ModuleId, CftModule>,
    pub(crate) diagnostics: CftDiagnostics,
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
        let source: Arc<str> = file.source.into();
        let parsed = parse_module(&module, &source);
        let ast = match parsed {
            Ok(ast) => Some(Arc::new(ast)),
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
