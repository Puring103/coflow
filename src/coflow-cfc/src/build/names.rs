use super::BuildCtx;
use crate::container::ModuleId;
use crate::error::{BuildError, BuildErrorKind};
use crate::span::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum QualifiedName<'a> {
    LocalPath {
        root: &'a str,
        field: &'a str,
    },
    ImportedData {
        module: ModuleId,
        name: &'a str,
    },
    ImportedPath {
        module: ModuleId,
        data: &'a str,
        field: &'a str,
    },
}

impl BuildCtx<'_> {
    pub(super) fn resolve_qualified_name<'a>(
        &mut self,
        module: &ModuleId,
        parts: &'a [String],
        span: Span,
    ) -> Option<QualifiedName<'a>> {
        match parts {
            [a, b] => {
                if let Some(dep) = self.try_resolve_import(module, a) {
                    if self.symbols.data.contains_key(&(dep.clone(), b.clone())) {
                        return Some(QualifiedName::ImportedData {
                            module: dep,
                            name: b,
                        });
                    }
                }
                Some(QualifiedName::LocalPath { root: a, field: b })
            }
            [alias, data, field] => {
                let dep = self.resolve_import(module, alias, span)?;
                Some(QualifiedName::ImportedPath {
                    module: dep,
                    data,
                    field,
                })
            }
            _ => None,
        }
    }

    pub(super) fn resolve_import(
        &mut self,
        module: &ModuleId,
        alias: &str,
        span: Span,
    ) -> Option<ModuleId> {
        let Some(module_data) = self.container.modules.get(module) else {
            self.errors.push(crate::build::support::build_error(format!(
                "unknown module `{module}`"
            )));
            return None;
        };
        let Some(import) = module_data
            .imports
            .iter()
            .find(|import| import.alias == alias)
        else {
            self.errors.push(BuildError::new(
                BuildErrorKind::Import,
                format!("unknown import alias `{alias}`"),
                Some(span),
            ));
            return None;
        };
        let Some(dep) = module_data.bindings.get(&import.id) else {
            self.errors.push(BuildError::new(
                BuildErrorKind::Import,
                format!("unbound import `{alias}`"),
                Some(import.span),
            ));
            return None;
        };
        Some(dep.clone())
    }

    pub(super) fn try_resolve_import(&self, module: &ModuleId, alias: &str) -> Option<ModuleId> {
        let module_data = self.container.modules.get(module)?;
        let import = module_data
            .imports
            .iter()
            .find(|import| import.alias == alias)?;
        module_data.bindings.get(&import.id).cloned()
    }
}
