use super::state::{FieldOrigin, TypeInfo};
use super::SchemaCompiler;
use crate::diagnostics::{CftDiagnostic, CftErrorCode};
use crate::module::ModuleId;
use crate::syntax::Span;
use coflow_structure::{StructureKind, TraversalCursor};
use std::collections::{BTreeMap, BTreeSet};

impl<'a> SchemaCompiler<'a> {
    pub(super) fn validate_inheritance(&mut self) -> bool {
        let names = self.types.keys().cloned().collect::<Vec<_>>();
        let Some(has_cycle) = self.build_inheritance_chains(&names) else {
            return false;
        };
        self.validate_inherited_fields(&names);
        !has_cycle
    }

    fn build_inheritance_chains(&mut self, names: &[String]) -> Option<bool> {
        let mut finished = BTreeSet::new();
        let mut has_cycle = false;
        for name in names {
            if finished.contains(name) {
                continue;
            }
            let mut path = Vec::new();
            let mut positions = BTreeMap::new();
            let mut current = name.clone();
            let mut path_has_cycle = false;
            let terminal_parent = loop {
                positions.insert(current.clone(), path.len());
                path.push(current.clone());
                let local_depth = u64::try_from(path.len()).unwrap_or(u64::MAX);
                if let Err(error) = self.budget.check_additional_depth(
                    TraversalCursor::root(),
                    StructureKind::SchemaDependency,
                    local_depth,
                ) {
                    let (module, span) = self.inheritance_edge_location(&current);
                    self.push_budget_error(error, &module, span);
                    return None;
                }

                let Some(parent) = self
                    .types
                    .get(&current)
                    .and_then(|info| info.def.parent.as_ref())
                    .filter(|parent| self.types.contains_key(&parent.name))
                    .map(|parent| parent.name.clone())
                else {
                    break None;
                };

                // Semantic cycles take precedence over the resource budget. In
                // particular, a self-cycle must remain an inheritance error at
                // the smallest possible depth limit.
                if let Some(cycle_start) = positions.get(&parent).copied() {
                    self.report_inheritance_cycle(&path[cycle_start..]);
                    has_cycle = true;
                    path_has_cycle = true;
                    break None;
                }

                let (module, span) = self.inheritance_edge_location(&current);
                if let Err(error) = self.budget.charge_work(StructureKind::SchemaDependency, 1) {
                    self.push_budget_error(error, &module, span);
                    return None;
                }
                if finished.contains(&parent) {
                    break Some(parent);
                }
                current = parent;
            };

            if path_has_cycle {
                // A path that enters a cycle has no valid root-first ancestry
                // chain. Mark it complete so the same cycle is not diagnosed
                // again from a descendant.
                finished.extend(path);
                continue;
            }

            let mut chain = terminal_parent
                .and_then(|parent| self.inheritance_chains.get(&parent).cloned())
                .unwrap_or_default();
            for current in path.iter().rev() {
                chain.push(current.clone());
                let depth = u64::try_from(chain.len()).unwrap_or(u64::MAX);
                if let Err(error) = self.budget.check_additional_depth(
                    TraversalCursor::root(),
                    StructureKind::SchemaDependency,
                    depth,
                ) {
                    let (module, span) = self.inheritance_edge_location(current);
                    self.push_budget_error(error, &module, span);
                    return None;
                }
                self.inheritance_chains
                    .insert(current.clone(), chain.clone());
                finished.insert(current.clone());
            }
        }
        Some(has_cycle)
    }

    fn validate_inherited_fields(&mut self, names: &[String]) {
        for name in names {
            let Some(info) = self.types.get(name).cloned() else {
                continue;
            };
            if let Some(parent) = &info.def.parent {
                if let Some(parent_info) = self.types.get(&parent.name) {
                    if parent_info.def.is_sealed {
                        self.diagnostics.push(
                            CftDiagnostic::error(
                                CftErrorCode::InheritSealedType,
                                info.module.clone(),
                                parent.span,
                                format!("cannot inherit sealed type `{}`", parent.name),
                            )
                            .with_related(
                                parent_info.module.clone(),
                                parent_info.def.name_span,
                                "sealed type is defined here",
                            ),
                        );
                    }
                    let inherited = self.collect_ancestor_fields(Some(&parent.name));
                    for field in &info.def.fields {
                        if let Some(first) = inherited.get(&field.name) {
                            self.diagnostics.push(
                                CftDiagnostic::error(
                                    CftErrorCode::DuplicateInheritedField,
                                    info.module.clone(),
                                    field.name_span,
                                    format!("field `{}` already exists in an ancestor", field.name),
                                )
                                .with_related(
                                    first.module.clone(),
                                    first.span,
                                    "ancestor field is here",
                                ),
                            );
                        }
                    }
                }
            }
        }
    }

    fn inheritance_edge_location(&self, name: &str) -> (ModuleId, Span) {
        let info = &self.types[name];
        (
            info.module.clone(),
            info.def
                .parent
                .as_ref()
                .map_or(info.def.name_span, |parent| parent.span),
        )
    }

    fn report_inheritance_cycle(&mut self, cycle: &[String]) {
        let Some((anchor_index, anchor)) = cycle.iter().enumerate().min_by_key(|(_, name)| {
            let info = &self.types[*name];
            let span = info
                .def
                .parent
                .as_ref()
                .map_or(info.def.name_span, |parent| parent.span);
            (info.module.as_str(), span.start, name.as_str())
        }) else {
            return;
        };
        let (module, span) = self.inheritance_edge_location(anchor);
        let mut diagnostic = CftDiagnostic::error(
            CftErrorCode::InheritanceCycle,
            module,
            span,
            "inheritance cycle detected",
        );
        for name in cycle
            .iter()
            .cycle()
            .skip(anchor_index + 1)
            .take(cycle.len().saturating_sub(1))
        {
            let (module, span) = self.inheritance_edge_location(name);
            diagnostic = diagnostic.with_related(module, span, "cycle continues here");
        }
        self.diagnostics.push(diagnostic);
    }

    /// Walks the inheritance chain root-first and returns a snapshot of every
    /// ancestor (plus the type itself). Cycle-safe; unknown parents truncate
    /// the chain. Used by [`Self::build_full_fields`] and
    /// [`Self::collect_all_schema_fields`].
    pub(super) fn ancestry_chain(&self, type_name: &str) -> Vec<TypeInfo<'a>> {
        self.inheritance_chains
            .get(type_name)
            .into_iter()
            .flatten()
            .filter_map(|name| self.types.get(name).cloned())
            .collect()
    }
    pub(super) fn collect_ancestor_fields(
        &self,
        parent_name: Option<&str>,
    ) -> BTreeMap<String, FieldOrigin> {
        let mut out = BTreeMap::new();
        let Some(parent_name) = parent_name else {
            return out;
        };
        for name in self
            .inheritance_chains
            .get(parent_name)
            .into_iter()
            .flatten()
        {
            let Some(info) = self.types.get(name) else {
                continue;
            };
            for field in &info.def.fields {
                out.entry(field.name.clone())
                    .or_insert_with(|| FieldOrigin {
                        module: info.module.clone(),
                        span: field.name_span,
                    });
            }
        }
        out
    }
}
