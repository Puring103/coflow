use crate::ast::{
    Annotation, AnnotationArg, BinOp, CheckExpr, CheckExprKind, CheckStmt, CmpOp, ConstLiteral,
    DefaultExpr, DefaultExprKind, EnumDef, FieldDef, Item, NameRef, TypeDef, TypePredicate,
    TypeRef, TypeRefKind, UnaryOp,
};
use crate::container::{CftContainer, ModuleId};
use crate::error::{CftDiagnostic, CftDiagnostics, CftErrorCode};
use crate::span::Span;
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

#[derive(Debug, Clone, PartialEq)]
pub struct CftSchemaModule {
    pub consts: Vec<CftSchemaConst>,
    pub types: Vec<CftSchemaType>,
    pub enums: Vec<CftSchemaEnum>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CftSchemaConst {
    pub module: ModuleId,
    pub name: String,
    pub value: CftConstValue,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CftConstValue {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CftSchemaType {
    pub module: ModuleId,
    pub name: String,
    pub parent: Option<String>,
    pub is_abstract: bool,
    pub is_sealed: bool,
    pub fields: Vec<CftSchemaField>,
    pub check: Option<CftSchemaCheckBlock>,
    pub annotations: Vec<CftAnnotation>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CftSchemaField {
    pub name: String,
    pub ty: String,
    pub has_default: bool,
    pub default: Option<CftSchemaDefaultValue>,
    pub annotations: Vec<CftAnnotation>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CftSchemaDefaultValue {
    Null,
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    Enum {
        enum_name: String,
        variant: String,
        value: i64,
    },
    EmptyArray,
    EmptyObject,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CftSchemaCheckBlock {
    pub stmts: Vec<CftSchemaCheckStmt>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CftSchemaCheckStmt {
    Expr(CftSchemaCheckExpr),
    Quantifier {
        kind: CftSchemaQuantifierKind,
        binding: String,
        collection: CftSchemaCheckExpr,
        body: Vec<CftSchemaCheckStmt>,
        span: Span,
    },
    When {
        condition: CftSchemaCheckExpr,
        body: Vec<CftSchemaCheckStmt>,
        span: Span,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct CftSchemaCheckExpr {
    pub kind: CftSchemaCheckExprKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CftSchemaCheckExprKind {
    Int(i64),
    Float(f64),
    Bool(bool),
    Null,
    String(String),
    Name(String),
    Field {
        expr: Box<CftSchemaCheckExpr>,
        name: String,
    },
    Index {
        expr: Box<CftSchemaCheckExpr>,
        index: Box<CftSchemaCheckExpr>,
    },
    Is {
        expr: Box<CftSchemaCheckExpr>,
        predicate: CftSchemaTypePredicate,
    },
    Call {
        name: String,
        args: Vec<CftSchemaCheckExpr>,
    },
    BinOp {
        op: CftSchemaBinOp,
        lhs: Box<CftSchemaCheckExpr>,
        rhs: Box<CftSchemaCheckExpr>,
    },
    Unary {
        op: CftSchemaUnaryOp,
        expr: Box<CftSchemaCheckExpr>,
    },
    CmpChain {
        first: Box<CftSchemaCheckExpr>,
        rest: Vec<(CftSchemaCmpOp, CftSchemaCheckExpr)>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CftSchemaTypePredicate {
    Type(String),
    Null,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CftSchemaQuantifierKind {
    All,
    Any,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CftSchemaBinOp {
    Or,
    And,
    BitOr,
    BitXor,
    BitAnd,
    Add,
    Sub,
    Shl,
    Shr,
    Mul,
    Div,
    IntDiv,
    Mod,
    Pow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CftSchemaUnaryOp {
    Not,
    BitNot,
    Neg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CftSchemaCmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CftSchemaEnum {
    pub module: ModuleId,
    pub name: String,
    pub variants: Vec<CftSchemaEnumVariant>,
    pub annotations: Vec<CftAnnotation>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CftSchemaEnumVariant {
    pub name: String,
    pub value: i64,
    pub annotations: Vec<CftAnnotation>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CftAnnotation {
    pub name: String,
    pub args: Vec<CftAnnotationValue>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CftAnnotationValue {
    Name(String),
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Null,
}

#[derive(Debug, Clone)]
pub(crate) struct CompiledSchema {
    pub(crate) modules: BTreeMap<ModuleId, CftSchemaModule>,
    pub(crate) consts: BTreeMap<String, CftSchemaConst>,
    pub(crate) types: BTreeMap<String, CftSchemaType>,
    pub(crate) enums: BTreeMap<String, CftSchemaEnum>,
}

pub(crate) fn compile_container(
    container: &CftContainer,
) -> Result<CompiledSchema, CftDiagnostics> {
    let mut compiler = SchemaCompiler::new(container);
    compiler.compile()
}

struct SchemaCompiler<'a> {
    container: &'a CftContainer,
    diagnostics: Vec<CftDiagnostic>,
    symbols: BTreeMap<String, Symbol>,
    consts: BTreeMap<String, ConstInfo<'a>>,
    types: BTreeMap<String, TypeInfo<'a>>,
    enums: BTreeMap<String, EnumInfo<'a>>,
    full_fields: BTreeMap<String, BTreeMap<String, FieldInfo>>,
}

impl<'a> SchemaCompiler<'a> {
    fn new(container: &'a CftContainer) -> Self {
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

    fn compile(&mut self) -> Result<CompiledSchema, CftDiagnostics> {
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
            for variant in &info.def.variants {
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
                next = if let Some(value) = value.checked_add(1) {
                    value
                } else {
                    self.push_diag(
                        CftErrorCode::InvalidEnumValueSequence,
                        &info.module,
                        variant.span,
                        "enum auto numbering overflowed",
                    );
                    value
                };
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
            let local_fields = info
                .def
                .fields
                .iter()
                .map(|field| field.name.as_str())
                .collect::<BTreeSet<_>>();
            for field in &info.def.fields {
                let Some(default) = &field.default else {
                    continue;
                };
                let field_ty = self.resolve_field_type(&info.module, &field.ty);
                let default_ty = self.default_expr_type(&info.module, default, &local_fields);
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
        local_fields: &BTreeSet<&str>,
    ) -> Ty {
        match &expr.kind {
            DefaultExprKind::Int(_) => Ty::Int,
            DefaultExprKind::Float(_) => Ty::Float,
            DefaultExprKind::Bool(_) => Ty::Bool,
            DefaultExprKind::Null => Ty::Null,
            DefaultExprKind::String(_) => Ty::String,
            DefaultExprKind::Name(name) => {
                if local_fields.contains(name.name.as_str()) {
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

struct TypeChecker<'a, 'b> {
    compiler: &'a mut SchemaCompiler<'b>,
    type_info: &'a TypeInfo<'b>,
    locals: Vec<HashMap<String, Ty>>,
}

impl<'a, 'b> TypeChecker<'a, 'b> {
    fn new(compiler: &'a mut SchemaCompiler<'b>, type_info: &'a TypeInfo<'b>) -> Self {
        Self {
            compiler,
            type_info,
            locals: Vec::new(),
        }
    }

    fn check_stmts(&mut self, stmts: &[CheckStmt]) {
        for stmt in stmts {
            self.check_stmt(stmt);
        }
    }

    fn check_stmt(&mut self, stmt: &CheckStmt) {
        match stmt {
            CheckStmt::Expr(expr) => {
                let ty = self.check_expr(expr);
                self.expect_bool(&ty, expr.span);
            }
            CheckStmt::When {
                condition, body, ..
            } => {
                let ty = self.check_expr(condition);
                self.expect_bool(&ty, condition.span);
                self.check_stmts(body);
            }
            CheckStmt::Quantifier {
                binding,
                collection,
                body,
                span,
                ..
            } => {
                let col_ty = self.check_expr(collection);
                let item_ty = match unwrap_nullable(&col_ty) {
                    Ty::Array(inner) => *inner.clone(),
                    Ty::Dict(key, value) => Ty::Entry(key.clone(), value.clone()),
                    Ty::Unknown => Ty::Unknown,
                    _ => {
                        self.diag(
                            CftErrorCode::QuantifierRequiresCollection,
                            *span,
                            "quantifier target must be an array or dict",
                        );
                        Ty::Unknown
                    }
                };
                self.locals
                    .push(HashMap::from([(binding.name.clone(), item_ty)]));
                self.check_stmts(body);
                self.locals.pop();
            }
        }
    }

    fn check_expr(&mut self, expr: &CheckExpr) -> Ty {
        match &expr.kind {
            CheckExprKind::Int(_) => Ty::Int,
            CheckExprKind::Float(_) => Ty::Float,
            CheckExprKind::Bool(_) => Ty::Bool,
            CheckExprKind::Null => Ty::Null,
            CheckExprKind::String(_) => Ty::String,
            CheckExprKind::Name(name) => self.resolve_value_name(name, expr.span),
            CheckExprKind::Unary { op, expr: inner } => {
                let ty = self.check_expr(inner);
                self.check_unary(*op, &ty, expr.span)
            }
            CheckExprKind::BinOp { op, lhs, rhs } => {
                let lhs_ty = self.check_expr(lhs);
                let rhs_ty = self.check_expr(rhs);
                self.check_binop(*op, &lhs_ty, &rhs_ty, expr.span)
            }
            CheckExprKind::CmpChain { first, rest } => {
                let mut lhs_ty = self.check_expr(first);
                for (op, rhs) in rest {
                    let rhs_ty = self.check_expr(rhs);
                    self.check_comparison(*op, &lhs_ty, &rhs_ty, rhs.span);
                    lhs_ty = rhs_ty;
                }
                Ty::Bool
            }
            CheckExprKind::Field { expr: inner, name } => self.check_field(inner, name, expr.span),
            CheckExprKind::Index { expr: inner, index } => {
                self.check_index(inner, index, expr.span)
            }
            CheckExprKind::Is {
                expr: inner,
                predicate,
            } => {
                let _ = self.check_expr(inner);
                self.check_is(predicate);
                Ty::Bool
            }
            CheckExprKind::Call { name, args } => self.check_call(name, args, expr.span),
        }
    }

    fn resolve_value_name(&mut self, name: &str, span: Span) -> Ty {
        for scope in self.locals.iter().rev() {
            if let Some(ty) = scope.get(name) {
                return ty.clone();
            }
        }
        if let Some(fields) = self.compiler.full_fields.get(&self.type_info.def.name) {
            if let Some(field) = fields.get(name) {
                return field.check_ty.clone();
            }
        }
        if let Some(info) = self.compiler.consts.get(name) {
            return Ty::from_const(&info.value);
        }
        if self.compiler.enums.contains_key(name) {
            return Ty::EnumNamespace(name.to_string());
        }
        self.diag(
            CftErrorCode::UnknownValueName,
            span,
            format!("unknown value `{name}`"),
        );
        Ty::Unknown
    }

    fn check_field(&mut self, inner: &CheckExpr, name: &NameRef, span: Span) -> Ty {
        if let CheckExprKind::Name(enum_name) = &inner.kind {
            if let Some(enum_info) = self.compiler.enums.get(enum_name) {
                if enum_info.variants.contains(&name.name) {
                    return Ty::Enum(enum_name.clone());
                }
                self.diag(
                    CftErrorCode::TypeUnknownEnumVariant,
                    name.span,
                    format!("unknown enum variant `{}`", name.name),
                );
                return Ty::Unknown;
            }
            if let Some(symbol) = self.compiler.symbols.get(enum_name) {
                if symbol.kind != SymbolKind::Enum && self.compiler.symbols.contains_key(enum_name)
                {
                    self.diag(
                        CftErrorCode::TypeEnumVariantOnNonEnum,
                        inner.span,
                        "enum variant access used on a non-enum name",
                    );
                    return Ty::Unknown;
                }
            }
        }

        let inner_ty = self.check_expr(inner);
        match unwrap_nullable(&inner_ty) {
            Ty::Type(type_name) => {
                if let Some(fields) = self.compiler.full_fields.get(type_name) {
                    if let Some(field) = fields.get(&name.name) {
                        field.check_ty.clone()
                    } else {
                        self.diag(
                            CftErrorCode::UnknownField,
                            name.span,
                            format!("unknown field `{}`", name.name),
                        );
                        Ty::Unknown
                    }
                } else {
                    Ty::Unknown
                }
            }
            Ty::Entry(key, value) => match name.name.as_str() {
                "key" => *key.clone(),
                "value" => *value.clone(),
                _ => {
                    self.diag(
                        CftErrorCode::UnknownField,
                        name.span,
                        "dict entry only has key and value fields",
                    );
                    Ty::Unknown
                }
            },
            Ty::Unknown => Ty::Unknown,
            _ => {
                self.diag(
                    CftErrorCode::FieldAccessOnNonObject,
                    span,
                    "field access requires an object",
                );
                Ty::Unknown
            }
        }
    }

    fn check_index(&mut self, inner: &CheckExpr, index: &CheckExpr, span: Span) -> Ty {
        let inner_ty = self.check_expr(inner);
        let index_ty = self.check_expr(index);
        match unwrap_nullable(&inner_ty) {
            Ty::Array(elem) => {
                if !same_type(&index_ty, &Ty::Int) && index_ty != Ty::Unknown {
                    self.diag(
                        CftErrorCode::IndexTypeMismatch,
                        index.span,
                        "array index must be int",
                    );
                }
                *elem.clone()
            }
            Ty::Dict(key, value) => {
                if !types_comparable(key, &index_ty) && index_ty != Ty::Unknown {
                    self.diag(
                        CftErrorCode::IndexTypeMismatch,
                        index.span,
                        "dict index type does not match key type",
                    );
                }
                *value.clone()
            }
            Ty::Unknown => Ty::Unknown,
            _ => {
                self.diag(
                    CftErrorCode::IndexOnNonIndexable,
                    span,
                    "index access requires an array or dict",
                );
                Ty::Unknown
            }
        }
    }

    fn check_is(&mut self, predicate: &TypePredicate) {
        if let TypePredicate::Type(name) = predicate {
            match self.compiler.symbols.get(&name.name) {
                Some(symbol) if symbol.kind == SymbolKind::Type => {}
                _ => self.diag(
                    CftErrorCode::InvalidIsPredicate,
                    name.span,
                    "is predicate must name a type or null",
                ),
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    fn check_call(&mut self, name: &NameRef, args: &[CheckExpr], span: Span) -> Ty {
        if self.compiler.enums.contains_key(&name.name) {
            if args.len() != 1 {
                self.diag(
                    CftErrorCode::FunctionArityMismatch,
                    span,
                    "enum constructor expects one argument",
                );
                return Ty::Unknown;
            }
            let arg_ty = self.check_expr(&args[0]);
            if !same_type(&arg_ty, &Ty::Int) && arg_ty != Ty::Unknown {
                self.diag(
                    CftErrorCode::FunctionArgTypeMismatch,
                    args[0].span,
                    "enum constructor argument must be int",
                );
            }
            return Ty::Enum(name.name.clone());
        }

        match name.name.as_str() {
            "len" => {
                if self.expect_arity(args, 1, span).is_err() {
                    return Ty::Unknown;
                }
                let ty = self.check_expr(&args[0]);
                if !matches!(
                    unwrap_nullable(&ty),
                    Ty::Array(_) | Ty::Dict(_, _) | Ty::Unknown
                ) {
                    self.diag(
                        CftErrorCode::FunctionArgTypeMismatch,
                        args[0].span,
                        "len expects an array or dict",
                    );
                }
                Ty::Int
            }
            "contains" => {
                if self.expect_arity(args, 2, span).is_err() {
                    return Ty::Bool;
                }
                let col_ty = self.check_expr(&args[0]);
                let value_ty = self.check_expr(&args[1]);
                match unwrap_nullable(&col_ty) {
                    Ty::Array(elem) => {
                        if !types_comparable(elem, &value_ty) && value_ty != Ty::Unknown {
                            self.diag(
                                CftErrorCode::FunctionArgTypeMismatch,
                                args[1].span,
                                "contains value type does not match array element type",
                            );
                        }
                    }
                    Ty::Dict(key, _) => {
                        if !types_comparable(key, &value_ty) && value_ty != Ty::Unknown {
                            self.diag(
                                CftErrorCode::FunctionArgTypeMismatch,
                                args[1].span,
                                "contains value type does not match dict key type",
                            );
                        }
                    }
                    Ty::Unknown => {}
                    _ => self.diag(
                        CftErrorCode::FunctionArgTypeMismatch,
                        args[0].span,
                        "contains expects an array or dict",
                    ),
                }
                Ty::Bool
            }
            "unique" => {
                if self.expect_arity(args, 1, span).is_err() {
                    return Ty::Bool;
                }
                let ty = self.check_expr(&args[0]);
                match unwrap_nullable(&ty) {
                    Ty::Array(elem) if unique_supported(elem) => {}
                    Ty::Array(_) => self.diag(
                        CftErrorCode::UniqueUnsupportedElementType,
                        args[0].span,
                        "unique does not support this element type",
                    ),
                    Ty::Unknown => {}
                    _ => self.diag(
                        CftErrorCode::FunctionArgTypeMismatch,
                        args[0].span,
                        "unique expects an array",
                    ),
                }
                Ty::Bool
            }
            "min" | "max" => {
                if self.expect_arity(args, 1, span).is_err() {
                    return Ty::Unknown;
                }
                let ty = self.check_expr(&args[0]);
                match unwrap_nullable(&ty) {
                    Ty::Array(elem) if min_max_supported(elem) => *elem.clone(),
                    Ty::Array(_) => {
                        self.diag(
                            CftErrorCode::FunctionArgTypeMismatch,
                            args[0].span,
                            "min/max expects int, float, or enum arrays",
                        );
                        Ty::Unknown
                    }
                    Ty::Unknown => Ty::Unknown,
                    _ => {
                        self.diag(
                            CftErrorCode::FunctionArgTypeMismatch,
                            args[0].span,
                            "min/max expects an array",
                        );
                        Ty::Unknown
                    }
                }
            }
            "sum" => {
                if self.expect_arity(args, 1, span).is_err() {
                    return Ty::Unknown;
                }
                let ty = self.check_expr(&args[0]);
                match unwrap_nullable(&ty) {
                    Ty::Array(elem) if matches!(elem.as_ref(), Ty::Int | Ty::Float) => {
                        *elem.clone()
                    }
                    Ty::Array(_) => {
                        self.diag(
                            CftErrorCode::FunctionArgTypeMismatch,
                            args[0].span,
                            "sum expects an int or float array",
                        );
                        Ty::Unknown
                    }
                    Ty::Unknown => Ty::Unknown,
                    _ => {
                        self.diag(
                            CftErrorCode::FunctionArgTypeMismatch,
                            args[0].span,
                            "sum expects an array",
                        );
                        Ty::Unknown
                    }
                }
            }
            "keys" => {
                if self.expect_arity(args, 1, span).is_err() {
                    return Ty::Unknown;
                }
                let ty = self.check_expr(&args[0]);
                match unwrap_nullable(&ty) {
                    Ty::Dict(key, _) => Ty::Array(key.clone()),
                    Ty::Unknown => Ty::Unknown,
                    _ => {
                        self.diag(
                            CftErrorCode::FunctionArgTypeMismatch,
                            args[0].span,
                            "keys expects a dict",
                        );
                        Ty::Unknown
                    }
                }
            }
            "values" => {
                if self.expect_arity(args, 1, span).is_err() {
                    return Ty::Unknown;
                }
                let ty = self.check_expr(&args[0]);
                match unwrap_nullable(&ty) {
                    Ty::Dict(_, value) => Ty::Array(value.clone()),
                    Ty::Unknown => Ty::Unknown,
                    _ => {
                        self.diag(
                            CftErrorCode::FunctionArgTypeMismatch,
                            args[0].span,
                            "values expects a dict",
                        );
                        Ty::Unknown
                    }
                }
            }
            "matches" => {
                if self.expect_arity(args, 2, span).is_err() {
                    return Ty::Bool;
                }
                let str_ty = self.check_expr(&args[0]);
                if !same_type(&str_ty, &Ty::String) && str_ty != Ty::Unknown {
                    self.diag(
                        CftErrorCode::FunctionArgTypeMismatch,
                        args[0].span,
                        "matches first argument must be string",
                    );
                }
                if let CheckExprKind::String(pattern) = &args[1].kind {
                    if Regex::new(pattern).is_err() {
                        self.diag(
                            CftErrorCode::InvalidRegexPattern,
                            args[1].span,
                            "regex pattern cannot be compiled",
                        );
                    }
                } else {
                    let _ = self.check_expr(&args[1]);
                    self.diag(
                        CftErrorCode::RegexPatternMustBeLiteral,
                        args[1].span,
                        "matches pattern must be a string literal",
                    );
                }
                Ty::Bool
            }
            _ => {
                self.diag(
                    CftErrorCode::UnknownFunction,
                    name.span,
                    format!("unknown function `{}`", name.name),
                );
                for arg in args {
                    let _ = self.check_expr(arg);
                }
                Ty::Unknown
            }
        }
    }

    fn check_unary(&mut self, op: UnaryOp, ty: &Ty, span: Span) -> Ty {
        match op {
            UnaryOp::Not if same_type(ty, &Ty::Bool) => Ty::Bool,
            UnaryOp::Neg | UnaryOp::BitNot if same_type(ty, &Ty::Int) => Ty::Int,
            UnaryOp::Neg if same_type(ty, &Ty::Float) => Ty::Float,
            UnaryOp::BitNot if self.is_flag_enum(ty) => ty.clone(),
            UnaryOp::BitNot => {
                self.diag(
                    CftErrorCode::BitwiseRequiresIntOrFlagEnum,
                    span,
                    "bitwise not requires int or flag enum",
                );
                Ty::Unknown
            }
            _ if *ty == Ty::Unknown => Ty::Unknown,
            _ => {
                self.diag(
                    CftErrorCode::OperatorTypeMismatch,
                    span,
                    "unary operator does not support this operand type",
                );
                Ty::Unknown
            }
        }
    }

    fn check_binop(&mut self, op: BinOp, lhs: &Ty, rhs: &Ty, span: Span) -> Ty {
        match op {
            BinOp::Or | BinOp::And => {
                if (!same_type(lhs, &Ty::Bool) || !same_type(rhs, &Ty::Bool))
                    && *lhs != Ty::Unknown
                    && *rhs != Ty::Unknown
                {
                    self.diag(
                        CftErrorCode::OperatorTypeMismatch,
                        span,
                        "logical operators require bool operands",
                    );
                }
                Ty::Bool
            }
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Pow => {
                if same_type(lhs, &Ty::Int) && same_type(rhs, &Ty::Int) {
                    Ty::Int
                } else if same_type(lhs, &Ty::Float) && same_type(rhs, &Ty::Float) {
                    Ty::Float
                } else {
                    self.operator_mismatch(lhs, rhs, span);
                    Ty::Unknown
                }
            }
            BinOp::IntDiv | BinOp::Mod => {
                if same_type(lhs, &Ty::Int) && same_type(rhs, &Ty::Int) {
                    Ty::Int
                } else {
                    self.operator_mismatch(lhs, rhs, span);
                    Ty::Unknown
                }
            }
            BinOp::Shl | BinOp::Shr => {
                if same_type(lhs, &Ty::Int) && same_type(rhs, &Ty::Int) {
                    Ty::Int
                } else {
                    self.diag(
                        CftErrorCode::ShiftRequiresInt,
                        span,
                        "shift operators require int operands",
                    );
                    Ty::Unknown
                }
            }
            BinOp::BitOr | BinOp::BitXor | BinOp::BitAnd => {
                if same_type(lhs, &Ty::Int) && same_type(rhs, &Ty::Int) {
                    Ty::Int
                } else if same_type(lhs, rhs) && self.is_flag_enum(lhs) {
                    lhs.clone()
                } else {
                    self.diag(
                        CftErrorCode::BitwiseRequiresIntOrFlagEnum,
                        span,
                        "bitwise operators require int or the same flag enum",
                    );
                    Ty::Unknown
                }
            }
        }
    }

    fn check_comparison(&mut self, op: CmpOp, lhs: &Ty, rhs: &Ty, span: Span) -> Ty {
        let ok = match op {
            CmpOp::Eq | CmpOp::Ne => types_comparable(lhs, rhs),
            CmpOp::Lt | CmpOp::Le | CmpOp::Gt | CmpOp::Ge => ordered_comparable(lhs, rhs),
        };
        if !ok && *lhs != Ty::Unknown && *rhs != Ty::Unknown {
            self.diag(
                CftErrorCode::ComparisonTypeMismatch,
                span,
                "comparison operands are not compatible",
            );
        }
        Ty::Bool
    }

    fn expect_bool(&mut self, ty: &Ty, span: Span) {
        if !same_type(ty, &Ty::Bool) && *ty != Ty::Unknown {
            self.diag(
                CftErrorCode::ConditionMustBeBool,
                span,
                "check conditions must be bool",
            );
        }
    }

    fn expect_arity(&mut self, args: &[CheckExpr], expected: usize, span: Span) -> Result<(), ()> {
        if args.len() == expected {
            Ok(())
        } else {
            self.diag(
                CftErrorCode::FunctionArityMismatch,
                span,
                format!("expected {expected} argument(s)"),
            );
            Err(())
        }
    }

    fn operator_mismatch(&mut self, lhs: &Ty, rhs: &Ty, span: Span) {
        if *lhs != Ty::Unknown && *rhs != Ty::Unknown {
            self.diag(
                CftErrorCode::OperatorTypeMismatch,
                span,
                "operator does not support these operand types",
            );
        }
    }

    fn is_flag_enum(&self, ty: &Ty) -> bool {
        let Ty::Enum(name) = unwrap_nullable(ty) else {
            return false;
        };
        self.compiler
            .enums
            .get(name)
            .is_some_and(|info| info.is_flag)
    }

    fn diag(&mut self, code: CftErrorCode, span: Span, message: impl Into<String>) {
        self.compiler.diagnostics.push(CftDiagnostic::error(
            code,
            self.type_info.module.clone(),
            span,
            message,
        ));
    }
}

#[derive(Debug, Clone)]
struct ConstInfo<'a> {
    module: ModuleId,
    def: &'a crate::ast::ConstDef,
    value: CftConstValue,
}

#[derive(Debug, Clone)]
struct TypeInfo<'a> {
    module: ModuleId,
    def: &'a TypeDef,
}

#[derive(Debug, Clone)]
struct EnumInfo<'a> {
    module: ModuleId,
    def: &'a EnumDef,
    variants: BTreeSet<String>,
    values: BTreeMap<i64, (ModuleId, Span)>,
    is_flag: bool,
}

#[derive(Debug, Clone)]
struct FieldInfo {
    check_ty: Ty,
}

#[derive(Debug, Clone)]
struct FieldOrigin {
    module: ModuleId,
    span: Span,
}

#[derive(Debug, Clone)]
struct Symbol {
    kind: SymbolKind,
    module: ModuleId,
    span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SymbolKind {
    Const,
    Type,
    Enum,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnnotationTarget {
    Type,
    Enum,
    Field,
}

#[derive(Debug, Clone)]
struct AnnotationSpec {
    targets: &'static [AnnotationTarget],
    args: AnnotationArgs,
}

impl AnnotationSpec {
    fn for_name(name: &str) -> Option<Self> {
        Some(match name {
            "struct" => Self {
                targets: &[AnnotationTarget::Type],
                args: AnnotationArgs::None,
            },
            "flag" => Self {
                targets: &[AnnotationTarget::Enum],
                args: AnnotationArgs::None,
            },
            "id" | "index" => Self {
                targets: &[AnnotationTarget::Field],
                args: AnnotationArgs::None,
            },
            "ref" => Self {
                targets: &[AnnotationTarget::Field],
                args: AnnotationArgs::OneName,
            },
            "display" => Self {
                targets: &[
                    AnnotationTarget::Type,
                    AnnotationTarget::Enum,
                    AnnotationTarget::Field,
                ],
                args: AnnotationArgs::OneString,
            },
            "deprecated" => Self {
                targets: &[
                    AnnotationTarget::Type,
                    AnnotationTarget::Enum,
                    AnnotationTarget::Field,
                ],
                args: AnnotationArgs::None,
            },
            _ => return None,
        })
    }

    fn args_valid(&self, annotation: &Annotation) -> bool {
        match self.args {
            AnnotationArgs::None => annotation.args.is_empty(),
            AnnotationArgs::OneName => {
                matches!(annotation.args.as_slice(), [AnnotationArg::Name(_)])
            }
            AnnotationArgs::OneString => {
                matches!(annotation.args.as_slice(), [AnnotationArg::String(_, _)])
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum AnnotationArgs {
    None,
    OneName,
    OneString,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Ty {
    Int,
    Float,
    Bool,
    String,
    Null,
    Type(String),
    Enum(String),
    EnumNamespace(String),
    Array(Box<Ty>),
    Dict(Box<Ty>, Box<Ty>),
    Nullable(Box<Ty>),
    Entry(Box<Ty>, Box<Ty>),
    EmptyArray,
    EmptyObject,
    Unknown,
}

impl Ty {
    fn from_const(value: &CftConstValue) -> Self {
        match value {
            CftConstValue::Int(_) => Self::Int,
            CftConstValue::Float(_) => Self::Float,
            CftConstValue::Bool(_) => Self::Bool,
            CftConstValue::String(_) => Self::String,
        }
    }

    fn is_nullable(&self) -> bool {
        matches!(self, Self::Nullable(_))
    }
}

fn unwrap_nullable(ty: &Ty) -> &Ty {
    match ty {
        Ty::Nullable(inner) => inner,
        other => other,
    }
}

fn const_value(value: &ConstLiteral) -> CftConstValue {
    match value {
        ConstLiteral::Int(value, _) => CftConstValue::Int(*value),
        ConstLiteral::Float(value, _) => CftConstValue::Float(*value),
        ConstLiteral::Bool(value, _) => CftConstValue::Bool(*value),
        ConstLiteral::String(value, _) => CftConstValue::String(value.clone()),
    }
}

fn has_annotation(annotations: &[Annotation], name: &str) -> bool {
    find_annotation(annotations, name).is_some()
}

fn find_annotation<'a>(annotations: &'a [Annotation], name: &str) -> Option<&'a Annotation> {
    annotations
        .iter()
        .find(|annotation| annotation.name == name)
}

fn convert_annotations(annotations: &[Annotation]) -> Vec<CftAnnotation> {
    annotations
        .iter()
        .map(|annotation| CftAnnotation {
            name: annotation.name.clone(),
            args: annotation
                .args
                .iter()
                .map(|arg| match arg {
                    AnnotationArg::Name(name) => CftAnnotationValue::Name(name.name.clone()),
                    AnnotationArg::String(value, _) => CftAnnotationValue::String(value.clone()),
                    AnnotationArg::Int(value, _) => CftAnnotationValue::Int(*value),
                    AnnotationArg::Float(value, _) => CftAnnotationValue::Float(*value),
                    AnnotationArg::Bool(value, _) => CftAnnotationValue::Bool(*value),
                    AnnotationArg::Null(_) => CftAnnotationValue::Null,
                })
                .collect(),
        })
        .collect()
}

fn convert_check_block(check: &crate::ast::CheckBlock) -> CftSchemaCheckBlock {
    CftSchemaCheckBlock {
        stmts: check.stmts.iter().map(convert_check_stmt).collect(),
        span: check.span,
    }
}

fn convert_check_stmt(stmt: &CheckStmt) -> CftSchemaCheckStmt {
    match stmt {
        CheckStmt::Expr(expr) => CftSchemaCheckStmt::Expr(convert_check_expr(expr)),
        CheckStmt::Quantifier {
            kind,
            binding,
            collection,
            body,
            span,
        } => CftSchemaCheckStmt::Quantifier {
            kind: match kind {
                crate::ast::QuantifierKind::All => CftSchemaQuantifierKind::All,
                crate::ast::QuantifierKind::Any => CftSchemaQuantifierKind::Any,
                crate::ast::QuantifierKind::None => CftSchemaQuantifierKind::None,
            },
            binding: binding.name.clone(),
            collection: convert_check_expr(collection),
            body: body.iter().map(convert_check_stmt).collect(),
            span: *span,
        },
        CheckStmt::When {
            condition,
            body,
            span,
        } => CftSchemaCheckStmt::When {
            condition: convert_check_expr(condition),
            body: body.iter().map(convert_check_stmt).collect(),
            span: *span,
        },
    }
}

fn convert_check_expr(expr: &CheckExpr) -> CftSchemaCheckExpr {
    CftSchemaCheckExpr {
        kind: match &expr.kind {
            CheckExprKind::Int(value) => CftSchemaCheckExprKind::Int(*value),
            CheckExprKind::Float(value) => CftSchemaCheckExprKind::Float(*value),
            CheckExprKind::Bool(value) => CftSchemaCheckExprKind::Bool(*value),
            CheckExprKind::Null => CftSchemaCheckExprKind::Null,
            CheckExprKind::String(value) => CftSchemaCheckExprKind::String(value.clone()),
            CheckExprKind::Name(name) => CftSchemaCheckExprKind::Name(name.clone()),
            CheckExprKind::Field { expr: inner, name } => CftSchemaCheckExprKind::Field {
                expr: Box::new(convert_check_expr(inner)),
                name: name.name.clone(),
            },
            CheckExprKind::Index { expr: inner, index } => CftSchemaCheckExprKind::Index {
                expr: Box::new(convert_check_expr(inner)),
                index: Box::new(convert_check_expr(index)),
            },
            CheckExprKind::Is {
                expr: inner,
                predicate,
            } => CftSchemaCheckExprKind::Is {
                expr: Box::new(convert_check_expr(inner)),
                predicate: match predicate {
                    TypePredicate::Type(name) => CftSchemaTypePredicate::Type(name.name.clone()),
                    TypePredicate::Null(_) => CftSchemaTypePredicate::Null,
                },
            },
            CheckExprKind::Call { name, args } => CftSchemaCheckExprKind::Call {
                name: name.name.clone(),
                args: args.iter().map(convert_check_expr).collect(),
            },
            CheckExprKind::BinOp { op, lhs, rhs } => CftSchemaCheckExprKind::BinOp {
                op: convert_bin_op(*op),
                lhs: Box::new(convert_check_expr(lhs)),
                rhs: Box::new(convert_check_expr(rhs)),
            },
            CheckExprKind::Unary { op, expr: inner } => CftSchemaCheckExprKind::Unary {
                op: match op {
                    UnaryOp::Not => CftSchemaUnaryOp::Not,
                    UnaryOp::BitNot => CftSchemaUnaryOp::BitNot,
                    UnaryOp::Neg => CftSchemaUnaryOp::Neg,
                },
                expr: Box::new(convert_check_expr(inner)),
            },
            CheckExprKind::CmpChain { first, rest } => CftSchemaCheckExprKind::CmpChain {
                first: Box::new(convert_check_expr(first)),
                rest: rest
                    .iter()
                    .map(|(op, rhs)| (convert_cmp_op(*op), convert_check_expr(rhs)))
                    .collect(),
            },
        },
        span: expr.span,
    }
}

fn convert_bin_op(op: BinOp) -> CftSchemaBinOp {
    match op {
        BinOp::Or => CftSchemaBinOp::Or,
        BinOp::And => CftSchemaBinOp::And,
        BinOp::BitOr => CftSchemaBinOp::BitOr,
        BinOp::BitXor => CftSchemaBinOp::BitXor,
        BinOp::BitAnd => CftSchemaBinOp::BitAnd,
        BinOp::Add => CftSchemaBinOp::Add,
        BinOp::Sub => CftSchemaBinOp::Sub,
        BinOp::Shl => CftSchemaBinOp::Shl,
        BinOp::Shr => CftSchemaBinOp::Shr,
        BinOp::Mul => CftSchemaBinOp::Mul,
        BinOp::Div => CftSchemaBinOp::Div,
        BinOp::IntDiv => CftSchemaBinOp::IntDiv,
        BinOp::Mod => CftSchemaBinOp::Mod,
        BinOp::Pow => CftSchemaBinOp::Pow,
    }
}

fn convert_cmp_op(op: CmpOp) -> CftSchemaCmpOp {
    match op {
        CmpOp::Eq => CftSchemaCmpOp::Eq,
        CmpOp::Ne => CftSchemaCmpOp::Ne,
        CmpOp::Lt => CftSchemaCmpOp::Lt,
        CmpOp::Le => CftSchemaCmpOp::Le,
        CmpOp::Gt => CftSchemaCmpOp::Gt,
        CmpOp::Ge => CftSchemaCmpOp::Ge,
    }
}

fn format_type_ref(ty: &TypeRef) -> String {
    match &ty.kind {
        TypeRefKind::Int => "int".to_string(),
        TypeRefKind::Float => "float".to_string(),
        TypeRefKind::Bool => "bool".to_string(),
        TypeRefKind::String => "string".to_string(),
        TypeRefKind::Named(name) => name.clone(),
        TypeRefKind::Array(inner) => format!("[{}]", format_type_ref(inner)),
        TypeRefKind::Dict(key, value) => {
            format!("{{{}: {}}}", format_type_ref(key), format_type_ref(value))
        }
        TypeRefKind::Nullable(inner) => format!("{}?", format_type_ref(inner)),
    }
}

fn is_valid_dict_key(ty: &Ty) -> bool {
    matches!(ty, Ty::Int | Ty::String | Ty::Enum(_) | Ty::Unknown)
}

fn is_string_or_int(ty: &Ty, allow_nullable: bool) -> bool {
    match ty {
        Ty::String | Ty::Int | Ty::Unknown => true,
        Ty::Nullable(inner) if allow_nullable => matches!(inner.as_ref(), Ty::String | Ty::Int),
        _ => false,
    }
}

fn is_indexable_field_type(ty: &Ty) -> bool {
    match ty {
        Ty::String | Ty::Int | Ty::Enum(_) | Ty::Unknown => true,
        Ty::Nullable(inner) => matches!(inner.as_ref(), Ty::String | Ty::Int | Ty::Enum(_)),
        _ => false,
    }
}

fn types_assignable(expected: &Ty, actual: &Ty) -> bool {
    if matches!(expected, Ty::Unknown) || matches!(actual, Ty::Unknown) {
        return true;
    }
    match (expected, actual) {
        (Ty::Nullable(inner), Ty::Null) => !matches!(inner.as_ref(), Ty::Unknown),
        (Ty::Nullable(inner), other) => types_assignable(inner, other),
        (Ty::Array(_), Ty::EmptyArray) | (Ty::Dict(_, _), Ty::EmptyObject) => true,
        (Ty::Enum(left), Ty::Enum(right)) | (Ty::Type(left), Ty::Type(right)) => left == right,
        _ => expected == actual,
    }
}

fn same_type(left: &Ty, right: &Ty) -> bool {
    types_comparable(left, right)
}

fn types_comparable(left: &Ty, right: &Ty) -> bool {
    if matches!(left, Ty::Unknown) || matches!(right, Ty::Unknown) {
        return true;
    }
    if matches!((left, right), (Ty::Null, Ty::Null)) {
        return true;
    }
    if matches!(
        (left, right),
        (Ty::Null, Ty::Nullable(_)) | (Ty::Nullable(_), Ty::Null)
    ) {
        return true;
    }
    match (unwrap_nullable(left), unwrap_nullable(right)) {
        (Ty::Unknown, _)
        | (_, Ty::Unknown)
        | (Ty::Null, Ty::Null)
        | (Ty::Int, Ty::Int)
        | (Ty::Float, Ty::Float)
        | (Ty::Bool, Ty::Bool)
        | (Ty::String, Ty::String) => true,
        (Ty::Enum(left), Ty::Enum(right)) | (Ty::Type(left), Ty::Type(right)) => left == right,
        _ => false,
    }
}

fn ordered_comparable(left: &Ty, right: &Ty) -> bool {
    match (unwrap_nullable(left), unwrap_nullable(right)) {
        (Ty::Unknown, _) | (_, Ty::Unknown) | (Ty::Int, Ty::Int) | (Ty::Float, Ty::Float) => true,
        (Ty::Enum(left), Ty::Enum(right)) => left == right,
        _ => false,
    }
}

fn unique_supported(ty: &Ty) -> bool {
    matches!(
        unwrap_nullable(ty),
        Ty::Int | Ty::Bool | Ty::String | Ty::Enum(_)
    )
}

fn min_max_supported(ty: &Ty) -> bool {
    matches!(unwrap_nullable(ty), Ty::Int | Ty::Float | Ty::Enum(_))
}

fn is_i64_power_of_two(value: i64) -> bool {
    value > 0 && (value & (value - 1)) == 0
}
