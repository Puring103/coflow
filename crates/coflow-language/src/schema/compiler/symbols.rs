use super::annotations::has_annotation;
use super::lower::const_value;
use super::state::{CheckInfo, ConstInfo, EnumInfo, Symbol, SymbolKind, TypeInfo};
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
        for (module_id, module) in &self.modules.modules {
            let Some(ast) = module.ast.as_ref() else {
                continue;
            };
            for item in &ast.items {
                match item {
                    Item::Const(def) => {
                        self.validate_identifier(&def.name, module_id, def.name_span);
                        if self.insert_symbol(
                            &def.name,
                            SymbolKind::Const,
                            module_id,
                            def.name_span,
                        ) {
                            self.consts.insert(
                                def.name.clone(),
                                ConstInfo {
                                    module: module_id.clone(),
                                    def,
                                    value: const_value(&def.value),
                                },
                            );
                        }
                    }
                    Item::Enum(def) => {
                        self.validate_identifier(&def.name, module_id, def.name_span);
                        if self.insert_symbol(&def.name, SymbolKind::Enum, module_id, def.name_span)
                        {
                            self.enums.insert(
                                def.name.clone(),
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
                        if self.insert_symbol(&def.name, SymbolKind::Type, module_id, def.name_span)
                        {
                            self.types.insert(
                                def.name.clone(),
                                TypeInfo {
                                    module: module_id.clone(),
                                    def,
                                },
                            );
                        }
                    }
                    Item::Check(def) => {
                        self.validate_identifier(&def.name, module_id, def.name_span);
                        if let Some(first) = self.checks.get(&def.name) {
                            self.diagnostics.push(
                                CftDiagnostic::error(
                                    CftErrorCode::DuplicateTopLevelCheck,
                                    module_id.clone(),
                                    def.name_span,
                                    format!("duplicate top-level check `{}`", def.name),
                                )
                                .with_related(
                                    first.module.clone(),
                                    first.def.name_span,
                                    "first definition is here",
                                ),
                            );
                        } else {
                            self.checks.insert(
                                def.name.clone(),
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
