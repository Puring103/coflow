use super::SchemaCompiler;
use crate::ast::{TypeRef, TypeRefKind};
use crate::compiled::support::{
    has_annotation, is_valid_dict_key, FieldInfo, FieldOrigin, SymbolKind, Ty, TypeInfo,
};
use crate::error::{CftDiagnostic, CftErrorCode};
use crate::module_id::ModuleId;
use crate::span::Span;
use coflow_structure::{StructureKind, TraversalCursor};
use std::collections::{BTreeMap, BTreeSet};

impl<'a> SchemaCompiler<'a> {
    pub(super) fn validate_type_headers(&mut self) {
        self.each_type(|this, info| {
            if info.def.is_abstract && info.def.is_sealed {
                let span = info
                    .def
                    .abstract_span
                    .map_or(info.def.span, |span| span)
                    .join(info.def.sealed_span.map_or(info.def.span, |span| span));
                this.push_diag(
                    CftErrorCode::ConflictingTypeModifiers,
                    &info.module,
                    span,
                    "abstract and sealed modifiers cannot be combined",
                );
            }
            if let Some(parent) = &info.def.parent {
                match this.symbols.get(&parent.name) {
                    Some(symbol) if symbol.kind == SymbolKind::Type => {}
                    Some(symbol) => {
                        this.diagnostics.push(
                            CftDiagnostic::error(
                                CftErrorCode::ParentMustBeType,
                                info.module.clone(),
                                parent.span,
                                "parent must be a type",
                            )
                            .with_related(
                                symbol.module.clone(),
                                symbol.span,
                                "name is defined here",
                            ),
                        );
                    }
                    None => {
                        this.push_diag(
                            CftErrorCode::UnknownNamedType,
                            &info.module,
                            parent.span,
                            format!("unknown parent type `{}`", parent.name),
                        );
                    }
                }
            }
        });
    }

    pub(super) fn validate_field_shapes(&mut self) {
        self.each_type(|this, info| {
            let mut fields: BTreeMap<String, Span> = BTreeMap::new();
            for field in &info.def.fields {
                this.validate_identifier(&field.name, &info.module, field.name_span);
                if let Some(first_span) = fields.get(&field.name) {
                    this.diagnostics.push(
                        CftDiagnostic::error(
                            CftErrorCode::DuplicateFieldName,
                            info.module.clone(),
                            field.name_span,
                            format!("duplicate field `{}`", field.name),
                        )
                        .with_related(
                            info.module.clone(),
                            *first_span,
                            "first field is here",
                        ),
                    );
                } else {
                    fields.insert(field.name.clone(), field.name_span);
                }
                this.validate_field_type(&info.module, &field.ty);
            }
        });
    }

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

