use super::SchemaCompiler;
use crate::ast::{AnnotationArg, DefaultExpr, DefaultExprKind, FieldDef};
use crate::compiled::support::{
    build_schema_type_ref, convert_check_block, find_annotation, has_annotation,
};
use crate::compiled::{
    CftConst, CftConstValue, CftEnum, CftEnumVariant, CftField, CftFieldDimension,
    CftSchemaDefaultValue, CftType, CompiledSchema,
};
use crate::{BucketName, ConstName, DimensionName, EnumName, EnumVariantName, FieldName, TypeName};
use std::collections::BTreeMap;
use std::sync::Arc;

impl SchemaCompiler<'_> {
    pub(super) fn build_schema(&self) -> CompiledSchema {
        let mut consts = BTreeMap::new();
        let mut enums = BTreeMap::new();

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
                    AnnotationArg::Name(name) => {
                        Some(EnumName::from_validated(name.name.clone()))
                    }
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
                check: info.def.check.as_ref().map(convert_check_block),
                dimension_checks: BTreeMap::new(),
                span: info.def.span,
            };
            types.insert(type_name, schema);
        }

        CompiledSchema { consts, types, enums }
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
            ty_ref: build_schema_type_ref(&field.ty, &|name| self.enums.contains_key(name)),
            has_default: field.default.is_some(),
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

fn localized_bucket(field: &FieldDef) -> Option<BucketName> {
    let annotation = find_annotation(&field.annotations, "localized")?;
    match annotation.args.first() {
        Some(AnnotationArg::String(bucket, _)) => {
            Some(BucketName::from_validated(bucket.clone()))
        }
        _ => None,
    }
}
