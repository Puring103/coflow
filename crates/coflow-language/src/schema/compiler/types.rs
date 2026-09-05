use super::annotations::has_annotation;
use super::inferred_type::{is_valid_dict_key, InferredType};
use super::state::{FieldInfo, SymbolKind};
use super::ResolvedTypes;
use crate::diagnostics::{CftDiagnostic, CftErrorCode};
use crate::module::ModuleId;
use crate::syntax::ast::{TypeRef, TypeRefKind};
use crate::source::Span;
use std::collections::BTreeMap;

impl ResolvedTypes<'_> {
    pub(super) fn validate_type_aliases(&mut self) {
        let names = self.aliases.keys().cloned().collect::<Vec<_>>();
        let mut visiting = Vec::new();
        for name in names {
            let _ = self.resolve_type_alias(&name, &mut visiting);
        }
    }

    fn resolve_type_alias(
        &mut self,
        name: &str,
        visiting: &mut Vec<String>,
    ) -> InferredType {
        if let Some(resolved) = self.resolved_aliases.get(name) {
            return resolved.clone();
        }
        let Some(info) = self.aliases.get(name).cloned() else {
            return InferredType::Unknown;
        };
        if let Some(start) = visiting.iter().position(|entry| entry == name) {
            let mut cycle = visiting[start..].to_vec();
            cycle.push(name.to_string());
            self.push_diag(
                CftErrorCode::TypeAliasCycle,
                &info.module,
                info.def.target.span,
                format!("type alias cycle: {}", cycle.join(" -> ")),
            );
            return InferredType::Unknown;
        }

        visiting.push(name.to_string());
        let resolved = self.resolve_alias_target(&info.module, &info.def.target, visiting);
        visiting.pop();
        self.resolved_aliases
            .insert(name.to_string(), resolved.clone());
        resolved
    }

    fn resolve_alias_target(
        &mut self,
        module: &ModuleId,
        ty: &TypeRef,
        visiting: &mut Vec<String>,
    ) -> InferredType {
        match &ty.kind {
            TypeRefKind::Int => InferredType::int(),
            TypeRefKind::Float => InferredType::float(),
            TypeRefKind::Bool => InferredType::bool(),
            TypeRefKind::String => InferredType::string(),
            TypeRefKind::Named(name) => {
                let resolved_name = name.to_string();
                match self.symbols.get(&resolved_name).map(|symbol| symbol.kind) {
                    Some(SymbolKind::Type) => {
                        if self.type_is_singleton(&resolved_name) {
                            self.push_diag(
                                CftErrorCode::InvalidAnnotatedFieldType,
                                module,
                                ty.span,
                                "singleton type cannot be used as a value type",
                            );
                        }
                        InferredType::object(crate::TypeName::from_validated(resolved_name))
                    }
                    Some(SymbolKind::Enum) => {
                        InferredType::enum_value(crate::EnumName::from_validated(resolved_name))
                    }
                    Some(SymbolKind::TypeAlias) => {
                        self.resolve_type_alias(&resolved_name, visiting)
                    }
                    Some(_) => {
                        let symbol = self.symbols.get(&resolved_name).cloned();
                        let mut diagnostic = CftDiagnostic::error(
                            CftErrorCode::UnknownNamedType,
                            module.clone(),
                            ty.span,
                            format!("alias target `{resolved_name}` is not a type or enum"),
                        );
                        if let Some(symbol) = symbol {
                            diagnostic = diagnostic.with_related(
                                symbol.module,
                                symbol.span,
                                "name is defined here",
                            );
                        }
                        self.diagnostics.push(diagnostic);
                        InferredType::Unknown
                    }
                    None => {
                        self.push_diag(
                            CftErrorCode::UnknownNamedType,
                            module,
                            ty.span,
                            format!("unknown alias target `{resolved_name}`"),
                        );
                        InferredType::Unknown
                    }
                }
            }
            TypeRefKind::Ref(inner) => {
                let inner_ty = self.resolve_alias_target(module, inner, visiting);
                match inner_ty.object_name() {
                    Some(name) if self.type_is_singleton(name) => self.push_diag(
                        CftErrorCode::InvalidAnnotatedFieldType,
                        module,
                        inner.span,
                        "reference target type must not be a singleton type",
                    ),
                    Some(_) => {}
                    None if inner_ty.is_unknown() => {}
                    None => self.push_diag(
                        CftErrorCode::InvalidAnnotatedFieldType,
                        module,
                        inner.span,
                        "reference target must be a non-singleton object type",
                    ),
                }
                InferredType::record_ref(inner_ty)
            }
            TypeRefKind::Array(inner) => {
                InferredType::array(self.resolve_alias_target(module, inner, visiting))
            }
            TypeRefKind::Dict(key, value) => {
                let key_ty = self.resolve_alias_target(module, key, visiting);
                if !key_ty.is_unknown() && !is_valid_dict_key(&key_ty) {
                    self.push_diag(
                        CftErrorCode::InvalidDictKeyType,
                        module,
                        key.span,
                        "dict key type must be string, int, or enum",
                    );
                }
                InferredType::dict(
                    key_ty,
                    self.resolve_alias_target(module, value, visiting),
                )
            }
            TypeRefKind::Option(inner) => {
                InferredType::option(self.resolve_alias_target(module, inner, visiting))
            }
            TypeRefKind::Result(value, error) => InferredType::result(
                self.resolve_alias_target(module, value, visiting),
                self.resolve_alias_target(module, error, visiting),
            ),
            TypeRefKind::Function(parameters, result) => InferredType::function(
                parameters
                    .iter()
                    .map(|parameter| {
                        (
                            parameter.name.as_ref().map(|name| name.name.clone()),
                            self.resolve_alias_target(module, &parameter.value_type, visiting),
                        )
                    })
                    .collect(),
                self.resolve_alias_target(module, result, visiting),
            ),
            TypeRefKind::Unit => InferredType::unit(),
        }
    }

    pub(super) fn validate_type_headers(&mut self) {
        let mut diagnostics = Vec::new();
        for info in self.types.values() {
            if info.def.is_abstract && info.def.is_sealed {
                let span = info
                    .def
                    .abstract_span
                    .map_or(info.def.span, |span| span)
                    .join(info.def.sealed_span.map_or(info.def.span, |span| span));
                diagnostics.push(CftDiagnostic::error(
                    CftErrorCode::ConflictingTypeModifiers,
                    info.module.clone(),
                    span,
                    "abstract and sealed modifiers cannot be combined",
                ));
            }
            if let Some(parent) = &info.def.parent {
                let parent_name = parent.name.clone();
                match self.symbols.get(&parent_name) {
                    Some(symbol) if symbol.kind == SymbolKind::Type => {}
                    Some(symbol) => {
                        diagnostics.push(
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
                        diagnostics.push(CftDiagnostic::error(
                            CftErrorCode::UnknownNamedType,
                            info.module.clone(),
                            parent.span,
                            format!("unknown parent type `{parent_name}`"),
                        ));
                    }
                }
            }
        }
        self.diagnostics.extend(diagnostics);
    }

    pub(super) fn validate_field_shapes(&mut self) {
        let mut diagnostics = Vec::new();
        for info in self.types.values() {
            let mut fields: BTreeMap<String, Span> = BTreeMap::new();
            for field in &info.def.fields {
                if crate::is_cft_reserved_identifier(&field.name) {
                    diagnostics.push(CftDiagnostic::error(
                        CftErrorCode::ReservedIdentifier,
                        info.module.clone(),
                        field.name_span,
                        format!("`{}` is a reserved identifier", field.name),
                    ));
                }
                if let Some(first_span) = fields.get(&field.name) {
                    diagnostics.push(
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
                let value_type =
                    self.validate_field_type(&info.module, &field.ty, &mut diagnostics);
                if value_type_contains_data_result(&value_type) {
                    diagnostics.push(CftDiagnostic::error(
                        CftErrorCode::ResultDataField,
                        info.module.clone(),
                        field.ty.span,
                        "Result cannot be used as an object data field type",
                    ));
                }
            }
        }
        self.diagnostics.extend(diagnostics);
    }

    pub(super) fn build_full_fields(&mut self) {
        self.full_fields = self
            .types
            .keys()
            .map(|name| {
                let mut map = BTreeMap::new();
                for info in self.ancestry_chain(name) {
                    for field in &info.def.fields {
                        let declared_ty = self.resolve_field_type(&info.module, &field.ty);
                        map.insert(
                            field.name.clone(),
                            FieldInfo {
                                declaring_type: crate::TypeName::from_validated(info.name.clone()),
                                inferred_type: declared_ty,
                                dimension: super::annotations::field_dimension_name(field),
                                span: field.span,
                            },
                        );
                    }
                }
                (name.clone(), map)
            })
            .collect();
    }

    /// 必填 object 字段必须能够有限展开；Option、集合和引用会终止默认创建路径。
    pub(super) fn validate_required_object_cycles(&mut self) {
        #[derive(Clone)]
        struct Edge {
            field: String,
            target: String,
            declaring_type: String,
            span: Span,
        }

        let graph = self
            .full_fields
            .iter()
            .map(|(owner, fields)| {
                let edges = fields
                    .iter()
                    .filter_map(|(name, field)| match &field.inferred_type {
                        InferredType::Value(crate::CftValueType::Object(target)) => Some(Edge {
                            field: name.clone(),
                            target: target.to_string(),
                            declaring_type: field.declaring_type.to_string(),
                            span: field.span,
                        }),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                (owner.clone(), edges)
            })
            .collect::<BTreeMap<_, _>>();

        let mut states = BTreeMap::<String, u8>::new();
        for root in graph.keys() {
            if states.get(root).copied() == Some(2) {
                continue;
            }
            let mut nodes = vec![root.clone()];
            let mut incoming = Vec::<Edge>::new();
            let mut frames = vec![(root.clone(), 0usize)];
            states.insert(root.clone(), 1);

            while let Some((owner, next_edge)) = frames.last_mut() {
                let edges = graph.get(owner).map_or(&[][..], Vec::as_slice);
                let Some(edge) = edges.get(*next_edge).cloned() else {
                    let completed = owner.clone();
                    frames.pop();
                    nodes.pop();
                    if !frames.is_empty() {
                        incoming.pop();
                    }
                    states.insert(completed, 2);
                    continue;
                };
                *next_edge += 1;

                match states.get(&edge.target).copied() {
                    Some(1) => {
                        let start = nodes
                            .iter()
                            .position(|node| node == &edge.target)
                            .unwrap_or(0);
                        let mut cycle = incoming[start..]
                            .iter()
                            .map(|step| format!("{}.{}", nodes[start], step.field))
                            .collect::<Vec<_>>();
                        cycle.push(format!("{owner}.{}", edge.field));
                        cycle.push(edge.target.clone());
                        let module = self
                            .types
                            .get(&edge.declaring_type)
                            .map_or_else(|| self.types[owner].module.clone(), |info| {
                                info.module.clone()
                            });
                        self.diagnostics.push(CftDiagnostic::error(
                            CftErrorCode::RequiredObjectCycle,
                            module,
                            edge.span,
                            format!(
                                "required object field cycle: {}",
                                cycle.join(" -> ")
                            ),
                        ));
                    }
                    Some(2) => {}
                    _ => {
                        states.insert(edge.target.clone(), 1);
                        nodes.push(edge.target.clone());
                        incoming.push(edge.clone());
                        frames.push((edge.target, 0));
                    }
                }
            }
        }
    }

    /// Resolves a `TypeRef` to an `InferredType` without emitting diagnostics. Errors
    /// (unknown names, invalid dict keys) are reported once by
    /// [`Self::validate_field_type`] during `validate_field_shapes`; later
    /// passes that need the resolved type just consume the result here.
    pub(super) fn resolve_field_type(&self, module: &ModuleId, ty: &TypeRef) -> InferredType {
        match &ty.kind {
            TypeRefKind::Int => InferredType::int(),
            TypeRefKind::Float => InferredType::float(),
            TypeRefKind::Bool => InferredType::bool(),
            TypeRefKind::String => InferredType::string(),
            TypeRefKind::Named(name) => {
                let name = name.to_string();
                match self.symbols.get(&name) {
                Some(symbol) if symbol.kind == SymbolKind::Type => {
                    InferredType::object(crate::TypeName::from_validated(name))
                }
                Some(symbol) if symbol.kind == SymbolKind::Enum => {
                    InferredType::enum_value(crate::EnumName::from_validated(name))
                }
                Some(symbol) if symbol.kind == SymbolKind::TypeAlias => self
                    .resolved_aliases
                    .get(&name)
                    .cloned()
                    .unwrap_or(InferredType::Unknown),
                _ => InferredType::Unknown,
                }
            }
            TypeRefKind::Ref(inner) => {
                InferredType::record_ref(self.resolve_field_type(module, inner))
            }
            TypeRefKind::Array(inner) => {
                InferredType::array(self.resolve_field_type(module, inner))
            }
            TypeRefKind::Dict(key, value) => {
                InferredType::dict(
                    self.resolve_field_type(module, key),
                    self.resolve_field_type(module, value),
                )
            }
            TypeRefKind::Option(inner) => {
                InferredType::option(self.resolve_field_type(module, inner))
            }
            TypeRefKind::Result(value, error) => InferredType::result(
                self.resolve_field_type(module, value),
                self.resolve_field_type(module, error),
            ),
            TypeRefKind::Function(parameters, result) => InferredType::function(
                parameters
                    .iter()
                    .map(|parameter| {
                        (
                            parameter.name.as_ref().map(|name| name.name.clone()),
                            self.resolve_field_type(module, &parameter.value_type),
                        )
                    })
                    .collect(),
                self.resolve_field_type(module, result),
            ),
            TypeRefKind::Unit => InferredType::unit(),
        }
    }

    /// 单次遍历字段类型，只读取已解析阶段，并把诊断写入本次校验的局部结果。
    fn validate_field_type(
        &self,
        module: &ModuleId,
        ty: &TypeRef,
        diagnostics: &mut Vec<CftDiagnostic>,
    ) -> InferredType {
        let diagnostic = |code, span, message: String| {
            CftDiagnostic::error(code, module.clone(), span, message)
        };
        match &ty.kind {
            TypeRefKind::Int => InferredType::int(),
            TypeRefKind::Float => InferredType::float(),
            TypeRefKind::Bool => InferredType::bool(),
            TypeRefKind::String => InferredType::string(),
            TypeRefKind::Named(name) => {
                let resolved_name = name.to_string();
                match self.symbols.get(&resolved_name) {
                    Some(symbol) if symbol.kind == SymbolKind::Type => {
                        if self.type_is_singleton(&resolved_name) {
                            diagnostics.push(diagnostic(
                                CftErrorCode::InvalidAnnotatedFieldType,
                                ty.span,
                                "singleton type cannot be used as a field type".into(),
                            ));
                        }
                        InferredType::object(crate::TypeName::from_validated(resolved_name))
                    }
                    Some(symbol) if symbol.kind == SymbolKind::Enum => {
                        InferredType::enum_value(crate::EnumName::from_validated(resolved_name))
                    }
                    Some(symbol) if symbol.kind == SymbolKind::TypeAlias => self
                        .resolved_aliases
                        .get(&resolved_name)
                        .cloned()
                        .unwrap_or(InferredType::Unknown),
                    Some(symbol) => {
                        diagnostics.push(
                            diagnostic(
                                CftErrorCode::UnknownNamedType,
                                ty.span,
                                format!("field type `{resolved_name}` is not a type or enum"),
                            )
                            .with_related(
                                symbol.module.clone(),
                                symbol.span,
                                "name is defined here",
                            ),
                        );
                        InferredType::Unknown
                    }
                    None => {
                        diagnostics.push(diagnostic(
                            CftErrorCode::UnknownNamedType,
                            ty.span,
                            format!("unknown field type `{resolved_name}`"),
                        ));
                        InferredType::Unknown
                    }
                }
            }
            TypeRefKind::Ref(inner) => {
                let inner_ty = self.validate_field_type(module, inner, diagnostics);
                match inner_ty.object_name() {
                    Some(name) if self.type_is_singleton(name) => diagnostics.push(diagnostic(
                        CftErrorCode::InvalidAnnotatedFieldType,
                        inner.span,
                        "reference target type must not be a singleton type".into(),
                    )),
                    Some(_) => {}
                    None if inner_ty.is_unknown() => {}
                    None => diagnostics.push(diagnostic(
                        CftErrorCode::InvalidAnnotatedFieldType,
                        inner.span,
                        "reference target must be a non-singleton object type".into(),
                    )),
                }
                InferredType::record_ref(inner_ty)
            }
            TypeRefKind::Array(inner) => {
                InferredType::array(self.validate_field_type(module, inner, diagnostics))
            }
            TypeRefKind::Dict(key, value) => {
                let key_ty = self.validate_field_type(module, key, diagnostics);
                if !is_valid_dict_key(&key_ty) {
                    diagnostics.push(diagnostic(
                        CftErrorCode::InvalidDictKeyType,
                        key.span,
                        "dict key type must be string, int, or enum".into(),
                    ));
                }
                InferredType::dict(
                    key_ty,
                    self.validate_field_type(module, value, diagnostics),
                )
            }
            TypeRefKind::Option(inner) => {
                InferredType::option(self.validate_field_type(module, inner, diagnostics))
            }
            TypeRefKind::Result(value, error) => InferredType::result(
                self.validate_field_type(module, value, diagnostics),
                self.validate_field_type(module, error, diagnostics),
            ),
            TypeRefKind::Function(parameters, result) => InferredType::function(
                parameters
                    .iter()
                    .map(|parameter| {
                        (
                            parameter.name.as_ref().map(|name| name.name.clone()),
                            self.validate_field_type(module, &parameter.value_type, diagnostics),
                        )
                    })
                    .collect(),
                self.validate_field_type(module, result, diagnostics),
            ),
            TypeRefKind::Unit => InferredType::unit(),
        }
    }

    fn type_is_singleton(&self, name: &str) -> bool {
        self.types
            .get(name)
            .is_some_and(|info| has_annotation(&info.def.annotations, "singleton"))
    }
}

fn value_type_contains_data_result(ty: &InferredType) -> bool {
    match ty {
        InferredType::Value(crate::CftValueType::Result(_, _)) => true,
        InferredType::Value(
            crate::CftValueType::Array(inner) | crate::CftValueType::Option(inner),
        ) => value_type_contains_result(inner),
        InferredType::Value(crate::CftValueType::Dict(key, value)) => {
            value_type_contains_result(key) || value_type_contains_result(value)
        }
        // 函数字段中的 Result 是函数协议的一部分，不是 object 数据字段。
        InferredType::Value(crate::CftValueType::Function(_, _)) | InferredType::Unknown => false,
        InferredType::Value(_) | InferredType::EnumNamespace(_) | InferredType::Entry(_, _) => false,
    }
}

fn value_type_contains_result(ty: &crate::CftValueType) -> bool {
    match ty {
        crate::CftValueType::Result(_, _) => true,
        crate::CftValueType::Array(inner) | crate::CftValueType::Option(inner) => {
            value_type_contains_result(inner)
        }
        crate::CftValueType::Dict(key, value) => {
            value_type_contains_result(key) || value_type_contains_result(value)
        }
        crate::CftValueType::Function(_, _) => false,
        _ => false,
    }
}