    pub(super) fn build_full_fields(&mut self) {
        let names = self.types.keys().cloned().collect::<Vec<_>>();
        for name in names {
            let chain = self.ancestry_chain(&name);
            let mut map = BTreeMap::new();
            for info in chain {
                for field in &info.def.fields {
                    let declared_ty = self.resolve_field_type(&field.ty);
                    let check_ty = declared_ty;
                    map.insert(field.name.clone(), FieldInfo { check_ty });
                }
            }
            self.full_fields.insert(name, map);
        }
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

    /// Resolves a `TypeRef` to a `Ty` without emitting diagnostics. Errors
    /// (unknown names, invalid dict keys) are reported once by
    /// [`Self::validate_field_type`] during `validate_field_shapes`; later
    /// passes that need the resolved type just consume the result here.
    pub(super) fn resolve_field_type(&self, ty: &TypeRef) -> Ty {
        match &ty.kind {
            TypeRefKind::Int => Ty::Int,
            TypeRefKind::Float => Ty::Float,
            TypeRefKind::Bool => Ty::Bool,
            TypeRefKind::String => Ty::String,
            TypeRefKind::Named(name) => match self.symbols.get(name) {
                Some(symbol) if symbol.kind == SymbolKind::Type => Ty::Type(name.clone()),
                Some(symbol) if symbol.kind == SymbolKind::Enum => Ty::Enum(name.clone()),
                _ => Ty::Unknown,
            },
            TypeRefKind::Ref(inner) => Ty::Ref(Box::new(self.resolve_field_type(inner))),
            TypeRefKind::Array(inner) => Ty::Array(Box::new(self.resolve_field_type(inner))),
            TypeRefKind::Dict(key, value) => Ty::Dict(
                Box::new(self.resolve_field_type(key)),
                Box::new(self.resolve_field_type(value)),
            ),
            TypeRefKind::Nullable(inner) => Ty::Nullable(Box::new(self.resolve_field_type(inner))),
        }
    }

    /// Walks a `TypeRef` once, emitting `UnknownNamedType` / `InvalidDictKeyType`
    /// diagnostics and returning the resolved type.
    fn validate_field_type(&mut self, module: &ModuleId, ty: &TypeRef) -> Ty {
        match &ty.kind {
            TypeRefKind::Int => Ty::Int,
            TypeRefKind::Float => Ty::Float,
            TypeRefKind::Bool => Ty::Bool,
            TypeRefKind::String => Ty::String,
            TypeRefKind::Named(name) => match self.symbols.get(name) {
                Some(symbol) if symbol.kind == SymbolKind::Type => {
                    if self.type_is_singleton(name) {
                        self.push_diag(
                            CftErrorCode::InvalidAnnotatedFieldType,
                            module,
                            ty.span,
                            "singleton type cannot be used as a field type",
                        );
                    }
                    Ty::Type(name.clone())
                }
                Some(symbol) if symbol.kind == SymbolKind::Enum => Ty::Enum(name.clone()),
                Some(symbol) => {
                    self.diagnostics.push(
                        CftDiagnostic::error(
                            CftErrorCode::UnknownNamedType,
                            module.clone(),
                            ty.span,
                            format!("field type `{name}` is not a type or enum"),
                        )
                        .with_related(
                            symbol.module.clone(),
                            symbol.span,
                            "name is defined here",
                        ),
                    );
                    Ty::Unknown
                }
                None => {
                    self.push_diag(
                        CftErrorCode::UnknownNamedType,
                        module,
                        ty.span,
                        format!("unknown field type `{name}`"),
                    );
                    Ty::Unknown
                }
            },
            TypeRefKind::Ref(inner) => {
                let inner_ty = self.validate_field_type(module, inner);
                match &inner_ty {
                    Ty::Type(name) if self.type_is_singleton(name) => {
                        self.push_diag(
                            CftErrorCode::InvalidAnnotatedFieldType,
                            module,
                            inner.span,
                            "reference target type must not be a singleton type",
                        );
                    }
                    Ty::Type(_) | Ty::Unknown => {}
                    _ => {
                        self.push_diag(
                            CftErrorCode::InvalidAnnotatedFieldType,
                            module,
                            inner.span,
                            "reference target must be a non-singleton object type",
                        );
                    }
                }
                Ty::Ref(Box::new(inner_ty))
            }
            TypeRefKind::Array(inner) => {
                let inner = self.validate_field_type(module, inner);
                Ty::Array(Box::new(inner))
            }
            TypeRefKind::Dict(key, value) => {
                let key_ty = self.validate_field_type(module, key);
                if !is_valid_dict_key(&key_ty) {
                    self.push_diag(
                        CftErrorCode::InvalidDictKeyType,
                        module,
                        key.span,
                        "dict key type must be string, int, or enum",
                    );
                }
                let value_ty = self.validate_field_type(module, value);
                Ty::Dict(Box::new(key_ty), Box::new(value_ty))
            }
            TypeRefKind::Nullable(inner) => {
                let inner = self.validate_field_type(module, inner);
                Ty::Nullable(Box::new(inner))
            }
        }
    }

    fn type_is_singleton(&self, name: &str) -> bool {
        self.types
            .get(name)
            .is_some_and(|info| has_annotation(&info.def.annotations, "singleton"))
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
