mod dimension_checks;
mod queries;

use crate::{
    CftAnnotation, CftConstValue, CftContainer, CftSchemaCheckBlock, CftSchemaEnum, CftSchemaType,
    CftSchemaTypeRef,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
pub struct CftSchemaView {
    pub consts: BTreeMap<String, CftConstValue>,
    pub types: BTreeMap<String, CftTypeMeta>,
    pub enums: BTreeMap<String, CftEnumMeta>,
    children_by_parent: BTreeMap<String, BTreeSet<String>>,
}

impl CftSchemaView {
    #[must_use]
    pub fn new(schema: &CftContainer) -> Self {
        let consts = schema
            .module_ids()
            .filter_map(|id| schema.schema(id))
            .flat_map(|module| module.consts.iter())
            .map(|schema_const| (schema_const.name.clone(), schema_const.value.clone()))
            .collect::<BTreeMap<_, _>>();

        let enums = schema
            .all_enums()
            .map(|schema_enum| {
                (
                    schema_enum.name.clone(),
                    CftEnumMeta::from_schema(schema_enum),
                )
            })
            .collect::<BTreeMap<_, _>>();

        let types = schema
            .all_types()
            .map(|schema_type| {
                let meta = CftTypeMeta::from_schema(schema_type);
                (meta.name.clone(), meta)
            })
            .collect::<BTreeMap<_, _>>();

        let children_by_parent = types.values().fold(
            BTreeMap::<String, BTreeSet<String>>::new(),
            |mut children, ty| {
                if let Some(parent) = &ty.parent {
                    children
                        .entry(parent.clone())
                        .or_default()
                        .insert(ty.name.clone());
                }
                children
            },
        );

        let mut view = Self {
            consts,
            types,
            enums,
            children_by_parent,
        };
        view.populate_dimension_checks();
        view
    }

    fn populate_dimension_checks(&mut self) {
        let names = self.types.keys().cloned().collect::<Vec<_>>();
        for name in &names {
            let checks = self.dimension_checks_for_type(name);
            if let Some(meta) = self.types.get_mut(name) {
                meta.dimension_checks = checks;
            }
        }
    }

    fn dimension_checks_for_type(&self, type_name: &str) -> BTreeMap<String, CftSchemaCheckBlock> {
        dimension_checks::dimension_checks_for_type(self, type_name)
    }

    #[must_use]
    pub fn type_meta(&self, type_name: &str) -> Option<&CftTypeMeta> {
        self.types.get(type_name)
    }

    pub fn type_names(&self) -> impl Iterator<Item = &String> {
        self.types.keys()
    }

    pub fn type_metas(&self) -> impl Iterator<Item = &CftTypeMeta> {
        self.types.values()
    }

    pub fn enum_names(&self) -> impl Iterator<Item = &String> {
        self.enums.keys()
    }

    pub fn enum_metas(&self) -> impl Iterator<Item = &CftEnumMeta> {
        self.enums.values()
    }

    #[must_use]
    pub fn enum_meta(&self, enum_name: &str) -> Option<&CftEnumMeta> {
        self.enums.get(enum_name)
    }

    #[must_use]
    pub fn has_type(&self, type_name: &str) -> bool {
        self.types.contains_key(type_name)
    }

    #[must_use]
    pub fn is_schema_enum(&self, name: &str) -> bool {
        self.enums.contains_key(name)
    }

    #[must_use]
    pub fn is_assignable(&self, actual_type: &str, expected_type: &str) -> bool {
        let mut current = Some(actual_type);
        while let Some(name) = current {
            if name == expected_type {
                return true;
            }
            current = self.types.get(name).and_then(|meta| meta.parent.as_deref());
        }
        false
    }

    #[must_use]
    pub fn enum_variant_value(&self, enum_name: &str, variant: &str) -> Option<i64> {
        self.enums
            .get(enum_name)
            .and_then(|meta| meta.variants.get(variant))
            .copied()
    }

    #[must_use]
    pub fn enum_value_from_int(&self, enum_name: &str, value: i64) -> Option<CftEnumValueMeta> {
        let meta = self.enums.get(enum_name)?;
        meta.variants
            .iter()
            .find(|(_, variant_value)| **variant_value == value)
            .map(|(variant, variant_value)| CftEnumValueMeta {
                enum_name: enum_name.to_string(),
                variant: Some(variant.clone()),
                value: *variant_value,
            })
    }

    #[must_use]
    pub fn checks_for_actual(
        &self,
        actual_type: &str,
        dimension: Option<&str>,
    ) -> Vec<CftSchemaCheckBlock> {
        if let Some(dimension) = dimension {
            return self
                .types
                .get(actual_type)
                .and_then(|meta| meta.dimension_checks.get(dimension))
                .cloned()
                .into_iter()
                .collect();
        }
        let mut chain = Vec::new();
        let mut current = Some(actual_type);
        while let Some(name) = current {
            let Some(meta) = self.types.get(name) else {
                break;
            };
            chain.push(meta);
            current = meta.parent.as_deref();
        }
        chain.reverse();
        chain
            .into_iter()
            .filter_map(|meta| meta.check.clone())
            .collect()
    }

    #[must_use]
    pub fn field_type(&self, actual_type: &str, field_name: &str) -> Option<&CftSchemaTypeRef> {
        self.field_meta(actual_type, field_name)
            .map(|field| &field.ty_ref)
    }

    #[must_use]
    pub fn field_meta(&self, actual_type: &str, field_name: &str) -> Option<&CftFieldMeta> {
        self.types
            .get(actual_type)?
            .all_fields
            .iter()
            .find(|field| field.name == field_name)
    }

    #[must_use]
    pub fn full_fields(&self, type_name: &str) -> Option<&[CftFieldMeta]> {
        self.types
            .get(type_name)
            .map(|meta| meta.all_fields.as_slice())
    }

    #[must_use]
    pub fn has_descendants(&self, type_name: &str) -> bool {
        self.children_by_parent
            .get(type_name)
            .is_some_and(|children| !children.is_empty())
    }

    pub fn descendant_names(&self, type_name: &str) -> impl Iterator<Item = &String> {
        self.children_by_parent.get(type_name).into_iter().flatten()
    }

    #[must_use]
    pub fn range_is_polymorphic(&self, type_name: &str) -> bool {
        self.types
            .get(type_name)
            .is_some_and(|meta| meta.is_abstract || self.has_descendants(type_name))
    }

    #[must_use]
    pub fn assignable_target_names(&self, actual_type: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut current = Some(actual_type);
        while let Some(name) = current {
            out.push(name.to_string());
            current = self.types.get(name).and_then(|meta| meta.parent.as_deref());
        }
        out
    }

    pub fn singleton_types(&self) -> impl Iterator<Item = &CftTypeMeta> {
        self.types.values().filter(|meta| meta.is_singleton)
    }

    #[must_use]
    pub fn concrete_assignable_types(&self, type_name: &str) -> Option<Vec<String>> {
        let mut out = Vec::new();
        let meta = self.types.get(type_name)?;
        if !meta.is_abstract {
            out.push(type_name.to_string());
        }
        self.collect_concrete_descendants(type_name, &mut out);
        Some(out)
    }

    fn collect_concrete_descendants(&self, type_name: &str, out: &mut Vec<String>) {
        for child in self.descendant_names(type_name) {
            let Some(child_meta) = self.types.get(child) else {
                continue;
            };
            if !child_meta.is_abstract {
                out.push(child.clone());
            }
            self.collect_concrete_descendants(child, out);
        }
    }

    #[must_use]
    pub fn dimension_field(
        &self,
        actual_type: &str,
        field_name: &str,
    ) -> Option<&CftDimensionFieldMeta> {
        self.types
            .get(actual_type)
            .and_then(|meta| meta.dimension_fields.get(field_name))
    }
}

#[derive(Debug, Clone)]
pub struct CftTypeMeta {
    pub module: String,
    pub name: String,
    pub parent: Option<String>,
    pub is_abstract: bool,
    pub is_sealed: bool,
    pub is_singleton: bool,
    pub annotations: Vec<CftAnnotation>,
    pub check: Option<CftSchemaCheckBlock>,
    pub dimension_checks: BTreeMap<String, CftSchemaCheckBlock>,
    pub own_fields: Vec<CftFieldMeta>,
    pub all_fields: Vec<CftFieldMeta>,
    pub fields: BTreeMap<String, CftSchemaTypeRef>,
    pub dimension_fields: BTreeMap<String, CftDimensionFieldMeta>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CftFieldMeta {
    pub name: String,
    pub raw_type: String,
    pub ty_ref: CftSchemaTypeRef,
    pub has_default: bool,
    pub default: Option<crate::CftSchemaDefaultValue>,
    pub annotations: Vec<CftAnnotation>,
    pub dimension: Option<CftDimensionFieldMeta>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CftDimensionFieldMeta {
    pub dimension: String,
    pub bucket: Option<String>,
}

impl CftTypeMeta {
    fn from_schema(schema_type: &CftSchemaType) -> Self {
        let dimension_fields = schema_type
            .all_fields
            .iter()
            .filter_map(|field| {
                let dimension = field.dimension.as_ref().map(|d| d.kind.name())?;
                Some((
                    field.name.clone(),
                    CftDimensionFieldMeta {
                        dimension: dimension.to_string(),
                        bucket: field.dimension.as_ref().and_then(|d| d.bucket.clone()),
                    },
                ))
            })
            .collect();
        Self {
            module: schema_type.module.to_string(),
            name: schema_type.name.clone(),
            parent: schema_type.parent.clone(),
            is_abstract: schema_type.is_abstract,
            is_sealed: schema_type.is_sealed,
            is_singleton: schema_type.is_singleton,
            annotations: schema_type.annotations.clone(),
            check: schema_type.check.clone(),
            dimension_checks: BTreeMap::new(),
            own_fields: schema_type
                .fields
                .iter()
                .map(CftFieldMeta::from_schema)
                .collect(),
            all_fields: schema_type
                .all_fields
                .iter()
                .map(CftFieldMeta::from_schema)
                .collect(),
            fields: schema_type
                .all_fields
                .iter()
                .map(|field| (field.name.clone(), field.ty_ref.clone()))
                .collect(),
            dimension_fields,
        }
    }
}

impl CftFieldMeta {
    fn from_schema(field: &crate::CftSchemaField) -> Self {
        Self {
            name: field.name.clone(),
            raw_type: field.ty.clone(),
            ty_ref: field.ty_ref.clone(),
            has_default: field.has_default,
            default: field.default.clone(),
            annotations: field.annotations.clone(),
            dimension: field
                .dimension
                .as_ref()
                .map(|dimension| CftDimensionFieldMeta {
                    dimension: dimension.kind.name().to_string(),
                    bucket: dimension.bucket.clone(),
                }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CftEnumValueMeta {
    pub enum_name: String,
    pub variant: Option<String>,
    pub value: i64,
}

#[derive(Debug, Clone)]
pub struct CftEnumMeta {
    pub module: String,
    pub name: String,
    pub annotations: Vec<CftAnnotation>,
    pub all_variants: Vec<CftEnumVariantMeta>,
    pub variants: BTreeMap<String, i64>,
}

#[derive(Debug, Clone)]
pub struct CftEnumVariantMeta {
    pub name: String,
    pub value: i64,
    pub annotations: Vec<CftAnnotation>,
}

impl CftEnumMeta {
    fn from_schema(schema_enum: &CftSchemaEnum) -> Self {
        Self {
            module: schema_enum.module.to_string(),
            name: schema_enum.name.clone(),
            annotations: schema_enum.annotations.clone(),
            all_variants: schema_enum
                .variants
                .iter()
                .map(|variant| CftEnumVariantMeta {
                    name: variant.name.clone(),
                    value: variant.value,
                    annotations: variant.annotations.clone(),
                })
                .collect(),
            variants: schema_enum
                .variants
                .iter()
                .map(|variant| (variant.name.clone(), variant.value))
                .collect(),
        }
    }
}
