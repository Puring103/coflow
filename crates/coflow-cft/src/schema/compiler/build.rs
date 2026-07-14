use super::SchemaCompiler;
use crate::ast::{AnnotationArg, DefaultExpr, DefaultExprKind, FieldDef};
use crate::schema::support::{
    build_schema_type_ref, convert_annotations, convert_check_block, find_annotation,
    format_type_ref, has_annotation,
};
use crate::schema::{
    CftConstValue, CftSchemaDefaultValue, CftSchemaEnum, CftSchemaEnumVariant, CftSchemaField,
    CftSchemaModule, CftSchemaType, Dimension, DimensionSpec, SchemaReflection,
};
use std::collections::BTreeMap;

impl SchemaCompiler<'_> {
    pub(super) fn build_schema(&self) -> SchemaReflection {
        let mut modules = self
            .modules
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
            let schema = crate::schema::CftSchemaConst {
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

        SchemaReflection {
            modules,
            consts,
            types,
            enums,
        }
    }

    fn build_schema_field(&self, field: &FieldDef, owner_type: &str) -> CftSchemaField {
        let localized = find_annotation(&field.annotations, "localized");
        let dimension_annotation = find_annotation(&field.annotations, "dimension");
        let dimension = localized
            .map(|_| DimensionSpec {
                kind: Dimension::Localized,
                bucket: localized_bucket(field).or_else(|| Some(owner_type.to_string())),
            })
            .or_else(|| {
                let annotation = dimension_annotation?;
                let Some(AnnotationArg::String(name, _)) = annotation.args.first() else {
                    return None;
                };
                Some(DimensionSpec {
                    kind: Dimension::Custom(name.clone()),
                    bucket: Some(owner_type.to_string()),
                })
            });
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
            dimension,
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
}

fn localized_bucket(field: &FieldDef) -> Option<String> {
    let annotation = find_annotation(&field.annotations, "localized")?;
    match annotation.args.first() {
        Some(AnnotationArg::String(bucket, _)) => Some(bucket.clone()),
        _ => None,
    }
}
