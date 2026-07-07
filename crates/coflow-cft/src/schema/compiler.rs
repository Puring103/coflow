mod annotations;
mod build;
mod defaults;
mod symbols;

use super::support::{
    has_annotation, is_valid_dict_key, ConstInfo, EnumInfo, FieldInfo, FieldOrigin, Symbol,
    SymbolKind, Ty, TypeInfo,
};
use super::type_checker::TypeChecker;
use super::CompiledSchema;
use crate::ast::{ConstLiteral, TypeRef, TypeRefKind};
use crate::container::{CftContainer, ModuleId};
use crate::error::{CftDiagnostic, CftDiagnostics, CftErrorCode};
use crate::span::Span;
use std::collections::{BTreeMap, HashSet};

pub(super) struct SchemaCompiler<'a> {
    pub(super) container: &'a CftContainer,
    pub(super) diagnostics: Vec<CftDiagnostic>,
    pub(super) symbols: BTreeMap<String, Symbol>,
    pub(super) consts: BTreeMap<String, ConstInfo<'a>>,
    pub(super) types: BTreeMap<String, TypeInfo<'a>>,
    pub(super) enums: BTreeMap<String, EnumInfo<'a>>,
    pub(super) full_fields: BTreeMap<String, BTreeMap<String, FieldInfo>>,
}

impl<'a> SchemaCompiler<'a> {
    pub(super) fn new(container: &'a CftContainer) -> Self {
        Self {
            container,
            diagnostics: Vec::new(),
            symbols: BTreeMap::new(),
            consts: BTreeMap::new(),
            types: BTreeMap::new(),
            enums: BTreeMap::new(),
            full_fields: BTreeMap::new(),
        }
    }

    pub(super) fn compile(&mut self) -> Result<CompiledSchema, CftDiagnostics> {
        self.report_dangling_annotations();
        self.collect_symbols();
        self.validate_enums();
        self.validate_const_type_annotations();
        self.validate_type_headers();
        self.validate_field_shapes();
        self.validate_inheritance();
        self.validate_annotations();
        self.validate_defaults();
        self.build_full_fields();
        self.validate_checks();

        if !self.diagnostics.is_empty() {
            return Err(CftDiagnostics::new(std::mem::take(&mut self.diagnostics)));
        }
        Ok(self.build_schema())
    }

    fn validate_const_type_annotations(&mut self) {
        self.each_const(|this, info| {
            let Some(ty) = &info.def.ty else {
                return;
            };
            let type_name = match &ty.kind {
                TypeRefKind::Int => "int",
                TypeRefKind::Float => "float",
                TypeRefKind::Bool => "bool",
                TypeRefKind::String => "string",
                _ => {
                    this.push_diag(
                        CftErrorCode::InvalidConstValue,
                        &info.module,
                        ty.span,
                        "const type annotation must be int, float, bool, or string",
                    );
                    return;
                }
            };
            let matches = matches!(
                (&ty.kind, &info.def.value),
                (TypeRefKind::Int, ConstLiteral::Int(..))
                    | (TypeRefKind::Float, ConstLiteral::Float(..))
                    | (TypeRefKind::Bool, ConstLiteral::Bool(..))
                    | (TypeRefKind::String, ConstLiteral::String(..))
            );
            if !matches {
                let value_span = info.def.value.span();
                this.push_diag(
                    CftErrorCode::InvalidConstValue,
                    &info.module,
                    value_span,
                    format!("const value does not match type annotation `{type_name}`"),
                );
            }
        });
    }

