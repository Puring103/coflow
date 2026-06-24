use super::support::{
    build_schema_type_ref, const_value, convert_annotations, convert_check_block, find_annotation,
    format_type_ref, has_annotation, is_i64_power_of_two, is_reserved_identifier,
    is_valid_dict_key, types_assignable, AnnotationSpec, AnnotationTarget, ConstInfo, EnumInfo,
    FieldInfo, FieldOrigin, Symbol, SymbolKind, Ty, TypeInfo,
};
use super::type_checker::TypeChecker;
use super::{
    CftConstValue, CftSchemaConst, CftSchemaDefaultValue, CftSchemaEnum, CftSchemaEnumVariant,
    CftSchemaField, CftSchemaModule, CftSchemaType, CompiledSchema,
};
use crate::ast::{
    Annotation, ConstLiteral, DefaultExpr, DefaultExprKind, FieldDef, Item, TypeRef, TypeRefKind,
};
use crate::container::{CftContainer, ModuleId};
use crate::error::{CftDiagnostic, CftDiagnostics, CftErrorCode};
use crate::span::Span;
use std::collections::{BTreeMap, BTreeSet, HashSet};

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

    fn report_dangling_annotations(&mut self) {
        for (module_id, module) in &self.container.modules {
            for annotation in &module.ast.dangling_annotations {
                self.push_diag(
                    CftErrorCode::AnnotationWithoutTarget,
                    module_id,
                    annotation.span,
                    "annotation has no target",
                );
            }
            for item in &module.ast.items {
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

    fn collect_symbols(&mut self) {
        for (module_id, module) in &self.container.modules {
            for item in &module.ast.items {
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

    fn validate_identifier(&mut self, name: &str, module_id: &ModuleId, span: Span) {
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

    fn validate_enums(&mut self) {
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

    fn validate_annotations(&mut self) {
        self.each_enum(|this, info| {
            this.validate_annotation_list(
                &info.module,
                AnnotationTarget::Enum,
                &info.def.annotations,
            );
            for variant in &info.def.variants {
                this.validate_annotation_list(
                    &info.module,
                    AnnotationTarget::EnumVariant,
                    &variant.annotations,
                );
            }
        });

        let mut id_as_enum_names = BTreeMap::<String, (ModuleId, Span)>::new();
        self.each_type(|this, info| {
            this.validate_annotation_list(
                &info.module,
                AnnotationTarget::Type,
                &info.def.annotations,
            );
            if let Some(annotation) = find_annotation(&info.def.annotations, "struct") {
                if !info.def.is_sealed {
                    this.push_diag(
                        CftErrorCode::StructRequiresSealedType,
                        &info.module,
                        annotation.span,
                        "@struct requires a sealed type",
                    );
                }
            }
            if let Some(singleton) = find_annotation(&info.def.annotations, "singleton") {
                if info.def.is_abstract {
                    this.push_diag(
                        CftErrorCode::SingletonOnAbstractType,
                        &info.module,
                        singleton.span,
                        "@singleton cannot be applied to an abstract type",
                    );
                }
                if find_annotation(&info.def.annotations, "idAsEnum").is_some() {
                    this.push_diag(
                        CftErrorCode::SingletonIdAsEnumConflict,
                        &info.module,
                        singleton.span,
                        "@singleton cannot be combined with @idAsEnum",
                    );
                }
            }
            if let Some(annotation) = find_annotation(&info.def.annotations, "idAsEnum") {
                if let Some(crate::ast::AnnotationArg::Name(enum_name)) = annotation.args.first() {
                    this.validate_id_as_enum_name(&info.module, &enum_name.name, enum_name.span);
                    this.register_id_as_enum_name(
                        &mut id_as_enum_names,
                        &info.module,
                        annotation,
                        &enum_name.name,
                    );
                }
            }
        });

        self.each_type(|this, info| {
            for field in &info.def.fields {
                this.validate_annotation_list(
                    &info.module,
                    AnnotationTarget::Field,
                    &field.annotations,
                );
                this.validate_field_annotations(&info.module, field, info.def.is_sealed);
            }
        });
    }

    fn register_id_as_enum_name(
        &mut self,
        id_as_enum_names: &mut BTreeMap<String, (ModuleId, Span)>,
        module: &ModuleId,
        annotation: &Annotation,
        enum_name: &str,
    ) {
        if let Some((first_module, first_span)) = id_as_enum_names.get(enum_name) {
            self.diagnostics.push(
                CftDiagnostic::error(
                    CftErrorCode::DuplicateGlobalName,
                    module.clone(),
                    annotation.span,
                    format!("duplicate @idAsEnum enum name `{enum_name}`"),
                )
                .with_related(
                    first_module.clone(),
                    *first_span,
                    "first @idAsEnum enum name is here",
                ),
            );
        } else {
            id_as_enum_names.insert(enum_name.to_string(), (module.clone(), annotation.span));
        }
    }

    fn validate_annotation_list(
        &mut self,
        module: &ModuleId,
        target: AnnotationTarget,
        annotations: &[Annotation],
    ) {
        let mut seen = BTreeMap::<&str, Span>::new();
        for annotation in annotations {
            let Some(spec) = AnnotationSpec::for_name(&annotation.name) else {
                self.push_diag(
                    CftErrorCode::UnknownAnnotation,
                    module,
                    annotation.name_span,
                    format!("unknown annotation `{}`", annotation.name),
                );
                continue;
            };
            if let Some(first) = seen.get(annotation.name.as_str()) {
                self.diagnostics.push(
                    CftDiagnostic::error(
                        CftErrorCode::DuplicateAnnotation,
                        module.clone(),
                        annotation.span,
                        format!("duplicate annotation `{}`", annotation.name),
                    )
                    .with_related(
                        module.clone(),
                        *first,
                        "first annotation is here",
                    ),
                );
            } else {
                seen.insert(&annotation.name, annotation.span);
            }
            if !spec.targets.contains(&target) {
                let code = if annotation.name == "localized" {
                    CftErrorCode::LocalizedOnInvalidTarget
                } else {
                    CftErrorCode::InvalidAnnotationTarget
                };
                self.push_diag(
                    code,
                    module,
                    annotation.span,
                    format!("@{} cannot be applied to this target", annotation.name),
                );
            }
            if !spec.args_valid(annotation) {
                self.push_diag(
                    CftErrorCode::InvalidAnnotationArgument,
                    module,
                    annotation.span,
                    format!("@{} has invalid arguments", annotation.name),
                );
            }
        }
    }

    fn validate_field_annotations(
        &mut self,
        module: &ModuleId,
        field: &FieldDef,
        owner_is_sealed: bool,
    ) {
        if owner_is_sealed {
            if let Some(annotation) = find_annotation(&field.annotations, "localized") {
                self.push_diag(
                    CftErrorCode::LocalizedOnInvalidTarget,
                    module,
                    annotation.span,
                    "@localized can only appear on top-level type fields, not inside sealed types",
                );
            }
        }
        if let Some(annotation) = find_annotation(&field.annotations, "expand") {
            // @expand requires the field to reference a concrete `type`. Arrays,
            // dicts, primitives, enums, and nullable wrappers don't make sense
            // because the loader needs a known set of inner field names to
            // consume across adjacent header columns.
            let resolved = self.resolve_field_type(&field.ty);
            if !matches!(resolved, Ty::Type(_) | Ty::Unknown) {
                self.push_diag(
                    CftErrorCode::InvalidAnnotatedFieldType,
                    module,
                    annotation.span,
                    "@expand fields must reference a concrete type (no nullable, arrays, dicts, enums, or primitives)",
                );
            }
        }
        // Forbid referencing a singleton type from any field type position
        // (top-level, array element, dict value/key, nullable inner).
        self.check_no_singleton_reference(module, &field.ty);
    }

    fn check_no_singleton_reference(&mut self, module: &ModuleId, ty: &TypeRef) {
        match &ty.kind {
            TypeRefKind::Named(name) => {
                if let Some(info) = self.types.get(name) {
                    if has_annotation(&info.def.annotations, "singleton") {
                        let owner_module = info.module.clone();
                        let owner_span = info.def.name_span;
                        self.diagnostics.push(
                            CftDiagnostic::error(
                                CftErrorCode::SingletonNotReferenceable,
                                module.clone(),
                                ty.span,
                                format!("singleton type `{name}` cannot be used as a field type"),
                            )
                            .with_related(
                                owner_module,
                                owner_span,
                                "singleton is defined here",
                            ),
                        );
                    }
                }
            }
            TypeRefKind::Array(inner) | TypeRefKind::Nullable(inner) => {
                self.check_no_singleton_reference(module, inner);
            }
            TypeRefKind::Dict(key, value) => {
                self.check_no_singleton_reference(module, key);
                self.check_no_singleton_reference(module, value);
            }
            _ => {}
        }
    }

    fn validate_id_as_enum_name(
        &mut self,
        module: &ModuleId,
        enum_name: &str,
        enum_name_span: Span,
    ) {
        match self.symbols.get(enum_name) {
            Some(symbol) if symbol.kind == SymbolKind::Enum => {
                if let Some(info) = self.enums.get(enum_name) {
                    if !info.def.variants.is_empty() {
                        self.diagnostics.push(
                            CftDiagnostic::error(
                                CftErrorCode::IdAsEnumRequiresEmptyEnum,
                                module.clone(),
                                enum_name_span,
                                format!(
                                    "@idAsEnum enum `{enum_name}` must be declared with no variants"
                                ),
                            )
                            .with_related(
                                info.module.clone(),
                                info.def.name_span,
                                "enum placeholder is defined here",
                            ),
                        );
                    }
                }
            }
            Some(symbol) => {
                self.diagnostics.push(
                    CftDiagnostic::error(
                        CftErrorCode::IdAsEnumRequiresEmptyEnum,
                        module.clone(),
                        enum_name_span,
                        format!("@idAsEnum argument `{enum_name}` must name an enum"),
                    )
                    .with_related(
                        symbol.module.clone(),
                        symbol.span,
                        "name is defined here",
                    ),
                );
            }
            None => {
                self.push_diag(
                    CftErrorCode::UnknownNamedType,
                    module,
                    enum_name_span,
                    format!("unknown @idAsEnum enum `{enum_name}`"),
                );
            }
        }
    }

    fn validate_defaults(&mut self) {
        self.each_type(|this, info| {
            let mut field_names = this
                .collect_ancestor_fields(
                    info.def.parent.as_ref().map(|parent| parent.name.as_str()),
                )
                .into_keys()
                .collect::<BTreeSet<_>>();
            field_names.extend(info.def.fields.iter().map(|field| field.name.clone()));
            for field in &info.def.fields {
                let Some(default) = &field.default else {
                    continue;
                };
                let field_ty = this.resolve_field_type(&field.ty);
                let default_ty = this.default_expr_type(&info.module, default, &field_names);
                if !types_assignable(&field_ty, &default_ty) {
                    this.push_diag(
                        CftErrorCode::DefaultTypeMismatch,
                        &info.module,
                        default.span,
                        "default value does not match field type",
                    );
                }
            }
        });
    }

    fn default_expr_type(
        &mut self,
        module: &ModuleId,
        expr: &DefaultExpr,
        field_names: &BTreeSet<String>,
    ) -> Ty {
        match &expr.kind {
            DefaultExprKind::Int(_) => Ty::Int,
            DefaultExprKind::Float(_) => Ty::Float,
            DefaultExprKind::Bool(_) => Ty::Bool,
            DefaultExprKind::Null => Ty::Null,
            DefaultExprKind::String(_) => Ty::String,
            DefaultExprKind::Name(name) => {
                if field_names.contains(&name.name) {
                    self.push_diag(
                        CftErrorCode::DefaultReferencesField,
                        module,
                        name.span,
                        "default value cannot reference a field",
                    );
                    return Ty::Unknown;
                }
                if let Some(info) = self.consts.get(&name.name) {
                    return Ty::from_const(&info.value);
                }
                self.push_diag(
                    CftErrorCode::UnknownConst,
                    module,
                    name.span,
                    format!("unknown const `{}`", name.name),
                );
                Ty::Unknown
            }
            DefaultExprKind::EnumVariant { enum_name, variant } => {
                self.default_enum_variant_type(module, enum_name, variant)
            }
            DefaultExprKind::Array(items) => {
                if items.is_empty() {
                    Ty::EmptyArray
                } else {
                    self.push_diag(
                        CftErrorCode::InvalidDefaultExpression,
                        module,
                        expr.span,
                        "only empty array defaults are allowed",
                    );
                    Ty::Unknown
                }
            }
            DefaultExprKind::Object(fields) => {
                if fields.is_empty() {
                    Ty::EmptyObject
                } else {
                    self.push_diag(
                        CftErrorCode::InvalidDefaultExpression,
                        module,
                        expr.span,
                        "only empty object defaults are allowed",
                    );
                    Ty::Unknown
                }
            }
        }
    }

    fn default_enum_variant_type(
        &mut self,
        module: &ModuleId,
        enum_name: &crate::ast::NameRef,
        variant: &crate::ast::NameRef,
    ) -> Ty {
        match self.symbols.get(&enum_name.name) {
            Some(symbol) if symbol.kind == SymbolKind::Enum => {
                match self.enums.get(&enum_name.name) {
                    Some(enum_info) if enum_info.variants.contains(&variant.name) => {
                        Ty::Enum(enum_name.name.clone())
                    }
                    Some(_) => {
                        self.push_diag(
                            CftErrorCode::UnknownEnumVariant,
                            module,
                            variant.span,
                            format!("unknown enum variant `{}`", variant.name),
                        );
                        Ty::Unknown
                    }
                    None => Ty::Unknown,
                }
            }
            Some(symbol) => {
                self.diagnostics.push(
                    CftDiagnostic::error(
                        CftErrorCode::EnumVariantOnNonEnum,
                        module.clone(),
                        enum_name.span,
                        "enum variant default is used on a non-enum name",
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
                    CftErrorCode::EnumVariantOnNonEnum,
                    module,
                    enum_name.span,
                    "enum variant default is used on an unknown enum",
                );
                Ty::Unknown
            }
        }
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

    fn build_schema(&self) -> CompiledSchema {
        let mut modules = self
            .container
            .modules
            .keys()
            .map(|id| {
                (
                    id.clone(),
                    CftSchemaModule {
                        consts: Vec::new(),
                        types: Vec::new(),
                        enums: Vec::new(),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        let mut consts = BTreeMap::new();
        let mut types = BTreeMap::new();
        let mut enums = BTreeMap::new();

        for (name, info) in &self.consts {
            let schema = CftSchemaConst {
                module: info.module.clone(),
                name: name.clone(),
                value: info.value.clone(),
                span: info.def.span,
            };
            if let Some(module) = modules.get_mut(&info.module) {
                module.consts.push(schema.clone());
            }
            consts.insert(name.clone(), schema);
        }

        for (name, info) in &self.enums {
            // `validate_enums` already resolved every variant's integer value
            // (auto-numbered or explicit) into `values_by_name`. We just look
            // them up here instead of re-walking the sequence.
            let variants = info
                .def
                .variants
                .iter()
                .map(|variant| CftSchemaEnumVariant {
                    name: variant.name.clone(),
                    value: info
                        .values_by_name
                        .get(&variant.name)
                        .copied()
                        .map_or(0, |value| value),
                    annotations: convert_annotations(&variant.annotations),
                    span: variant.span,
                })
                .collect::<Vec<_>>();
            let schema = CftSchemaEnum {
                module: info.module.clone(),
                name: name.clone(),
                variants,
                annotations: convert_annotations(&info.def.annotations),
                span: info.def.span,
            };
            if let Some(module) = modules.get_mut(&info.module) {
                module.enums.push(schema.clone());
            }
            enums.insert(name.clone(), schema);
        }

        for (name, info) in &self.types {
            let fields = info
                .def
                .fields
                .iter()
                .map(|field| self.build_schema_field(field, name))
                .collect();
            let all_fields = self.collect_all_schema_fields(name);
            let is_singleton = has_annotation(&info.def.annotations, "singleton");
            let schema = CftSchemaType {
                module: info.module.clone(),
                name: name.clone(),
                parent: info.def.parent.as_ref().map(|parent| parent.name.clone()),
                is_abstract: info.def.is_abstract,
                is_sealed: info.def.is_sealed,
                is_singleton,
                fields,
                all_fields,
                check: info.def.check.as_ref().map(convert_check_block),
                annotations: convert_annotations(&info.def.annotations),
                span: info.def.span,
            };
            if let Some(module) = modules.get_mut(&info.module) {
                module.types.push(schema.clone());
            }
            types.insert(name.clone(), schema);
        }

        CompiledSchema {
            modules,
            consts,
            types,
            enums,
        }
    }

    fn build_schema_field(&self, field: &FieldDef, owner_type: &str) -> CftSchemaField {
        let localized = find_annotation(&field.annotations, "localized");
        let is_localized = localized.is_some();
        let localization_bucket = localized.map(|_| owner_type.to_string());
        CftSchemaField {
            name: field.name.clone(),
            ty: format_type_ref(&field.ty),
            ty_ref: build_schema_type_ref(&field.ty),
            has_default: field.default.is_some(),
            default: field
                .default
                .as_ref()
                .and_then(|default| self.schema_default_value(default)),
            annotations: convert_annotations(&field.annotations),
            is_localized,
            localization_bucket,
            span: field.span,
        }
    }

    fn collect_all_schema_fields(&self, type_name: &str) -> Vec<CftSchemaField> {
        self.ancestry_chain(type_name)
            .into_iter()
            .flat_map(|info| {
                let owner = info.def.name.clone();
                info.def
                    .fields
                    .iter()
                    .map(|field| self.build_schema_field(field, &owner))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    fn schema_default_value(&self, expr: &DefaultExpr) -> Option<CftSchemaDefaultValue> {
        Some(match &expr.kind {
            DefaultExprKind::Null => CftSchemaDefaultValue::Null,
            DefaultExprKind::Int(value) => CftSchemaDefaultValue::Int(*value),
            DefaultExprKind::Float(value) => CftSchemaDefaultValue::Float(*value),
            DefaultExprKind::Bool(value) => CftSchemaDefaultValue::Bool(*value),
            DefaultExprKind::String(value) => CftSchemaDefaultValue::String(value.clone()),
            DefaultExprKind::Array(items) if items.is_empty() => CftSchemaDefaultValue::EmptyArray,
            DefaultExprKind::Object(fields) if fields.is_empty() => {
                CftSchemaDefaultValue::EmptyObject
            }
            DefaultExprKind::Name(name) => match self.consts.get(&name.name)?.value.clone() {
                CftConstValue::Int(value) => CftSchemaDefaultValue::Int(value),
                CftConstValue::Float(value) => CftSchemaDefaultValue::Float(value),
                CftConstValue::Bool(value) => CftSchemaDefaultValue::Bool(value),
                CftConstValue::String(value) => CftSchemaDefaultValue::String(value),
            },
            DefaultExprKind::EnumVariant { enum_name, variant } => CftSchemaDefaultValue::Enum {
                enum_name: enum_name.name.clone(),
                variant: variant.name.clone(),
                value: self.enum_variant_value(&enum_name.name, &variant.name)?,
            },
            DefaultExprKind::Array(_) | DefaultExprKind::Object(_) => return None,
        })
    }

    fn enum_variant_value(&self, enum_name: &str, variant_name: &str) -> Option<i64> {
        self.enums
            .get(enum_name)?
            .values_by_name
            .get(variant_name)
            .copied()
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
                Some(symbol) if symbol.kind == SymbolKind::Type => Ty::Type(name.clone()),
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
