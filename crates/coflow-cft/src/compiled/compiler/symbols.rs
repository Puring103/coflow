use super::SchemaCompiler;
use crate::ast::Item;
use crate::compiled::support::{
    const_value, has_annotation, is_i64_power_of_two, is_reserved_identifier, ConstInfo, EnumInfo,
    Symbol, SymbolKind, TypeInfo,
};
use crate::error::{CftDiagnostic, CftErrorCode};
use crate::module_id::ModuleId;
use crate::span::Span;
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
                }
            }
        }
    }

    pub(super) fn validate_identifier(&mut self, name: &str, module_id: &ModuleId, span: Span) {
        if is_reserved_identifier(name) {
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

    pub(super) fn validate_enums(&mut self) {
        let names = self.enums.keys().cloned().collect::<Vec<_>>();
        for name in names {
            let Some(info) = self.enums.get(&name).cloned() else {
                continue;
            };
            let mut next = 0_i64;
            let mut variant_names: BTreeMap<String, (ModuleId, Span)> = BTreeMap::new();
            let mut values: BTreeMap<i64, (String, ModuleId, Span)> = BTreeMap::new();
            let mut variants = BTreeSet::new();
            let mut values_by_name = BTreeMap::new();
            for (index, variant) in info.def.variants.iter().enumerate() {
                self.validate_identifier(&variant.name, &info.module, variant.name_span);
                if let Some(first) = variant_names.get(&variant.name) {
                    self.diagnostics.push(
                        CftDiagnostic::error(
                            CftErrorCode::DuplicateEnumVariant,
                            info.module.clone(),
                            variant.name_span,
                            format!("duplicate enum variant `{}`", variant.name),
                        )
                        .with_related(
                            first.0.clone(),
                            first.1,
                            "first variant is here",
                        ),
                    );
                } else {
                    variant_names.insert(
                        variant.name.clone(),
                        (info.module.clone(), variant.name_span),
                    );
                }
                let value = variant.value.as_ref().map_or(next, |value| value.value);
                if value == i64::MAX
                    && info
                        .def
                        .variants
                        .iter()
                        .skip(index + 1)
                        .any(|next_variant| next_variant.value.is_none())
                {
                    self.push_diag(
                        CftErrorCode::InvalidEnumValueSequence,
                        &info.module,
                        variant.span,
                        "enum auto numbering overflowed",
                    );
                }
                next = value.saturating_add(1);
                if let Some(first) = values.get(&value) {
                    self.diagnostics.push(
                        CftDiagnostic::error(
                            CftErrorCode::DuplicateEnumValue,
                            info.module.clone(),
                            variant.span,
                            format!("duplicate enum value `{value}`"),
                        )
                        .with_related(
                            first.1.clone(),
                            first.2,
                            "first value is here",
                        ),
                    );
                } else {
                    values.insert(
                        value,
                        (variant.name.clone(), info.module.clone(), variant.span),
                    );
                }
                if info.is_flag && value != 0 && !is_i64_power_of_two(value) {
                    self.push_diag(
                        CftErrorCode::InvalidFlagEnumValue,
                        &info.module,
                        variant.span,
                        "@flag enum values must be powers of two, except zero",
                    );
                }
                variants.insert(variant.name.clone());
                // First definition wins on name collisions; later duplicates
                // already raised `DuplicateEnumVariant` above.
                values_by_name.entry(variant.name.clone()).or_insert(value);
            }
            if let Some(stored) = self.enums.get_mut(&name) {
                stored.variants = variants;
                stored.values = values
                    .into_iter()
                    .map(|(value, (_, module, span))| (value, (module, span)))
                    .collect();
                stored.values_by_name = values_by_name;
            }
        }
    }
}
