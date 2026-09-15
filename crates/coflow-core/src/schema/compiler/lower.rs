use super::annotations::{field_dimension_name, find_annotation, has_annotation};
use super::ValidatedSchema;
use crate::schema::{
    CftAnnotation, CftAnnotationValue, CftConst, CftConstValue, CftDisplayMetadata, CftEnum,
    CftEnumVariant, CftField, CftFieldDimension, CftSchemaCheckBlock, CftSchemaDefaultValue,
    CftTopLevelCheck, CftType, CftValueType,
};
use crate::{BucketName, CheckName, ConstName, EnumName, EnumVariantName, FieldName, TypeName};
use coflow_language::cft::syntax::ast::{Annotation, AnnotationArg, DefaultExpr, FieldDef};
use std::collections::BTreeMap;
use std::sync::Arc;

impl ValidatedSchema<'_> {
    pub(super) fn lower_declarations(&self) -> super::SchemaDeclarations {
        super::SchemaDeclarations {
            aliases: self
                .resolved_aliases
                .iter()
                .filter_map(|(name, ty)| ty.value_type().map(|ty| (name.clone(), ty.clone())))
                .collect(),
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
            let Some((value_type, value)) = self.constants.get(name) else {
                debug_assert!(false, "constants are resolved before lowering");
                continue;
            };
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
                    value: info.values_by_name.get(&variant.name).copied().unwrap_or(0),
                    annotations: Self::schema_annotations(&variant.annotations),
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
                annotations: Self::schema_annotations(&info.def.annotations),
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
                    .map(|field| Arc::new(self.build_schema_field(&info.module, field, &type_name)))
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
            let is_singleton =
                info.def.kind == coflow_language::cft::syntax::ast::TypeKind::Singleton;
            let is_host = has_annotation(&info.def.annotations, "Host");
            let id_as_enum = find_annotation(&info.def.annotations, "idAsEnum")
                .and_then(|annotation| annotation.args.first())
                .and_then(|arg| match arg {
                    AnnotationArg::Name(name) => Some(EnumName::from_validated(name.name.clone())),
                    _ => None,
                });
            let schema = CftType {
                kind: info.def.kind,
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
                is_host,
                id_as_enum,
                annotations: Self::schema_annotations(&info.def.annotations),
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
                .resolve_field_type(&field.ty)
                .value_type()
                .cloned()
                .unwrap_or(CftValueType::Unit),
            default: field
                .default
                .as_ref()
                .and_then(|default| self.schema_default_value(module, default)),
            is_expand: has_annotation(&field.annotations, "expand"),
            dimension,
            annotations: Self::schema_annotations(&field.annotations),
            display: display_metadata(&field.annotations),
            span: field.span,
        }
    }

    fn schema_annotations(annotations: &[Annotation]) -> Vec<CftAnnotation> {
        annotations
            .iter()
            .map(|annotation| CftAnnotation {
                name: annotation.name.clone(),
                arguments: annotation
                    .args
                    .iter()
                    .map(|argument| match argument {
                        AnnotationArg::Name(name) => CftAnnotationValue::Name(name.name.clone()),
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
            .defaults
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
        CftConstValue::Array(values) => {
            CftSchemaDefaultValue::Array(values.iter().map(const_value_as_default).collect())
        }
        CftConstValue::Dictionary(entries) => CftSchemaDefaultValue::Dictionary(
            entries
                .iter()
                .map(|(key, value)| (const_value_as_default(key), const_value_as_default(value)))
                .collect(),
        ),
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
    // 声明编译只保留检查源码，执行编译由独立空接口承接。
    fn convert_check_block(
        &self,
        _module: &crate::ModuleId,
        check: &coflow_language::cft::syntax::ast::CheckBlock,
    ) -> CftSchemaCheckBlock {
        CftSchemaCheckBlock {
            source: check.source.clone(),
            span: check.span,
        }
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