    fn validate_type_headers(&mut self) {
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

    fn validate_field_shapes(&mut self) {
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

    fn validate_inheritance(&mut self) {
        let names = self.types.keys().cloned().collect::<Vec<_>>();
        let mut visiting = HashSet::new();
        let mut visited = HashSet::new();
        for name in &names {
            self.detect_cycle(name, &mut visiting, &mut visited);
        }

        for name in &names {
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

    fn detect_cycle(
        &mut self,
        name: &str,
        visiting: &mut HashSet<String>,
        visited: &mut HashSet<String>,
    ) {
        if visited.contains(name) {
            return;
        }
        if !visiting.insert(name.to_string()) {
            if let Some(info) = self.types.get(name) {
                let span = info
                    .def
                    .parent
                    .as_ref()
                    .map_or(info.def.name_span, |p| p.span);
                let module = info.module.clone();
                self.push_diag(
                    CftErrorCode::InheritanceCycle,
                    &module,
                    span,
                    "inheritance cycle detected",
                );
            }
            return;
        }
        if let Some(parent) = self
            .types
            .get(name)
            .and_then(|info| info.def.parent.as_ref())
            .map(|parent| parent.name.clone())
        {
            if self.types.contains_key(&parent) {
                self.detect_cycle(&parent, visiting, visited);
            }
        }
        visiting.remove(name);
        visited.insert(name.to_string());
    }

    fn build_full_fields(&mut self) {
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
    fn ancestry_chain(&self, type_name: &str) -> Vec<TypeInfo<'a>> {
        let mut chain = Vec::new();
        let mut current = Some(type_name.to_string());
        let mut seen = HashSet::new();
        while let Some(name) = current {
            if !seen.insert(name.clone()) {
                break;
            }
            let Some(info) = self.types.get(&name).cloned() else {
                break;
            };
            current = info.def.parent.as_ref().map(|p| p.name.clone());
            chain.push(info);
        }
        chain.reverse();
        chain
    }

    fn validate_checks(&mut self) {
        self.each_type(|this, info| {
            if let Some(check) = &info.def.check {
                let mut checker = TypeChecker::new(this, info);
                checker.check_stmts(&check.stmts);
            }
        });
    }

    /// Resolves a `TypeRef` to a `Ty` without emitting diagnostics. Errors
    /// (unknown names, invalid dict keys) are reported once by
    /// [`Self::validate_field_type`] during `validate_field_shapes`; later
    /// passes that need the resolved type just consume the result here.
    fn resolve_field_type(&self, ty: &TypeRef) -> Ty {
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

    fn collect_ancestor_fields(&self, parent_name: Option<&str>) -> BTreeMap<String, FieldOrigin> {
        let mut out = BTreeMap::new();
        let mut current = parent_name.map(str::to_string);
        let mut seen = HashSet::new();
        while let Some(name) = current {
            if !seen.insert(name.clone()) {
                break;
            }
            let Some(info) = self.types.get(&name) else {
                break;
            };
            for field in &info.def.fields {
                out.entry(field.name.clone())
                    .or_insert_with(|| FieldOrigin {
                        module: info.module.clone(),
                        span: field.name_span,
                    });
            }
            current = info.def.parent.as_ref().map(|parent| parent.name.clone());
        }
        out
    }

    fn push_diag(
        &mut self,
        code: CftErrorCode,
        module: &ModuleId,
        span: Span,
        message: impl Into<String>,
    ) {
        self.diagnostics
            .push(CftDiagnostic::error(code, module.clone(), span, message));
    }

    /// Iterates over every type, releasing the borrow on `self.types` for each
    /// call so the body can use `&mut self`. Only the key snapshot is allocated
    /// upfront; each info is cloned one at a time inside the loop.
    fn each_type<F: FnMut(&mut Self, &TypeInfo<'a>)>(&mut self, mut body: F) {
        let keys: Vec<String> = self.types.keys().cloned().collect();
        for key in keys {
            if let Some(info) = self.types.get(&key).cloned() {
                body(self, &info);
            }
        }
    }

    fn each_enum<F: FnMut(&mut Self, &EnumInfo<'a>)>(&mut self, mut body: F) {
        let keys: Vec<String> = self.enums.keys().cloned().collect();
        for key in keys {
            if let Some(info) = self.enums.get(&key).cloned() {
                body(self, &info);
            }
        }
    }

    fn each_const<F: FnMut(&mut Self, &ConstInfo<'a>)>(&mut self, mut body: F) {
        let keys: Vec<String> = self.consts.keys().cloned().collect();
        for key in keys {
            if let Some(info) = self.consts.get(&key).cloned() {
                body(self, &info);
            }
        }
    }
}
