use super::annotations::has_annotation;
use super::inferred_type::{is_valid_dict_key, InferredType};
use super::state::{FieldInfo, SymbolKind};
use super::SchemaCompiler;
use crate::diagnostics::{CftDiagnostic, CftErrorCode};
use crate::module::ModuleId;
use crate::syntax::ast::{TypeRef, TypeRefKind};
use crate::syntax::Span;
use std::collections::BTreeMap;

impl SchemaCompiler<'_> {
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

    pub(super) fn build_full_fields(&mut self) {
        let names = self.types.keys().cloned().collect::<Vec<_>>();
        for name in names {
            let chain = self.ancestry_chain(&name);
            let mut map = BTreeMap::new();
            for info in chain {
                for field in &info.def.fields {
                    let declared_ty = self.resolve_field_type(&field.ty);
                    map.insert(
                        field.name.clone(),
                        FieldInfo {
                            inferred_type: declared_ty,
                        },
                    );
                }
            }
            self.full_fields.insert(name, map);
        }
    }

    /// Resolves a `TypeRef` to an `InferredType` without emitting diagnostics. Errors
    /// (unknown names, invalid dict keys) are reported once by
    /// [`Self::validate_field_type`] during `validate_field_shapes`; later
    /// passes that need the resolved type just consume the result here.
    pub(super) fn resolve_field_type(&self, ty: &TypeRef) -> InferredType {
        match &ty.kind {
            TypeRefKind::Int => InferredType::int(),
            TypeRefKind::Float => InferredType::float(),
            TypeRefKind::Bool => InferredType::bool(),
            TypeRefKind::String => InferredType::string(),
            TypeRefKind::Named(name) => match self.symbols.get(name) {
                Some(symbol) if symbol.kind == SymbolKind::Type => {
                    InferredType::object(crate::TypeName::from_validated(name.clone()))
                }
                Some(symbol) if symbol.kind == SymbolKind::Enum => {
                    InferredType::enum_value(crate::EnumName::from_validated(name.clone()))
                }
                _ => InferredType::Unknown,
            },
            TypeRefKind::Ref(inner) => InferredType::record_ref(self.resolve_field_type(inner)),
            TypeRefKind::Array(inner) => InferredType::array(self.resolve_field_type(inner)),
            TypeRefKind::Dict(key, value) => {
                InferredType::dict(self.resolve_field_type(key), self.resolve_field_type(value))
            }
            TypeRefKind::Nullable(inner) => InferredType::nullable(self.resolve_field_type(inner)),
        }
    }

    /// Walks a `TypeRef` once, emitting `UnknownNamedType` / `InvalidDictKeyType`
    /// diagnostics and returning the resolved type.
    fn validate_field_type(&mut self, module: &ModuleId, ty: &TypeRef) -> InferredType {
        match &ty.kind {
            TypeRefKind::Int => InferredType::int(),
            TypeRefKind::Float => InferredType::float(),
            TypeRefKind::Bool => InferredType::bool(),
            TypeRefKind::String => InferredType::string(),
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
                    InferredType::object(crate::TypeName::from_validated(name.clone()))
                }
                Some(symbol) if symbol.kind == SymbolKind::Enum => {
                    InferredType::enum_value(crate::EnumName::from_validated(name.clone()))
                }
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
                    InferredType::Unknown
                }
                None => {
                    self.push_diag(
                        CftErrorCode::UnknownNamedType,
                        module,
                        ty.span,
                        format!("unknown field type `{name}`"),
                    );
                    InferredType::Unknown
                }
            },
            TypeRefKind::Ref(inner) => {
                let inner_ty = self.validate_field_type(module, inner);
                match inner_ty.object_name() {
                    Some(name) if self.type_is_singleton(name) => {
                        self.push_diag(
                            CftErrorCode::InvalidAnnotatedFieldType,
                            module,
                            inner.span,
                            "reference target type must not be a singleton type",
                        );
                    }
                    Some(_) => {}
                    None if inner_ty.is_unknown() => {}
                    None => {
                        self.push_diag(
                            CftErrorCode::InvalidAnnotatedFieldType,
                            module,
                            inner.span,
                            "reference target must be a non-singleton object type",
                        );
                    }
                }
                InferredType::record_ref(inner_ty)
            }
            TypeRefKind::Array(inner) => {
                let inner = self.validate_field_type(module, inner);
                InferredType::array(inner)
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
                InferredType::dict(key_ty, value_ty)
            }
            TypeRefKind::Nullable(inner) => {
                let inner = self.validate_field_type(module, inner);
                InferredType::nullable(inner)
            }
        }
    }

    fn type_is_singleton(&self, name: &str) -> bool {
        self.types
            .get(name)
            .is_some_and(|info| has_annotation(&info.def.annotations, "singleton"))
    }
}
