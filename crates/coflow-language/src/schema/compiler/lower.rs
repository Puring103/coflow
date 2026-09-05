use super::annotations::{field_dimension_name, find_annotation, has_annotation};
use super::ValidatedSchema;
use crate::schema::{
    CftAnnotation, CftAnnotationValue, CftConst, CftConstValue, CftDisplayMetadata, CftEnum, CftEnumVariant, CftField,
    CftFieldDimension, CftSchemaBinOp, CftSchemaCheckBlock, CftSchemaCheckExpr,
    CftSchemaCheckExprKind, CftSchemaCheckFormatSegment, CftSchemaCheckMessage,
    CftSchemaCheckMessageKind, CftSchemaCheckStmt, CftSchemaCmpOp, CftSchemaDefaultValue,
    CftSchemaQuantifierKind, CftSchemaTypePredicate, CftSchemaUnaryOp, CftTopLevelCheck, CftType,
    CftValueType,
};
use crate::syntax::ast::{
    Annotation, AnnotationArg, BinOp, CheckExpr, CheckExprKind, CheckFormatSegment,
    CheckMessageKind, CheckStmt, CmpOp, DefaultExpr, FieldDef,
    TypePredicate, UnaryOp,
};
use crate::{BucketName, CheckName, ConstName, EnumName, EnumVariantName, FieldName, TypeName};
use std::collections::BTreeMap;
use std::sync::Arc;

