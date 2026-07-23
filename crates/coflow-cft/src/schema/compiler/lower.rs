use super::annotations::{find_annotation, has_annotation};
use super::SchemaCompiler;
use crate::schema::{
    CftConst, CftConstValue, CftEnum, CftEnumVariant, CftField, CftFieldDimension, CftSchemaBinOp,
    CftSchemaCheckBlock, CftSchemaCheckExpr, CftSchemaCheckExprKind, CftSchemaCheckFormatSegment,
    CftSchemaCheckMessage, CftSchemaCheckMessageKind, CftSchemaCheckStmt, CftSchemaCmpOp,
    CftSchemaDefaultValue, CftSchemaQuantifierKind, CftSchemaTypePredicate, CftSchemaUnaryOp,
    CftTopLevelCheck, CftType, CftValueType,
};
use crate::syntax::ast::{
    AnnotationArg, BinOp, CheckExpr, CheckExprKind, CheckFormatSegment, CheckMessageKind,
    CheckStmt, CmpOp, ConstLiteral, DefaultExpr, DefaultExprKind, FieldDef, TypePredicate, TypeRef,
    TypeRefKind, UnaryOp,
};
use crate::{BucketName, CheckName, ConstName, DimensionName, EnumName, EnumVariantName, FieldName, TypeName};
use std::collections::BTreeMap;
use std::sync::Arc;

