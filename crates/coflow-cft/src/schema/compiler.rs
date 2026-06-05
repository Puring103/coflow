use super::support::{
    const_value, convert_annotations, convert_check_block, find_annotation, format_type_ref,
    has_annotation, is_i64_power_of_two, is_indexable_field_type, is_string_or_int,
    is_valid_dict_key, types_assignable, AnnotationSpec, AnnotationTarget, ConstInfo, EnumInfo,
    FieldInfo, FieldOrigin, Symbol, SymbolKind, Ty, TypeInfo,
};
use super::type_checker::TypeChecker;
use super::{
    CftConstValue, CftSchemaConst, CftSchemaDefaultValue, CftSchemaEnum, CftSchemaEnumVariant,
    CftSchemaField, CftSchemaModule, CftSchemaType, CompiledSchema,
};
use crate::ast::{
    Annotation, AnnotationArg, DefaultExpr, DefaultExprKind, FieldDef, Item, TypeRef, TypeRefKind,
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
                        for variant in &def.variants {
                            for annotation in &variant.annotations {
                                self.push_diag(
                                    CftErrorCode::InvalidAnnotationTarget,
                                    module_id,
                                    annotation.span,
                                    "annotations cannot be applied to enum variants",
                                );
                            }
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
                        self.insert_symbol(&def.name, SymbolKind::Const, module_id, def.name_span);
                        self.consts.insert(
                            def.name.clone(),
                            ConstInfo {
                                module: module_id.clone(),
                                def,
                                value: const_value(&def.value),
                            },
                        );
                    }
                    Item::Enum(def) => {
                        self.insert_symbol(&def.name, SymbolKind::Enum, module_id, def.name_span);
                        self.enums.insert(
                            def.name.clone(),
                            EnumInfo {
                                module: module_id.clone(),
                                def,
                                variants: BTreeSet::new(),
                                values: BTreeMap::new(),
                                is_flag: has_annotation(&def.annotations, "flag"),
                            },
                        );
                    }
                    Item::Type(def) => {
                        self.insert_symbol(&def.name, SymbolKind::Type, module_id, def.name_span);
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

    fn insert_symbol(&mut self, name: &str, kind: SymbolKind, module_id: &ModuleId, span: Span) {
        if let Some(first) = self.symbols.get(name) {
            let diagnostic = CftDiagnostic::error(
                CftErrorCode::DuplicateGlobalName,
                module_id.clone(),
                span,
                format!("duplicate global name `{name}`"),
            )
            .with_related(first.module.clone(), first.span, "first definition is here");
            self.diagnostics.push(diagnostic);
        } else {
            self.symbols.insert(
                name.to_string(),
                Symbol {
                    kind,
                    module: module_id.clone(),
                    span,
                },
            );
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
            for (index, variant) in info.def.variants.iter().enumerate() {
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
                let value = if let Some(value) = &variant.value {
                    value.value
                } else {
                    next
                };
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
            }
            if let Some(stored) = self.enums.get_mut(&name) {
                stored.variants = variants;
                stored.values = values
                    .into_iter()
                    .map(|(value, (_, module, span))| (value, (module, span)))
                    .collect();
            }
        }
    }

    fn validate_type_headers(&mut self) {
        let infos = self.types.values().cloned().collect::<Vec<_>>();
        for info in infos {
            if info.def.is_abstract && info.def.is_sealed {
                let span = info
                    .def
                    .abstract_span
                    .unwrap_or(info.def.span)
                    .join(info.def.sealed_span.unwrap_or(info.def.span));
                self.push_diag(
                    CftErrorCode::ConflictingTypeModifiers,
                    &info.module,
                    span,
                    "abstract and sealed modifiers cannot be combined",
                );
            }
            if let Some(parent) = &info.def.parent {
                match self.symbols.get(&parent.name) {
                    Some(symbol) if symbol.kind == SymbolKind::Type => {}
                    Some(symbol) => {
                        self.diagnostics.push(
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
                        self.push_diag(
                            CftErrorCode::UnknownNamedType,
                            &info.module,
                            parent.span,
                            format!("unknown parent type `{}`", parent.name),
                        );
                    }
                }
            }
        }
    }

    fn validate_field_shapes(&mut self) {
        let infos = self.types.values().cloned().collect::<Vec<_>>();
        for info in infos {
            let mut fields: BTreeMap<String, Span> = BTreeMap::new();
            for field in &info.def.fields {
                if let Some(first_span) = fields.get(&field.name) {
                    self.diagnostics.push(
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
                self.resolve_field_type(&info.module, &field.ty);
            }
        }
    }

    fn validate_inheritance(&mut self) {
        let names = self.types.keys().cloned().collect::<Vec<_>>();
        let mut visiting = HashSet::new();
        let mut visited = HashSet::new();
        for name in &names {
            self.detect_cycle(name, &mut visiting, &mut visited, &mut Vec::new());
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
                    let inherited = self.collect_ancestor_fields(&parent.name);
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

        self.validate_id_fields_by_tree();
    }

    fn detect_cycle(
        &mut self,
        name: &str,
        visiting: &mut HashSet<String>,
        visited: &mut HashSet<String>,
        stack: &mut Vec<String>,
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
        stack.push(name.to_string());
        if let Some(parent) = self
            .types
            .get(name)
            .and_then(|info| info.def.parent.as_ref())
            .map(|parent| parent.name.clone())
        {
            if self.types.contains_key(&parent) {
                self.detect_cycle(&parent, visiting, visited, stack);
            }
        }
        stack.pop();
        visiting.remove(name);
        visited.insert(name.to_string());
    }

    fn validate_id_fields_by_tree(&mut self) {
        let names = self.types.keys().cloned().collect::<Vec<_>>();
        let mut first_by_root: BTreeMap<String, (ModuleId, Span)> = BTreeMap::new();
        for name in names {
            let Some(info) = self.types.get(&name).cloned() else {
                continue;
            };
            let root = self.root_type_name(&name);
            for field in &info.def.fields {
                if !has_annotation(&field.annotations, "id") {
                    continue;
                }
                if let Some(first) = first_by_root.get(&root) {
                    self.diagnostics.push(
                        CftDiagnostic::error(
                            CftErrorCode::MultipleIdFieldsInTree,
                            info.module.clone(),
                            field.name_span,
                            "inheritance tree already has an @id field",
                        )
                        .with_related(
                            first.0.clone(),
                            first.1,
                            "first @id field is here",
                        ),
                    );
                } else {
                    first_by_root.insert(root.clone(), (info.module.clone(), field.name_span));
                }
            }
        }
    }

    fn validate_annotations(&mut self) {
        let enum_infos = self.enums.values().cloned().collect::<Vec<_>>();
        for info in enum_infos {
            self.validate_annotation_list(
                &info.module,
                AnnotationTarget::Enum,
                &info.def.annotations,
            );
            if has_annotation(&info.def.annotations, "struct") {
                if let Some(annotation) = find_annotation(&info.def.annotations, "struct") {
                    self.push_diag(
                        CftErrorCode::InvalidAnnotationTarget,
                        &info.module,
                        annotation.span,
                        "@struct can only be applied to types",
                    );
                }
            }
        }

        let type_infos = self.types.values().cloned().collect::<Vec<_>>();
        for info in type_infos {
            self.validate_annotation_list(
                &info.module,
                AnnotationTarget::Type,
                &info.def.annotations,
            );
            if let Some(annotation) = find_annotation(&info.def.annotations, "struct") {
                if !info.def.is_sealed {
                    self.push_diag(
                        CftErrorCode::StructRequiresSealedType,
                        &info.module,
                        annotation.span,
                        "@struct requires a sealed type",
                    );
                }
            }
            for field in &info.def.fields {
                self.validate_annotation_list(
                    &info.module,
                    AnnotationTarget::Field,
                    &field.annotations,
                );
                self.validate_field_annotations(&info.module, field);
            }
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
                self.push_diag(
                    CftErrorCode::InvalidAnnotationTarget,
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

    fn validate_field_annotations(&mut self, module: &ModuleId, field: &FieldDef) {
        if let Some(annotation) = find_annotation(&field.annotations, "id") {
            if !is_string_or_int(&self.resolve_field_type(module, &field.ty), false) {
                self.push_diag(
                    CftErrorCode::InvalidAnnotatedFieldType,
                    module,
                    annotation.span,
                    "@id fields must be string or int",
                );
            }
        }
        if let Some(annotation) = find_annotation(&field.annotations, "ref") {
            if !is_string_or_int(&self.resolve_field_type(module, &field.ty), true) {
                self.push_diag(
                    CftErrorCode::InvalidAnnotatedFieldType,
                    module,
                    annotation.span,
                    "@ref fields must be string or int, optionally nullable",
                );
            }
            if let Some(AnnotationArg::Name(target)) = annotation.args.first() {
                match self.symbols.get(&target.name) {
                    Some(symbol) if symbol.kind == SymbolKind::Type => {}
                    Some(symbol) => {
                        self.diagnostics.push(
                            CftDiagnostic::error(
                                CftErrorCode::RefTargetMustBeType,
                                module.clone(),
                                target.span,
                                "@ref target must be a type",
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
                            CftErrorCode::RefTargetMustBeType,
                            module,
                            target.span,
                            "@ref target must be a known type",
                        );
                    }
                }
            }
        }
        if let Some(annotation) = find_annotation(&field.annotations, "index") {
            if !is_indexable_field_type(&self.resolve_field_type(module, &field.ty)) {
                self.push_diag(
                    CftErrorCode::InvalidAnnotatedFieldType,
                    module,
                    annotation.span,
                    "@index fields must be string, int, or enum, optionally nullable",
                );
            }
        }
    }

    fn validate_defaults(&mut self) {
        let type_infos = self.types.values().cloned().collect::<Vec<_>>();
        for info in type_infos {
            let mut field_names = self
                .collect_ancestor_fields(
                    info.def
                        .parent
                        .as_ref()
                        .map_or("", |parent| parent.name.as_str()),
                )
                .into_keys()
                .collect::<BTreeSet<_>>();
            field_names.extend(info.def.fields.iter().map(|field| field.name.clone()));
            for field in &info.def.fields {
                let Some(default) = &field.default else {
                    continue;
                };
                let field_ty = self.resolve_field_type(&info.module, &field.ty);
                let default_ty = self.default_expr_type(&info.module, default, &field_names);
                if !types_assignable(&field_ty, &default_ty) {
                    self.push_diag(
                        CftErrorCode::DefaultTypeMismatch,
                        &info.module,
                        default.span,
                        "default value does not match field type",
                    );
                }
            }
        }
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
                match self.symbols.get(&enum_name.name) {
                    Some(symbol) if symbol.kind == SymbolKind::Enum => {
                        if let Some(enum_info) = self.enums.get(&enum_name.name) {
                            if enum_info.variants.contains(&variant.name) {
                                Ty::Enum(enum_name.name.clone())
                            } else {
                                self.push_diag(
                                    CftErrorCode::UnknownEnumVariant,
                                    module,
                                    variant.span,
                                    format!("unknown enum variant `{}`", variant.name),
                                );
                                Ty::Unknown
                            }
                        } else {
                            Ty::Unknown
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

    fn build_full_fields(&mut self) {
        let names = self.types.keys().cloned().collect::<Vec<_>>();
        for name in names {
            let mut map = BTreeMap::new();
            self.fill_fields(&name, &mut map, &mut HashSet::new());
            self.full_fields.insert(name, map);
        }
    }

    fn fill_fields(
        &mut self,
        type_name: &str,
        out: &mut BTreeMap<String, FieldInfo>,
        seen: &mut HashSet<String>,
    ) {
        if !seen.insert(type_name.to_string()) {
            return;
        }
        let Some(info) = self.types.get(type_name).cloned() else {
            return;
        };
        if let Some(parent) = &info.def.parent {
            self.fill_fields(&parent.name, out, seen);
        }
        for field in &info.def.fields {
            let declared_ty = self.resolve_field_type(&info.module, &field.ty);
            let check_ty = self.check_type_for_field(&info.module, field, &declared_ty);
            out.insert(field.name.clone(), FieldInfo { check_ty });
        }
    }

    fn validate_checks(&mut self) {
        let infos = self.types.values().cloned().collect::<Vec<_>>();
        for info in infos {
            if let Some(check) = &info.def.check {
                let mut checker = TypeChecker::new(self, &info);
                checker.check_stmts(&check.stmts);
            }
        }
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
            let mut next = 0_i64;
            let variants = info
                .def
                .variants
                .iter()
                .map(|variant| {
                    let value = variant.value.as_ref().map_or(next, |value| value.value);
                    next = value.saturating_add(1);
                    CftSchemaEnumVariant {
                        name: variant.name.clone(),
                        value,
                        annotations: Vec::new(),
                        span: variant.span,
                    }
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
                .map(|field| CftSchemaField {
                    name: field.name.clone(),
                    ty: format_type_ref(&field.ty),
                    has_default: field.default.is_some(),
                    default: field
                        .default
                        .as_ref()
                        .and_then(|default| self.schema_default_value(default)),
                    annotations: convert_annotations(&field.annotations),
                    span: field.span,
                })
                .collect();
            let schema = CftSchemaType {
                module: info.module.clone(),
                name: name.clone(),
                parent: info.def.parent.as_ref().map(|parent| parent.name.clone()),
                is_abstract: info.def.is_abstract,
                is_sealed: info.def.is_sealed,
                fields,
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
        let info = self.enums.get(enum_name)?;
        let mut next = 0_i64;
        for variant in &info.def.variants {
            let value = variant.value.as_ref().map_or(next, |value| value.value);
            if variant.name == variant_name {
                return Some(value);
            }
            next = value.saturating_add(1);
        }
        None
    }

    fn resolve_field_type(&mut self, module: &ModuleId, ty: &TypeRef) -> Ty {
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
                let inner = self.resolve_field_type(module, inner);
                Ty::Array(Box::new(inner))
            }
            TypeRefKind::Dict(key, value) => {
                let key_ty = self.resolve_field_type(module, key);
                if !is_valid_dict_key(&key_ty) {
                    self.push_diag(
                        CftErrorCode::InvalidDictKeyType,
                        module,
                        key.span,
                        "dict key type must be string, int, or enum",
                    );
                }
                let value_ty = self.resolve_field_type(module, value);
                Ty::Dict(Box::new(key_ty), Box::new(value_ty))
            }
            TypeRefKind::Nullable(inner) => {
                let inner = self.resolve_field_type(module, inner);
                Ty::Nullable(Box::new(inner))
            }
        }
    }

    fn check_type_for_field(&mut self, module: &ModuleId, field: &FieldDef, declared: &Ty) -> Ty {
        if let Some(annotation) = find_annotation(&field.annotations, "ref") {
            if let Some(AnnotationArg::Name(target)) = annotation.args.first() {
                let target_ty = Ty::Type(target.name.clone());
                return if declared.is_nullable() {
                    Ty::Nullable(Box::new(target_ty))
                } else {
                    target_ty
                };
            }
            self.push_diag(
                CftErrorCode::InvalidAnnotationArgument,
                module,
                annotation.span,
                "@ref requires a type-name argument",
            );
        }
        declared.clone()
    }

    fn collect_ancestor_fields(&self, parent_name: &str) -> BTreeMap<String, FieldOrigin> {
        let mut out = BTreeMap::new();
        let mut current = Some(parent_name.to_string());
        let mut seen = HashSet::new();
        while let Some(name) = current {
            if !seen.insert(name.clone()) {
                break;
            }
            let Some(info) = self.types.get(&name) else {
                break;
            };
            for field in &info.def.fields {
                out.entry(field.name.clone()).or_insert(FieldOrigin {
                    module: info.module.clone(),
                    span: field.name_span,
                });
            }
            current = info.def.parent.as_ref().map(|parent| parent.name.clone());
        }
        out
    }

    fn root_type_name(&self, name: &str) -> String {
        let mut current = name.to_string();
        let mut seen = HashSet::new();
        while seen.insert(current.clone()) {
            let Some(parent) = self
                .types
                .get(&current)
                .and_then(|info| info.def.parent.as_ref())
                .map(|parent| parent.name.clone())
            else {
                break;
            };
            current = parent;
        }
        current
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
}