impl ValidatedSchema<'_> {
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
                let check = CftTopLevelCheck {
                    module: info.module.clone(),
                    name: name.clone(),
                    block,
                    span: info.def.span,
                };
                (name, check)
            })
            .collect()
    }

    fn build_consts(&self) -> BTreeMap<ConstName, CftConst> {
        let mut consts = BTreeMap::new();
        for (name, info) in &self.consts {
            let (value_type, value) = self
                .resolved_constants
                .get(name)
                .expect("constants are resolved before lowering");
            let name = ConstName::from_validated(name.clone());
            let schema = CftConst {
                module: info.module.clone(),
                name: name.clone(),
                value_type: value_type.clone(),
                value: value.clone(),
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
                    annotations: self.schema_annotations(&variant.annotations),
                    display: display_metadata(&variant.annotations),
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
                annotations: self.schema_annotations(&info.def.annotations),
                display: display_metadata(&info.def.annotations),
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
                    .map(|field| {
                        Arc::new(self.build_schema_field(&info.module, field, &type_name))
                    })
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
            let is_host = has_annotation(&info.def.annotations, "Host");
            let id_as_enum = find_annotation(&info.def.annotations, "idAsEnum")
                .and_then(|annotation| annotation.args.first())
                .and_then(|arg| match arg {
                    AnnotationArg::Name(name) => Some(EnumName::from_validated(
                        name.name.clone(),
                    )),
                    _ => None,
                });
            let schema = CftType {
                module: info.module.clone(),
                name: type_name.clone(),
                parent: info
                    .def
                    .parent
                    .as_ref()
                    .map(|parent| {
                        TypeName::from_validated(parent.name.clone())
                    }),
                is_abstract: info.def.is_abstract,
                is_sealed: info.def.is_sealed,
                is_struct: has_annotation(&info.def.annotations, "struct"),
                is_singleton,
                is_host,
                id_as_enum,
                annotations: self.schema_annotations(&info.def.annotations),
                display: display_metadata(&info.def.annotations),
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

    fn build_schema_field(
        &self,
        module: &crate::ModuleId,
        field: &FieldDef,
        owner_type: &TypeName,
    ) -> CftField {
        let dimension = field_dimension_name(field).map(|dimension| CftFieldDimension {
            bucket: (dimension.as_str() == "language")
                .then(|| localized_bucket(field))
                .flatten(),
            dimension,
        });
        CftField {
            declaring_type: owner_type.clone(),
            name: FieldName::from_validated(field.name.clone()),
            value_type: self
                .resolve_field_type(module, &field.ty)
                .value_type()
                .cloned()
                .unwrap_or(CftValueType::Unit),
            default: field
                .default
                .as_ref()
                .and_then(|default| self.schema_default_value(module, default)),
            is_expand: has_annotation(&field.annotations, "expand"),
            dimension,
            annotations: self.schema_annotations(&field.annotations),
            display: display_metadata(&field.annotations),
            span: field.span,
        }
    }

    fn schema_annotations(
        &self,
        annotations: &[Annotation],
    ) -> Vec<CftAnnotation> {
        annotations
            .iter()
            .map(|annotation| CftAnnotation {
                name: annotation.name.clone(),
                arguments: annotation
                    .args
                    .iter()
                    .map(|argument| match argument {
                        AnnotationArg::Name(name) => CftAnnotationValue::Name(
                            name.name.clone(),
                        ),
                        AnnotationArg::String(value, _) => {
                            CftAnnotationValue::String(value.clone())
                        }
                        AnnotationArg::Int(value, _) => CftAnnotationValue::Int(*value),
                        AnnotationArg::Float(value, _) => CftAnnotationValue::Float(*value),
                        AnnotationArg::Bool(value, _) => CftAnnotationValue::Bool(*value),
                    })
                    .collect(),
            })
            .collect()
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
                    .get(info.name.as_str())
                    .cloned()
                    .unwrap_or_default()
            })
            .collect()
    }

    fn schema_default_value(
        &self,
        module: &crate::ModuleId,
        expr: &DefaultExpr,
    ) -> Option<CftSchemaDefaultValue> {
        let value = self
            .resolved_defaults
            .get(&(module.clone(), expr.span.start, expr.span.end))?;
        Some(const_value_as_default(value))
    }

}

fn const_value_as_default(value: &CftConstValue) -> CftSchemaDefaultValue {
    match value {
        CftConstValue::Int(value) => CftSchemaDefaultValue::Int(*value),
        CftConstValue::Float(value) => CftSchemaDefaultValue::Float(*value),
        CftConstValue::Bool(value) => CftSchemaDefaultValue::Bool(*value),
        CftConstValue::String(value) => CftSchemaDefaultValue::String(value.clone()),
        CftConstValue::FormattedString(source) => {
            CftSchemaDefaultValue::FormattedString(source.clone())
        }
        CftConstValue::Function(source) => CftSchemaDefaultValue::Function(source.clone()),
        CftConstValue::Enum {
            enum_name,
            variant,
            value,
        } => CftSchemaDefaultValue::Enum {
            enum_name: enum_name.clone(),
            variant: variant.clone(),
            value: *value,
        },
        CftConstValue::OptionNone => CftSchemaDefaultValue::OptionNone,
        CftConstValue::OptionSome(value) => {
            CftSchemaDefaultValue::OptionSome(Box::new(const_value_as_default(value)))
        }
        CftConstValue::ResultOk(value) => {
            CftSchemaDefaultValue::ResultOk(Box::new(const_value_as_default(value)))
        }
        CftConstValue::ResultErr(value) => {
            CftSchemaDefaultValue::ResultErr(Box::new(const_value_as_default(value)))
        }
        CftConstValue::Array(values) => CftSchemaDefaultValue::Array(
            values.iter().map(const_value_as_default).collect(),
        ),
        CftConstValue::Dictionary(entries) => CftSchemaDefaultValue::Dictionary(
            entries
                .iter()
                .map(|(key, value)| {
                    (const_value_as_default(key), const_value_as_default(value))
                })
                .collect(),
        ),
        CftConstValue::Object { fields, .. } if fields.is_empty() => {
            CftSchemaDefaultValue::EmptyObject
        }
        CftConstValue::Object { type_name, fields } => CftSchemaDefaultValue::Object {
            type_name: type_name.clone(),
            fields: fields
                .iter()
                .map(|(name, value)| (name.clone(), const_value_as_default(value)))
                .collect(),
        },
        CftConstValue::RecordReference { type_name, key } => {
            CftSchemaDefaultValue::RecordReference {
                type_name: type_name.clone(),
                key: key.clone(),
            }
        }
    }
}

fn localized_bucket(field: &FieldDef) -> Option<BucketName> {
    let annotation = find_annotation(&field.annotations, "localized")?;
    match annotation.args.first() {
        Some(AnnotationArg::String(bucket, _)) => Some(BucketName::from_validated(bucket.clone())),
        _ => None,
    }
}

impl ValidatedSchema<'_> {
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
            dimension_statements: self
                .check_dimensions
                .get(&(module.clone(), check.span.start, check.span.end))
                .cloned()
                .unwrap_or_default(),
            statement_dependencies: self
                .check_statement_dependencies
                .get(&(module.clone(), check.span.start, check.span.end))
                .cloned()
                .unwrap_or_else(|| {
                    vec![crate::schema::CheckStatementDependencies::default(); check.stmts.len()]
                }),
        }
    }

    fn convert_check_stmt(&self, module: &crate::ModuleId, stmt: &CheckStmt) -> CftSchemaCheckStmt {
        match stmt {
            CheckStmt::Expr {
                condition,
                message,
                span,
            } => CftSchemaCheckStmt::Expr {
                condition: self.convert_check_expr(module, condition),
                message: message.as_ref().map(|message| CftSchemaCheckMessage {
                    kind: match &message.kind {
                        CheckMessageKind::String(value) => {
                            CftSchemaCheckMessageKind::String(value.clone())
                        }
                        CheckMessageKind::Formatted(segments) => {
                            CftSchemaCheckMessageKind::Formatted(
                                self.convert_format_segments(module, segments),
                            )
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
                collection: self.convert_check_expr(module, collection),
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
                condition: self.convert_check_expr(module, condition),
                body: body
                    .iter()
                    .map(|stmt| self.convert_check_stmt(module, stmt))
                    .collect(),
                span: *span,
            },
        }
    }
}

impl ValidatedSchema<'_> {
fn convert_check_expr(&self, module: &crate::ModuleId, expr: &CheckExpr) -> CftSchemaCheckExpr {
    CftSchemaCheckExpr {
        kind: match &expr.kind {
            CheckExprKind::Int(value) => CftSchemaCheckExprKind::Int(*value),
            CheckExprKind::Float(value) => CftSchemaCheckExprKind::Float(*value),
            CheckExprKind::Bool(value) => CftSchemaCheckExprKind::Bool(*value),
            CheckExprKind::String(value) => CftSchemaCheckExprKind::String(value.clone()),
            CheckExprKind::FormattedString(segments) => {
                CftSchemaCheckExprKind::FormattedString(
                    self.convert_format_segments(module, segments),
                )
            }
            CheckExprKind::Name(name) => CftSchemaCheckExprKind::Name(name.clone()),
            CheckExprKind::StaticPath(path) => {
                let raw_name = path.canonical();
                let resolved_name = raw_name;
                if self.consts.contains_key(&resolved_name)
                    || self.enums.contains_key(&resolved_name)
                {
                    CftSchemaCheckExprKind::Name(resolved_name)
                } else {
                    // 语法层保证限定名至少包含一个段，这里直接拆出末段作为枚举成员。
                    let (variant, owner) = path
                        .segments
                        .split_last()
                        .expect("name path must contain at least one segment");
                    let owner = owner
                        .iter()
                        .map(|segment| segment.name.as_str())
                        .collect::<Vec<_>>()
                        .join("::");
                    CftSchemaCheckExprKind::Field {
                        expr: Box::new(CftSchemaCheckExpr {
                            kind: CftSchemaCheckExprKind::Name(owner),
                            span: path.span,
                        }),
                        name: FieldName::from_validated(variant.name.clone()),
                    }
                }
            }
            CheckExprKind::Records { type_name } => CftSchemaCheckExprKind::Records {
                type_name: TypeName::from_validated(
                    type_name.name.clone(),
                ),
            },
            CheckExprKind::Field { expr: inner, name } => CftSchemaCheckExprKind::Field {
                expr: Box::new(self.convert_check_expr(module, inner)),
                name: FieldName::from_validated(name.name.clone()),
            },
            CheckExprKind::Index { expr: inner, index } => CftSchemaCheckExprKind::Index {
                expr: Box::new(self.convert_check_expr(module, inner)),
                index: Box::new(self.convert_check_expr(module, index)),
            },
            CheckExprKind::Is {
                expr: inner,
                predicate,
            } => CftSchemaCheckExprKind::Is {
                expr: Box::new(self.convert_check_expr(module, inner)),
                predicate: match predicate {
                    TypePredicate::Type(name) => {
                        CftSchemaTypePredicate::Type(TypeName::from_validated(
                            name.name.clone(),
                        ))
                    }
                },
            },
            CheckExprKind::Call { name, args } => CftSchemaCheckExprKind::Call {
                name: name.name.clone(),
                args: args
                    .iter()
                    .map(|arg| self.convert_check_expr(module, arg))
                    .collect(),
            },
            CheckExprKind::MethodCall {
                receiver,
                name,
                args,
            } => CftSchemaCheckExprKind::MethodCall {
                receiver: Box::new(self.convert_check_expr(module, receiver)),
                name: name.name.clone(),
                args: args
                    .iter()
                    .map(|arg| self.convert_check_expr(module, arg))
                    .collect(),
            },
            CheckExprKind::BinOp { op, lhs, rhs } => CftSchemaCheckExprKind::BinOp {
                op: convert_bin_op(*op),
                lhs: Box::new(self.convert_check_expr(module, lhs)),
                rhs: Box::new(self.convert_check_expr(module, rhs)),
            },
            CheckExprKind::Unary { op, expr: inner } => CftSchemaCheckExprKind::Unary {
                op: match op {
                    UnaryOp::Not => CftSchemaUnaryOp::Not,
                    UnaryOp::BitNot => CftSchemaUnaryOp::BitNot,
                    UnaryOp::Neg => CftSchemaUnaryOp::Neg,
                },
                expr: Box::new(self.convert_check_expr(module, inner)),
            },
            CheckExprKind::CmpChain { first, rest } => CftSchemaCheckExprKind::CmpChain {
                first: Box::new(self.convert_check_expr(module, first)),
                rest: rest
                    .iter()
                    .map(|(op, rhs)| {
                        (convert_cmp_op(*op), self.convert_check_expr(module, rhs))
                    })
                    .collect(),
            },
        },
        span: expr.span,
    }
}

fn convert_format_segments(
    &self,
    module: &crate::ModuleId,
    segments: &[CheckFormatSegment],
) -> Vec<CftSchemaCheckFormatSegment> {
    segments
        .iter()
        .map(|segment| match segment {
            CheckFormatSegment::Text(value, span) => {
                CftSchemaCheckFormatSegment::Text(value.clone(), *span)
            }
            CheckFormatSegment::Expr(expr) => {
                CftSchemaCheckFormatSegment::Expr(self.convert_check_expr(module, expr))
            }
        })
        .collect()
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

fn display_metadata(annotations: &[Annotation]) -> Option<CftDisplayMetadata> {
    let string_arg = |name| {
        find_annotation(annotations, name)
            .and_then(|annotation| annotation.args.first())
            .and_then(|arg| match arg {
                AnnotationArg::String(value, _) => Some(value.clone()),
                _ => None,
            })
    };
    let label = string_arg("label");
    let description = string_arg("description");
    (label.is_some() || description.is_some()).then_some(CftDisplayMetadata { label, description })
}