impl SchemaCompiler<'_> {
    pub(super) fn lower_declarations(&self) -> super::SchemaDeclarations {
        super::SchemaDeclarations {
            consts: self.build_consts(),
            enums: self.build_enums(),
            types: self.build_types(),
            checks: self.build_checks(),
            sources: self
                .modules
                .modules()
                .map(|(id, module)| {
                    (
                        id.clone(),
                        crate::schema::CftSchemaSource {
                            path: module.path().to_path_buf(),
                            source: module.shared_source(),
                        },
                    )
                })
                .collect(),
        }
    }

    fn build_checks(&self) -> BTreeMap<CheckName, CftTopLevelCheck> {
        self.checks
            .iter()
            .map(|(name, info)| {
                let name = CheckName::from_validated(name.clone());
                let block = self.convert_check_block(&info.module, &info.def.block);
                let record_sets = collect_record_sets(&block);
                let check = CftTopLevelCheck {
                    module: info.module.clone(),
                    name: name.clone(),
                    block,
                    span: info.def.span,
                    record_sets,
                };
                (name, check)
            })
            .collect()
    }

    fn build_consts(&self) -> BTreeMap<ConstName, CftConst> {
        let mut consts = BTreeMap::new();
        for (name, info) in &self.consts {
            let name = ConstName::from_validated(name.clone());
            let schema = CftConst {
                module: info.module.clone(),
                name: name.clone(),
                value: info.value.clone(),
                span: info.def.span,
            };
            consts.insert(name, schema);
        }
        consts
    }

    fn build_enums(&self) -> BTreeMap<EnumName, CftEnum> {
        let mut enums = BTreeMap::new();
        for (name, info) in &self.enums {
            // `validate_enums` already resolved every variant's integer value
            // (auto-numbered or explicit) into `values_by_name`. We just look
            // them up here instead of re-walking the sequence.
            let variants = info
                .def
                .variants
                .iter()
                .map(|variant| CftEnumVariant {
                    name: EnumVariantName::from_validated(variant.name.clone()),
                    value: info
                        .values_by_name
                        .get(&variant.name)
                        .copied()
                        .map_or(0, |value| value),
                    span: variant.span,
                })
                .collect::<Vec<_>>();
            let variant_by_name = variants
                .iter()
                .enumerate()
                .map(|(index, variant)| (variant.name.clone(), index))
                .collect();
            let variant_by_value = variants
                .iter()
                .enumerate()
                .map(|(index, variant)| (variant.value, index))
                .collect();
            let name = EnumName::from_validated(name.clone());
            let schema = CftEnum {
                module: info.module.clone(),
                name: name.clone(),
                variants,
                variant_by_name,
                variant_by_value,
                is_flag: has_annotation(&info.def.annotations, "flag"),
                span: info.def.span,
            };
            enums.insert(name, schema);
        }
        enums
    }

    fn build_types(&self) -> BTreeMap<TypeName, CftType> {
        let own_fields = self
            .types
            .iter()
            .map(|(name, info)| {
                let type_name = TypeName::from_validated(name.clone());
                let fields = info
                    .def
                    .fields
                    .iter()
                    .map(|field| Arc::new(self.build_schema_field(field, &type_name)))
                    .collect::<Vec<_>>();
                (type_name, fields)
            })
            .collect::<BTreeMap<_, _>>();
        let mut types = BTreeMap::new();
        for (name, info) in &self.types {
            let type_name = TypeName::from_validated(name.clone());
            let fields = own_fields.get(&type_name).cloned().unwrap_or_default();
            let all_fields = self.collect_all_schema_fields(name, &own_fields);
            let field_by_name = all_fields
                .iter()
                .enumerate()
                .map(|(index, field)| (field.name.clone(), index))
                .collect();
            let is_singleton = has_annotation(&info.def.annotations, "singleton");
            let id_as_enum = find_annotation(&info.def.annotations, "idAsEnum")
                .and_then(|annotation| annotation.args.first())
                .and_then(|arg| match arg {
                    AnnotationArg::Name(name) => Some(EnumName::from_validated(name.name.clone())),
                    _ => None,
                });
            let schema = CftType {
                module: info.module.clone(),
                name: type_name.clone(),
                parent: info
                    .def
                    .parent
                    .as_ref()
                    .map(|parent| TypeName::from_validated(parent.name.clone())),
                is_abstract: info.def.is_abstract,
                is_sealed: info.def.is_sealed,
                is_struct: has_annotation(&info.def.annotations, "struct"),
                is_singleton,
                id_as_enum,
                own_fields: fields,
                all_fields,
                field_by_name,
                check: info
                    .def
                    .check
                    .as_ref()
                    .map(|check| self.convert_check_block(&info.module, check)),
                span: info.def.span,
            };
            types.insert(type_name, schema);
        }
        types
    }

    fn build_schema_field(&self, field: &FieldDef, owner_type: &TypeName) -> CftField {
        let localized = find_annotation(&field.annotations, "localized");
        let dimension_annotation = find_annotation(&field.annotations, "dimension");
        let dimension = localized
            .map(|_| CftFieldDimension {
                dimension: DimensionName::from_validated("language"),
                bucket: localized_bucket(field),
            })
            .or_else(|| {
                let annotation = dimension_annotation?;
                let Some(AnnotationArg::String(name, _)) = annotation.args.first() else {
                    return None;
                };
                Some(CftFieldDimension {
                    dimension: DimensionName::from_validated(name.clone()),
                    bucket: None,
                })
            });
        CftField {
            declaring_type: owner_type.clone(),
            name: FieldName::from_validated(field.name.clone()),
            value_type: build_schema_value_type(&field.ty, &|name| self.enums.contains_key(name)),
            default: field
                .default
                .as_ref()
                .and_then(|default| self.schema_default_value(default)),
            is_expand: has_annotation(&field.annotations, "expand"),
            dimension,
            span: field.span,
        }
    }

    fn collect_all_schema_fields(
        &self,
        type_name: &str,
        own_fields: &BTreeMap<TypeName, Vec<Arc<CftField>>>,
    ) -> Vec<Arc<CftField>> {
        self.ancestry_chain(type_name)
            .into_iter()
            .flat_map(|info| {
                own_fields
                    .get(info.def.name.as_str())
                    .cloned()
                    .unwrap_or_default()
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
                enum_name: EnumName::from_validated(enum_name.name.clone()),
                variant: EnumVariantName::from_validated(variant.name.clone()),
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
}

fn collect_record_sets(block: &CftSchemaCheckBlock) -> std::collections::BTreeSet<TypeName> {
    struct Collector(std::collections::BTreeSet<TypeName>);
    impl crate::schema::check_visit::CheckVisitor for Collector {
        fn visit_records(&mut self, type_name: &TypeName) {
            self.0.insert(type_name.clone());
        }
    }
    let mut collector = Collector(std::collections::BTreeSet::new());
    crate::schema::check_visit::CheckVisitor::visit_block(&mut collector, block);
    collector.0
}

fn localized_bucket(field: &FieldDef) -> Option<BucketName> {
    let annotation = find_annotation(&field.annotations, "localized")?;
    match annotation.args.first() {
        Some(AnnotationArg::String(bucket, _)) => Some(BucketName::from_validated(bucket.clone())),
        _ => None,
    }
}

pub(super) fn const_value(value: &ConstLiteral) -> CftConstValue {
    match value {
        ConstLiteral::Int(value, _) => CftConstValue::Int(*value),
        ConstLiteral::Float(value, _) => CftConstValue::Float(*value),
        ConstLiteral::Bool(value, _) => CftConstValue::Bool(*value),
        ConstLiteral::String(value, _) => CftConstValue::String(value.clone()),
    }
}
impl SchemaCompiler<'_> {
    fn convert_check_block(
        &self,
        module: &crate::ModuleId,
        check: &crate::syntax::ast::CheckBlock,
    ) -> CftSchemaCheckBlock {
        CftSchemaCheckBlock {
            stmts: check
                .stmts
                .iter()
                .map(|stmt| self.convert_check_stmt(module, stmt))
                .collect(),
            span: check.span,
        }
    }

    fn convert_check_stmt(&self, module: &crate::ModuleId, stmt: &CheckStmt) -> CftSchemaCheckStmt {
        match stmt {
            CheckStmt::Expr {
                condition,
                message,
                span,
            } => CftSchemaCheckStmt::Expr {
                condition: convert_check_expr(condition),
                message: message.as_ref().map(|message| CftSchemaCheckMessage {
                    kind: match &message.kind {
                        CheckMessageKind::String(value) => {
                            CftSchemaCheckMessageKind::String(value.clone())
                        }
                        CheckMessageKind::Formatted(segments) => {
                            CftSchemaCheckMessageKind::Formatted(convert_format_segments(segments))
                        }
                    },
                    span: message.span,
                }),
                span: *span,
            },
            CheckStmt::Quantifier {
                kind,
                bindings: _,
                collection,
                body,
                span,
            } => CftSchemaCheckStmt::Quantifier {
                kind: match kind {
                    crate::syntax::ast::QuantifierKind::All => CftSchemaQuantifierKind::All,
                    crate::syntax::ast::QuantifierKind::Any => CftSchemaQuantifierKind::Any,
                    crate::syntax::ast::QuantifierKind::None => CftSchemaQuantifierKind::None,
                },
                bindings: self.quantifier_bindings[&(module.clone(), span.start, span.end)].clone(),
                collection: convert_check_expr(collection),
                body: body
                    .iter()
                    .map(|stmt| self.convert_check_stmt(module, stmt))
                    .collect(),
                span: *span,
            },
            CheckStmt::When {
                condition,
                body,
                span,
            } => CftSchemaCheckStmt::When {
                condition: convert_check_expr(condition),
                body: body
                    .iter()
                    .map(|stmt| self.convert_check_stmt(module, stmt))
                    .collect(),
                span: *span,
            },
        }
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
            CheckExprKind::FormattedString(segments) => {
                CftSchemaCheckExprKind::FormattedString(convert_format_segments(segments))
            }
            CheckExprKind::Name(name) => CftSchemaCheckExprKind::Name(name.clone()),
            CheckExprKind::Records { type_name } => CftSchemaCheckExprKind::Records {
                type_name: TypeName::from_validated(type_name.name.clone()),
            },
            CheckExprKind::Field { expr: inner, name } => CftSchemaCheckExprKind::Field {
                expr: Box::new(convert_check_expr(inner)),
                name: FieldName::from_validated(name.name.clone()),
            },
            CheckExprKind::SafeField { expr: inner, name } => CftSchemaCheckExprKind::SafeField {
                expr: Box::new(convert_check_expr(inner)),
                name: FieldName::from_validated(name.name.clone()),
            },
            CheckExprKind::Index { expr: inner, index } => CftSchemaCheckExprKind::Index {
                expr: Box::new(convert_check_expr(inner)),
                index: Box::new(convert_check_expr(index)),
            },
            CheckExprKind::SafeIndex { expr: inner, index } => CftSchemaCheckExprKind::SafeIndex {
                expr: Box::new(convert_check_expr(inner)),
                index: Box::new(convert_check_expr(index)),
            },
            CheckExprKind::Coalesce { lhs, rhs } => CftSchemaCheckExprKind::Coalesce {
                lhs: Box::new(convert_check_expr(lhs)),
                rhs: Box::new(convert_check_expr(rhs)),
            },
            CheckExprKind::Is {
                expr: inner,
                predicate,
            } => CftSchemaCheckExprKind::Is {
                expr: Box::new(convert_check_expr(inner)),
                predicate: match predicate {
                    TypePredicate::Type(name) => {
                        CftSchemaTypePredicate::Type(TypeName::from_validated(name.name.clone()))
                    }
                    TypePredicate::Null(_) => CftSchemaTypePredicate::Null,
                },
            },
            CheckExprKind::Call { name, args } => CftSchemaCheckExprKind::Call {
                name: name.name.clone(),
                args: args.iter().map(convert_check_expr).collect(),
            },
            CheckExprKind::MethodCall {
                receiver,
                name,
                args,
            } => CftSchemaCheckExprKind::MethodCall {
                receiver: Box::new(convert_check_expr(receiver)),
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

fn convert_format_segments(segments: &[CheckFormatSegment]) -> Vec<CftSchemaCheckFormatSegment> {
    segments
        .iter()
        .map(|segment| match segment {
            CheckFormatSegment::Text(value, span) => {
                CftSchemaCheckFormatSegment::Text(value.clone(), *span)
            }
            CheckFormatSegment::Expr(expr) => {
                CftSchemaCheckFormatSegment::Expr(convert_check_expr(expr))
            }
        })
        .collect()
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

pub(super) fn build_schema_value_type(
    ty: &TypeRef,
    is_enum: &impl Fn(&str) -> bool,
) -> CftValueType {
    match &ty.kind {
        TypeRefKind::Int => CftValueType::Int,
        TypeRefKind::Float => CftValueType::Float,
        TypeRefKind::Bool => CftValueType::Bool,
        TypeRefKind::String => CftValueType::String,
        TypeRefKind::Named(name) if is_enum(name) => {
            CftValueType::Enum(EnumName::from_validated(name.clone()))
        }
        TypeRefKind::Named(name) => CftValueType::Object(TypeName::from_validated(name.clone())),
        TypeRefKind::Ref(inner) => match &inner.kind {
            TypeRefKind::Named(name) => {
                CftValueType::RecordRef(TypeName::from_validated(name.clone()))
            }
            _ => build_schema_value_type(inner, is_enum),
        },
        TypeRefKind::Array(inner) => {
            CftValueType::Array(Box::new(build_schema_value_type(inner, is_enum)))
        }
        TypeRefKind::Dict(key, value) => CftValueType::Dict(
            Box::new(build_schema_value_type(key, is_enum)),
            Box::new(build_schema_value_type(value, is_enum)),
        ),
        TypeRefKind::Nullable(inner) => {
            CftValueType::Nullable(Box::new(build_schema_value_type(inner, is_enum)))
        }
    }
}
