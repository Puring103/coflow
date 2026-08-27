use super::annotations::has_annotation;
use super::state::{
    CheckInfo, ConstInfo, EnumInfo, Symbol, SymbolKind, TypeAliasInfo, TypeInfo,
};
use super::SchemaCompiler;
use crate::diagnostics::{CftDiagnostic, CftErrorCode};
use crate::module::ModuleId;
use crate::syntax::ast::Item;
use crate::syntax::Span;
use std::collections::{BTreeMap, BTreeSet};

impl SchemaCompiler<'_> {
    pub(super) fn report_dangling_annotations(&mut self) {
        for (module_id, module) in &self.modules.modules {
            let Some(ast) = module.ast.as_ref() else {
                continue;
            };
            for annotation in &ast.dangling_annotations {
                self.push_diag(
                    CftErrorCode::AnnotationWithoutTarget,
                    module_id,
                    annotation.span,
                    "annotation has no target",
                );
            }
            for item in &ast.items {
                match item {
                    Item::Const(def) => {
                        for annotation in &def.annotations {
                            self.push_diag(
                                CftErrorCode::InvalidAnnotationTarget,
                                module_id,
                                annotation.span,
                                "annotations cannot be applied to const definitions",
                            );
                        }
                    }
                    Item::Enum(def) => {
                        for annotation in &def.dangling_annotations {
                            self.push_diag(
                                CftErrorCode::AnnotationWithoutTarget,
                                module_id,
                                annotation.span,
                                "annotation has no target",
                            );
                        }
                    }
                    Item::Type(def) => {
                        for annotation in &def.dangling_annotations {
                            self.push_diag(
                                CftErrorCode::AnnotationWithoutTarget,
                                module_id,
                                annotation.span,
                                "annotation has no target",
                            );
                        }
                    }
                    Item::TypeAlias(def) => {
                        for annotation in &def.annotations {
                            self.push_diag(
                                CftErrorCode::InvalidAnnotationTarget,
                                module_id,
                                annotation.span,
                                "annotations cannot be applied to type aliases",
                            );
                        }
                    }
                    Item::Check(def) => {
                        for annotation in &def.annotations {
                            self.push_diag(
                                CftErrorCode::InvalidAnnotationTarget,
                                module_id,
                                annotation.span,
                                "annotations cannot be applied to top-level checks",
                            );
                        }
                    }
                }
            }
        }
    }

    pub(super) fn collect_symbols(&mut self) {
        self.collect_module_scopes();
        for (module_id, module) in &self.modules.modules {
            let Some(ast) = module.ast.as_ref() else {
                continue;
            };
            for item in &ast.items {
                match item {
                    Item::Const(def) => {
                        self.validate_identifier(&def.name, module_id, def.name_span);
                        let name = self.declaration_name(module_id, &def.name);
                        if self.insert_symbol(
                            &name,
                            SymbolKind::Const,
                            module_id,
                            def.name_span,
                        ) {
                            self.consts.insert(
                                name.clone(),
                                ConstInfo {
                                    module: module_id.clone(),
                                    def,
                                    value_type: None,
                                    value: None,
                                },
                            );
                        }
                    }
                    Item::Enum(def) => {
                        self.validate_identifier(&def.name, module_id, def.name_span);
                        let name = self.declaration_name(module_id, &def.name);
                        if self.insert_symbol(&name, SymbolKind::Enum, module_id, def.name_span)
                        {
                            self.enums.insert(
                                name.clone(),
                                EnumInfo {
                                    module: module_id.clone(),
                                    def,
                                    variants: BTreeSet::new(),
                                    values: BTreeMap::new(),
                                    values_by_name: BTreeMap::new(),
                                    is_flag: has_annotation(&def.annotations, "flag"),
                                },
                            );
                        }
                    }
                    Item::Type(def) => {
                        self.validate_identifier(&def.name, module_id, def.name_span);
                        let name = self.declaration_name(module_id, &def.name);
                        if self.insert_symbol(&name, SymbolKind::Type, module_id, def.name_span)
                        {
                            self.types.insert(
                                name.clone(),
                                TypeInfo {
                                    name,
                                    module: module_id.clone(),
                                    def,
                                },
                            );
                        }
                    }
                    Item::TypeAlias(def) => {
                        self.validate_identifier(&def.name, module_id, def.name_span);
                        let name = self.declaration_name(module_id, &def.name);
                        if self.insert_symbol(
                            &name,
                            SymbolKind::TypeAlias,
                            module_id,
                            def.name_span,
                        ) {
                            self.aliases.insert(
                                name.clone(),
                                TypeAliasInfo {
                                    module: module_id.clone(),
                                    def,
                                },
                            );
                        }
                    }
                    Item::Check(def) => {
                        self.validate_identifier(&def.name, module_id, def.name_span);
                        let name = self.declaration_name(module_id, &def.name);
                        if let Some(first) = self.checks.get(&name) {
                            self.diagnostics.push(
                                CftDiagnostic::error(
                                    CftErrorCode::DuplicateTopLevelCheck,
                                    module_id.clone(),
                                    def.name_span,
                                    format!("duplicate top-level check `{name}`"),
                                )
                                .with_related(
                                    first.module.clone(),
                                    first.def.name_span,
                                    "first definition is here",
                                ),
                            );
                        } else {
                            self.checks.insert(
                                name.clone(),
                                CheckInfo {
                                    module: module_id.clone(),
                                    def,
                                },
                            );
                        }
                    }
                }
            }
        }
        self.validate_uses();
    }

    fn collect_module_scopes(&mut self) {
        let mut scopes = BTreeMap::new();
        for (module_id, module) in &self.modules.modules {
            let Some(ast) = module.ast.as_ref() else {
                continue;
            };
            let namespace = ast
                .namespace
                .as_ref()
                .map(|declaration| declaration.path.canonical());
            let declaration_names = ast
                .items
                .iter()
                .map(|item| match item {
                    Item::Const(def) => (&def.name, def.name_span),
                    Item::Enum(def) => (&def.name, def.name_span),
                    Item::Type(def) => (&def.name, def.name_span),
                    Item::TypeAlias(def) => (&def.name, def.name_span),
                    Item::Check(def) => (&def.name, def.name_span),
                })
                .collect::<BTreeMap<_, _>>();
            let mut uses = BTreeMap::<String, String>::new();
            let mut use_spans = BTreeMap::<String, Span>::new();
            for declaration in &ast.uses {
                let local_name = declaration.local_name().name.clone();
                if let Some(first_span) = use_spans.get(&local_name) {
                    self.diagnostics.push(
                        CftDiagnostic::error(
                            CftErrorCode::DuplicateGlobalName,
                            module_id.clone(),
                            declaration.span,
                            format!("duplicate use name `{local_name}`"),
                        )
                        .with_related(module_id.clone(), *first_span, "first use is here"),
                    );
                    continue;
                }
                if let Some(first_span) = declaration_names.get(&local_name) {
                    self.diagnostics.push(
                        CftDiagnostic::error(
                            CftErrorCode::DuplicateGlobalName,
                            module_id.clone(),
                            declaration.span,
                            format!("use name `{local_name}` conflicts with a local declaration"),
                        )
                        .with_related(
                            module_id.clone(),
                            *first_span,
                            "local declaration is here",
                        ),
                    );
                    continue;
                }
                use_spans.insert(local_name.clone(), declaration.span);
                uses.insert(local_name, declaration.path.canonical());
            }
            scopes.insert(module_id.clone(), super::state::ModuleScope { namespace, uses });
        }
        self.module_scopes = scopes;
    }

    fn validate_uses(&mut self) {
        let uses = self
            .module_scopes
            .iter()
            .flat_map(|(module, scope)| {
                scope
                    .uses
                    .iter()
                    .map(|(local, target)| (module.clone(), local.clone(), target.clone()))
            })
            .collect::<Vec<_>>();
        for (module, local, target) in uses {
            if !self.symbols.contains_key(&target) {
                self.push_diag(
                    CftErrorCode::UnknownNamedType,
                    &module,
                    self.modules
                        .module(&module)
                        .and_then(|module| module.ast())
                        .and_then(|ast| {
                            ast.uses
                                .iter()
                                .find(|declaration| declaration.local_name().name == local)
                        })
                        .map_or(Span::new(0, 0), |declaration| declaration.span),
                    format!("use target `{target}` does not name a declaration"),
                );
            }
        }
    }

    pub(super) fn declaration_name(&self, module: &ModuleId, name: &str) -> String {
        self.module_scopes
            .get(module)
            .and_then(|scope| scope.namespace.as_ref())
            .map_or_else(|| name.to_string(), |namespace| format!("{namespace}::{name}"))
    }

    pub(super) fn resolve_name(&self, module: &ModuleId, name: &str) -> String {
        if name.contains("::") {
            return name.to_string();
        }
        if let Some(target) = self
            .module_scopes
            .get(module)
            .and_then(|scope| scope.uses.get(name))
        {
            return target.clone();
        }
        self.declaration_name(module, name)
    }

    pub(super) fn validate_identifier(&mut self, name: &str, module_id: &ModuleId, span: Span) {
        if crate::is_cft_reserved_identifier(name) {
            self.push_diag(
                CftErrorCode::ReservedIdentifier,
                module_id,
                span,
                format!("`{name}` is a reserved identifier"),
            );
        }
    }

    /// Registers `name` in the global symbol table. Returns `true` on success
    /// and `false` when the name is already taken (a diagnostic is emitted in
    /// that case). Callers should skip inserting into secondary maps on `false`
    /// so that every map consistently holds the first-seen definition.
    fn insert_symbol(
        &mut self,
        name: &str,
        kind: SymbolKind,
        module_id: &ModuleId,
        span: Span,
    ) -> bool {
        if let Some(first) = self.symbols.get(name) {
            let diagnostic = CftDiagnostic::error(
                CftErrorCode::DuplicateGlobalName,
                module_id.clone(),
                span,
                format!("duplicate global name `{name}`"),
            )
            .with_related(first.module.clone(), first.span, "first definition is here");
            self.diagnostics.push(diagnostic);
            false
        } else {
            self.symbols.insert(
                name.to_string(),
                Symbol {
                    kind,
                    module: module_id.clone(),
                    span,
                },
            );
            true
        }
    }
}
